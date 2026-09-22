//! Local diagnostic log (D-158): timings and outcomes for everything the launcher does,
//! so a problem can be explained after the fact instead of guessed at.
//!
//! Nothing leaves the machine. Lines go to `<app local data>/logs/launcher.log`, which
//! rotates at 2 MB to `launcher.1.log`, and the last few hundred are kept in memory so
//! the Logs page can show them without touching the disk. The whole thing can be
//! switched off in Settings (D-169).
//!
//! Volume is the point of failure for a log like this: anything on a per-row or per-frame
//! path must not call it. The call sites are starts, ends, counts and errors.

use std::fmt;
use std::fs::{self, OpenOptions};
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
/// first `:`, so muting `ui` silences `ui:ipc` and `ui:window` together.
static MUTED: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn set_muted(areas: Vec<String>) {
    if let Ok(mut m) = MUTED.lock() {
        *m = areas;
    }
}

fn muted(target: &str) -> bool {
    let area = target.split_once(':').map_or(target, |(a, _)| a);
    MUTED
        .lock()
        .map(|m| m.iter().any(|x| x == area))
        .unwrap_or(false)
}

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const RING: usize = 400;

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
    /// Area: "steam", "a2s", "cache", "launch", "news", "ui", …
    pub target: String,
    pub message: String,
}

struct Sink {
    path: Option<PathBuf>,
    ring: Vec<Entry>,
}

static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

fn sink() -> &'static Mutex<Sink> {
    SINK.get_or_init(|| {
        Mutex::new(Sink {
            path: None,
            ring: Vec::with_capacity(RING),
        })
    })
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
    if fs::create_dir_all(&logs).is_err() {
        return;
    }
    let path = logs.join("launcher.log");
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > MAX_BYTES {
            let _ = fs::rename(&path, logs.join("launcher.1.log"));
        }
    }
    if let Ok(mut s) = sink().lock() {
        s.path = Some(path);
    }
}

pub fn path() -> Option<PathBuf> {
    sink().lock().ok().and_then(|s| s.path.clone())
}

/// Appends one entry. Never panics and never blocks on a poisoned lock.
pub fn write(level: Level, target: &str, message: impl Into<String>) {
    if !enabled() || muted(target) {
        return;
    }
    let entry = Entry {
        at: now_ms(),
        level,
        target: target.to_string(),
        message: message.into(),
    };

    #[cfg(debug_assertions)]
    eprintln!("[{}] {}: {}", entry.level, entry.target, entry.message);

    let Ok(mut s) = sink().lock() else { return };
    if s.ring.len() == RING {
        s.ring.remove(0);
    }
    s.ring.push(entry.clone());

    let Some(path) = s.path.clone() else { return };
    drop(s);
    let line = format!(
        "{} {:<5} {:<8} {}\n",
        stamp(entry.at),
        entry.level.to_string(),
        entry.target,
        entry.message
    );
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
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

    #[test]
    fn ring_keeps_the_most_recent_and_never_grows() {
        for i in 0..(RING + 50) {
            write(Level::Info, "test", format!("entry {i}"));
        }
        let all = recent(RING * 2);
        assert!(all.len() <= RING, "ring is capped");
        assert!(
            all.last()
                .unwrap()
                .message
                .ends_with(&format!("{}", RING + 49)),
            "newest entry is last"
        );
        assert_eq!(recent(5).len(), 5, "limit respected");
    }

    #[test]
    fn stamp_is_a_readable_utc_timestamp() {
        // 1 789 084 800 123 ms = 2026-09-11 00:00:00.123 UTC.
        let s = stamp(1_789_084_800_123);
        assert!(s.starts_with("2026-09-11 00:00:00"), "got {s}");
        assert!(s.ends_with(".123"), "got {s}");
    }
}
