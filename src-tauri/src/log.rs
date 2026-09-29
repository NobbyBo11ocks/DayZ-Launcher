//! Local diagnostic log (D-158): timings and outcomes for everything the launcher does,
//! so a problem can be explained after the fact instead of guessed at.
//!
//! Nothing leaves the machine. Lines go to `<app local data>/logs/launcher.log`, which
//! rotates at 2 MB to `launcher.1.log`, and the last few hundred are kept in memory so
//! the Logs page can show them without touching the disk. The whole thing can be
//! switched off on the Logs page (D-169).
//!
//! Volume is the point of failure for a log like this: anything on a per-row or per-frame
//! path must not call it. The call sites are starts, ends, counts and errors.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use serde::Serialize;

/// Off means nothing is recorded at all — no file, no memory ring (D-169). The
/// setting drives it; it starts on so a failure during start-up is still caught.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Turns recording on or off. Switching it off also drops what is already in memory,
/// so "no logging" means exactly that.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if !on {
        if let Ok(mut s) = sink().lock() {
            s.ring.clear();
        }
    }
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Areas the user has switched off (D-172). An entry's area is its target up to the
/// first `:`, folded by `area` into the Logs page chip that covers it, so muting `app`
/// silences `ui:ipc`, `ui:window`, `ipc`, `news` and `update` together. Muting
/// silences an area's information lines only: its warnings and errors are what every
/// "The details are on the Logs page" points at, and a muted App lost them (row 25,
/// approved).
static MUTED: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn set_muted(areas: Vec<String>) {
    if let Ok(mut m) = MUTED.lock() {
        *m = areas;
    }
}

/// The Logs page's area for a target: its head up to the first `:`, with the heads that
/// have no chip of their own folded into the chip that says it covers them. App covers
/// "anything the interface reports" (every `ui:…` target), the news feed and the
/// updater's clean-up, and Mods covers junctions. With the raw head, muting App or Mods
/// left those entries recorded, and "junctions", "news" and "update" could not be muted
/// at all (D-256, D-287); the row stream's own `ipc` lines had no chip either (row 25).
/// `Logs.svelte` mirrors this.
pub fn area(target: &str) -> &str {
    let head = target.split_once(':').map_or(target, |(a, _)| a);
    match head {
        "ui" | "ipc" | "news" | "update" => "app",
        "junctions" => "mods",
        other => other,
    }
}

fn muted(target: &str) -> bool {
    let area = area(target);
    MUTED
        .lock()
        .map(|m| m.iter().any(|x| x == area))
        .unwrap_or(false)
}

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const RING: usize = 400;
/// A message is cut to this many characters, after the profile folder is taken out of
/// it: cut first, a path straddling the cut kept part of the account name (row 25).
const MAX_MESSAGE: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Unix milliseconds.
    pub at: u64,
    pub level: Level,
    /// "steam", "verify", "cache:prune", "ui:ipc", …; its area is the part before the
    /// first `:`, folded by `area`.
    pub target: String,
    pub message: String,
}

struct Sink {
    path: Option<PathBuf>,
    /// The file, opened once and kept: each line opened, sized and closed it before, and
    /// writers that raced between those steps rotated each other's files away (row 25).
    file: Option<File>,
    /// Bytes in the file, so the cap needs no call per line.
    size: u64,
    max_bytes: u64,
    /// Why the file cannot be written, while it cannot: it stopped without a word, and
    /// the page went on showing lines kept only in memory (row 25).
    file_error: Option<String>,
    ring: Vec<Entry>,
}

impl Sink {
    fn new(path: Option<PathBuf>, max_bytes: u64) -> Self {
        Self {
            path,
            file: None,
            size: 0,
            max_bytes,
            file_error: None,
            ring: Vec::with_capacity(RING),
        }
    }

    /// Opens the file, making its folder again if it went: a player who deletes the logs
    /// folder to start clean had no file for the rest of the session (row 25).
    fn open(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(f) => {
                self.size = f.metadata().map(|m| m.len()).unwrap_or(0);
                self.file = Some(f);
            }
            Err(e) => self.file_error = Some(e.to_string()),
        }
    }

    /// Appends one line under the lock, and rotates past the cap. A failed write drops the
    /// handle and tries once more with a fresh one.
    fn append(&mut self, line: &[u8]) {
        if self.path.is_none() {
            return;
        }
        for attempt in 0..2 {
            if self.file.is_none() {
                self.open();
            }
            let Some(f) = self.file.as_mut() else {
                return;
            };
            match f.write_all(line) {
                Ok(()) => {
                    self.size += line.len() as u64;
                    self.file_error = None;
                    break;
                }
                Err(e) => {
                    self.file = None;
                    self.file_error = Some(e.to_string());
                    if attempt == 1 {
                        return;
                    }
                }
            }
        }
        if self.size > self.max_bytes {
            self.rotate();
        }
    }

    /// `launcher.log` becomes `launcher.1.log`. When the rename is refused (a viewer
    /// holding the file without delete sharing) the file is emptied instead: the rename
    /// was tried again on every line while the file grew past the cap (row 25).
    fn rotate(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.file = None;
        self.size = 0;
        if let Err(e) = fs::rename(&path, path.with_file_name("launcher.1.log")) {
            if let Err(t) = OpenOptions::new().write(true).truncate(true).open(&path) {
                self.file_error = Some(format!(
                    "the log could not be rotated ({e}) or emptied ({t})"
                ));
            }
        }
    }
}

static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

fn sink() -> &'static Mutex<Sink> {
    SINK.get_or_init(|| Mutex::new(Sink::new(None, MAX_BYTES)))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Points the log at `<dir>/logs/launcher.log`. Safe to call once at start-up; without
/// it the log still works in memory, which keeps unit tests and early calls harmless.
pub fn init(dir: &std::path::Path) {
    let logs = dir.join("logs");
    let path = logs.join("launcher.log");
    // The folder is made again at the first line when it cannot be made now.
    let _ = fs::create_dir_all(&logs);
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > MAX_BYTES {
            let _ = fs::rename(&path, logs.join("launcher.1.log"));
        }
    }
    if let Ok(mut s) = sink().lock() {
        s.path = Some(path);
        s.file = None;
    }
}

pub fn path() -> Option<PathBuf> {
    sink().lock().ok().and_then(|s| s.path.clone())
}

/// Why the file cannot be written, while it cannot.
pub fn file_error() -> Option<String> {
    sink().lock().ok().and_then(|s| s.file_error.clone())
}

/// The player's profile folder as it appears in paths (`C:\Users\<name>`), read once:
/// its long and short (8.3) forms, each with forward slashes and with the doubled
/// backslashes of a `{:?}`-printed path. `%TEMP%` is the short form for a long account
/// name (`C:\Users\ADMINI~1\…`), and a temp file's error prints it debug-quoted, so
/// six letters of the name reached the log past the two forms kept before (row 25).
fn profile_forms() -> &'static [String] {
    static FORMS: OnceLock<Vec<String>> = OnceLock::new();
    FORMS.get_or_init(|| {
        let Some(p) = std::env::var_os("USERPROFILE").map(|p| p.to_string_lossy().into_owned())
        else {
            return Vec::new();
        };
        let mut bases = vec![p.trim_end_matches(['\\', '/']).to_string()];
        if let Some(short) = short_path(&bases[0]) {
            bases.push(short.trim_end_matches(['\\', '/']).to_string());
        }
        path_forms(&bases)
    })
}

/// Every way `bases` are written in a message, longest first so a longer form is taken
/// before a shorter one inside it.
fn path_forms(bases: &[String]) -> Vec<String> {
    let mut forms: Vec<String> = bases
        .iter()
        .filter(|b| b.len() >= 4)
        .flat_map(|b| [b.clone(), b.replace('\\', "/"), b.replace('\\', "\\\\")])
        .collect();
    forms.sort_by_key(|f| std::cmp::Reverse(f.len()));
    forms.dedup();
    forms
}

/// The 8.3 form of a path, when Windows keeps one.
fn short_path(long: &str) -> Option<String> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
    let wide: Vec<u16> = std::ffi::OsStr::new(long)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut buf = vec![0u16; 1024];
    // SAFETY: `wide` ends in a NUL and `buf` holds as many units as its length says.
    let n =
        unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    let short = std::ffi::OsString::from_wide(&buf[..n])
        .to_string_lossy()
        .into_owned();
    (!short.eq_ignore_ascii_case(long)).then_some(short)
}

/// `%USERPROFILE%` for the player's own profile folder, so a log shared in a bug
/// report does not carry the Windows account name (row 21). Matched without regard to
/// ASCII case, as Windows paths are, and only as a whole folder: for a profile
/// `C:\Users\Al`, `C:\Users\Alice` became `%USERPROFILE%ice` (row 25).
fn redact(s: &str, forms: &[String]) -> String {
    let mut out = s.to_string();
    for form in forms {
        let mut from = 0;
        loop {
            let found = out[from..]
                .char_indices()
                .map(|(i, _)| from + i)
                .find(|&i| {
                    out.len() - i >= form.len()
                        && out.is_char_boundary(i + form.len())
                        && out[i..i + form.len()].eq_ignore_ascii_case(form)
                        && !out[i + form.len()..]
                            .chars()
                            .next()
                            .is_some_and(char::is_alphanumeric)
                });
            let Some(at) = found else { break };
            out.replace_range(at..at + form.len(), "%USERPROFILE%");
            from = at + "%USERPROFILE%".len();
        }
    }
    out
}

/// Collapses the control characters that let a message forge log lines.
///
/// Most of what reaches this sink is ours, but not all of it: a server name arrives
/// from A2S_INFO or the DZSA JSON and goes straight into the "join" and "verify"
/// entries, and `log_ui` takes whatever the WebView sends. A name containing a
/// newline and a plausible timestamp writes entries indistinguishable from real ones,
/// in the file the user copies into a support report. The WebView's own path has
/// flattened since D-163; the Rust macros never did (D-221). Cut to `MAX_MESSAGE` after
/// the profile folder is out of it.
fn flatten(s: &str) -> String {
    let s = redact(s, profile_forms());
    let mut out = String::with_capacity(s.len().min(MAX_MESSAGE * 4));
    for ch in s.chars().take(MAX_MESSAGE) {
        match ch {
            '\n' | '\r' => out.push_str("\\n"),
            '\t' => out.push(' '),
            c if c.is_control() => out.push(char::REPLACEMENT_CHARACTER),
            c => out.push(c),
        }
    }
    out
}

/// Appends one entry. Never panics and never blocks on a poisoned lock.
pub fn write(level: Level, target: &str, message: impl Into<String>) {
    write_at(level, target, message.into(), now_ms(), false);
}

/// The panic hook's line, past the muted areas: the dialog sends the player to the
/// log, and a muted App dropped the one line that says what happened (row 25).
pub fn write_unmuted(level: Level, target: &str, message: impl Into<String>) {
    write_at(level, target, message.into(), now_ms(), true);
}

/// A line from the page, at the time the page wrote it when that is plausible: stamped
/// on arrival, two lines the page sent in order could swap (row 25).
pub fn write_from_page(level: Level, target: &str, message: impl Into<String>, at: Option<u64>) {
    let now = now_ms();
    let at = at.filter(|&t| t.abs_diff(now) < 60_000).unwrap_or(now);
    write_at(level, target, message.into(), at, false);
}

fn write_at(level: Level, target: &str, message: String, at: u64, unmuted: bool) {
    if !enabled() || (!unmuted && level == Level::Info && muted(target)) {
        return;
    }
    let entry = Entry {
        at,
        level,
        target: target.to_string(),
        message: flatten(&message),
    };

    #[cfg(debug_assertions)]
    eprintln!("[{}] {}: {}", entry.level, entry.target, entry.message);

    let line = format!(
        "{} {:<5} {:<8} {}\n",
        stamp(entry.at),
        entry.level.to_string(),
        entry.target,
        entry.message
    );
    let Ok(mut s) = sink().lock() else { return };
    if s.ring.len() == RING {
        s.ring.remove(0);
    }
    s.ring.push(entry);
    // Under the lock, so the file's order is the ring's and a rotation is one writer's.
    // The cap was only ever applied in `init`, so a long session had no bound at all -
    // and the fastest writer is `log_ui`, which the WebView calls on every
    // `console.error`. A page in an error loop wrote until the disk filled (D-221).
    s.append(line.as_bytes());
}

/// `2026-09-22 12:34:56.789` in UTC, computed without a date crate. UTC keeps a log
/// comparable across machines and avoids a DST discontinuity in the middle of a session.
fn stamp(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let millis = ms % 1000;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Civil date from days since epoch (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{d:02} {h:02}:{m:02}:{s:02}.{millis:03}")
}

/// Most recent entries, newest last.
pub fn recent(limit: usize) -> Vec<Entry> {
    let Ok(s) = sink().lock() else {
        return Vec::new();
    };
    let start = s.ring.len().saturating_sub(limit);
    s.ring[start..].to_vec()
}

#[macro_export]
macro_rules! log_info {
    ($target:expr, $($arg:tt)*) => { $crate::log::write($crate::log::Level::Info, $target, format!($($arg)*)) };
}
#[macro_export]
macro_rules! log_warn {
    ($target:expr, $($arg:tt)*) => { $crate::log::write($crate::log::Level::Warn, $target, format!($($arg)*)) };
}
#[macro_export]
macro_rules! log_error {
    ($target:expr, $($arg:tt)*) => { $crate::log::write($crate::log::Level::Error, $target, format!($($arg)*)) };
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Row 21: a path under the player's profile is logged as %USERPROFILE%, whatever
    /// its case or slashes; other paths are left alone.
    #[test]
    fn the_profile_folder_is_not_written_into_the_log() {
        let forms = vec![
            r"C:\Users\Zoë Admin".to_string(),
            "C:/Users/Zoë Admin".to_string(),
        ];
        assert_eq!(
            redact(
                r"open C:\Users\Zoë Admin\AppData\x.log and c:\users\zoë admin\y",
                &forms
            ),
            r"open %USERPROFILE%\AppData\x.log and %USERPROFILE%\y"
        );
        assert_eq!(
            redact("C:/Users/Zoë Admin/Documents", &forms),
            "%USERPROFILE%/Documents"
        );
        assert_eq!(
            redact(r"G:\SteamLibrary\steamapps", &forms),
            r"G:\SteamLibrary\steamapps"
        );
    }

    /// Row 25: the short 8.3 form, a path printed with `{:?}` (backslashes doubled), and
    /// only a whole folder: another account whose name starts the same stays as it is.
    #[test]
    fn every_form_of_the_profile_is_taken_out() {
        let forms = path_forms(&[
            r"C:\Users\Zoë Admin".to_string(),
            r"C:\Users\ZOADMI~1".to_string(),
        ]);
        assert_eq!(
            redact(
                r"at path C:\Users\ZOADMI~1\AppData\Local\Temp\x.tmp",
                &forms
            ),
            r"at path %USERPROFILE%\AppData\Local\Temp\x.tmp"
        );
        assert_eq!(
            redact(r#"at path "C:\\Users\\Zoë Admin\\x""#, &forms),
            r#"at path "%USERPROFILE%\\x""#
        );
        assert_eq!(
            redact(r"C:\Users\Zoë Admin2\x", &forms),
            r"C:\Users\Zoë Admin2\x"
        );
        assert_eq!(redact(r"C:\Users\Zoë Admin", &forms), "%USERPROFILE%");
    }

    /// Row 25: the file is made again when its folder went, and a line cut to the cap
    /// is cut after the profile is out of it.
    #[test]
    fn the_file_comes_back_when_its_folder_is_deleted() {
        let dir = std::env::temp_dir().join(format!("dzl-log-{}", std::process::id()));
        let path = dir.join("logs").join("launcher.log");
        let mut s = Sink::new(Some(path.clone()), MAX_BYTES);
        s.append(b"one\n");
        drop(s.file.take());
        let _ = fs::remove_dir_all(&dir);
        s.append(b"two\n");
        let text = fs::read_to_string(&path).unwrap_or_default();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(text, "two\n");
        assert!(s.file_error.is_none());
    }

    /// Row 25: eight writers at once, the file rotated many times: every line whole, none
    /// twice, and neither file past the cap.
    #[test]
    fn writers_racing_a_rotation_lose_no_whole_line() {
        let dir = std::env::temp_dir().join(format!("dzl-rot-{}", std::process::id()));
        let path = dir.join("launcher.log");
        let sink = std::sync::Arc::new(Mutex::new(Sink::new(Some(path.clone()), 16 * 1024)));
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let sink = std::sync::Arc::clone(&sink);
                std::thread::spawn(move || {
                    for i in 0..2000 {
                        let line = format!("writer {t} line {i:04} {}\n", "x".repeat(40));
                        sink.lock().unwrap().append(line.as_bytes());
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let mut lines = Vec::new();
        for f in [path.clone(), path.with_file_name("launcher.1.log")] {
            lines.extend(
                fs::read_to_string(f)
                    .unwrap_or_default()
                    .lines()
                    .map(str::to_string),
            );
        }
        let sizes: Vec<u64> = [path.clone(), path.with_file_name("launcher.1.log")]
            .iter()
            .map(|f| fs::metadata(f).map(|m| m.len()).unwrap_or(0))
            .collect();
        let _ = fs::remove_dir_all(&dir);
        assert!(
            sizes.iter().all(|&n| n <= 16 * 1024 + 100),
            "a file past the cap: {sizes:?}"
        );
        let whole = lines
            .iter()
            .all(|l| l.starts_with("writer ") && l.ends_with(&"x".repeat(40)));
        assert!(whole, "a line was cut or interleaved");
        let set: std::collections::HashSet<&String> = lines.iter().collect();
        assert_eq!(set.len(), lines.len(), "a line written twice");
        assert!(
            lines.len() > 200,
            "at least a file's worth kept: {}",
            lines.len()
        );
    }

    #[test]
    fn ring_keeps_the_most_recent_and_never_grows() {
        for i in 0..(RING + 50) {
            write(Level::Info, "ringtest", format!("entry {i}"));
        }
        let all = recent(RING * 2);
        assert!(all.len() <= RING, "ring is capped");
        // Only its own lines: other tests log in parallel into the same ring (row 25).
        let own: Vec<&Entry> = all.iter().filter(|e| e.target == "ringtest").collect();
        assert!(
            own.last()
                .unwrap()
                .message
                .ends_with(&format!("{}", RING + 49)),
            "newest entry is last"
        );
        assert_eq!(recent(5).len(), 5, "limit respected");
    }

    /// Row 25 (approved): a muted area loses its information lines, never its warnings
    /// or errors.
    #[test]
    fn muting_keeps_warnings_and_errors() {
        set_muted(vec!["mutetest".into()]);
        write(Level::Info, "mutetest", "row25-mute-info");
        write(Level::Warn, "mutetest:sub", "row25-mute-warn");
        write(Level::Error, "mutetest", "row25-mute-error");
        set_muted(Vec::new());
        let seen: Vec<String> = recent(RING)
            .into_iter()
            .filter(|e| e.message.starts_with("row25-mute"))
            .map(|e| e.message)
            .collect();
        assert_eq!(seen, ["row25-mute-warn", "row25-mute-error"]);
    }

    #[test]
    fn areas_without_a_chip_fold_into_the_one_that_covers_them() {
        assert_eq!(area("ui:ipc"), "app");
        assert_eq!(area("ipc"), "app");
        assert_eq!(area("ui"), "app");
        assert_eq!(area("news"), "app");
        assert_eq!(area("update"), "app");
        assert_eq!(area("junctions"), "mods");
        assert_eq!(area("steam"), "steam");
        assert_eq!(area("cache:prune"), "cache");
    }

    #[test]
    fn stamp_is_a_readable_utc_timestamp() {
        // 1 789 084 800 123 ms = 2026-09-11 00:00:00.123 UTC.
        let s = stamp(1_789_084_800_123);
        assert!(s.starts_with("2026-09-11 00:00:00"), "got {s}");
        assert!(s.ends_with(".123"), "got {s}");
    }
}
