//! Settings persisted as JSON in the app config directory (docs/05 §2): launch
//! options plus the UI preferences (theme, accent, filters, onboarding, last update
//! check, the News page's switch and its last-seen post, the version whose notes were
//! last shown). The UI ones used to live
//! only in the WebView's localStorage, which can be
//! reset or detached from the app; the file is the source of truth and localStorage
//! is an instant-start cache (D-070).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Where the installer leaves its one-shot choices: `${MANUPRODUCTKEY}` in the NSIS
/// template, which is `HKCU\Software\<manufacturer>\<product name>` (D-206).
const INSTALL_CHOICES_KEY: &str = r"Software\dayzlauncher\DZSA CrayZ Launcher";

const THEMES: [&str; 2] = ["slate", "light"];
// Keep in step with `ACCENTS` in src/lib/state/prefs.svelte.ts and the tokens in src/app.css.
const ACCENTS: [&str; 12] = [
    "amber", "orange", "red", "rose", "pink", "violet", "indigo", "blue", "sky", "teal", "green",
    "lime",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UiPrefs {
    pub theme: String,
    pub accent: String,
    pub onboarded: bool,
    /// Server browser filters exactly as the frontend stores them (opaque object).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Value>,
    pub last_update_check_ms: u64,
    /// Unix seconds of the newest news post the user has seen (News tab, D-099).
    #[serde(default)]
    pub news_seen: i64,
    /// Set once the one-time move off the old amber default has run (D-132). Without
    /// it, every existing install would keep amber for ever, since the saved value
    /// cannot be told apart from a deliberate choice.
    #[serde(default)]
    pub accent_default_v2: bool,
    /// Show the News page at all (D-174). Off removes it from the sidebar and stops
    /// the launcher fetching Steam's news feed, its pictures and YouTube previews.
    pub news: bool,
    /// The launcher version whose "What's new" notes were last shown (D-301). Empty in
    /// a file written before 0.1.78, which the front end reads as an update to this one.
    pub last_seen_version: String,
    /// Keys this build does not know, kept as they came: a newer version's, which an
    /// older one dropped at its first write — a downgrade and an upgrade again lost them,
    /// and one-time moves ran twice (row 15, H4). Never rename a key or change its type;
    /// add a new one (docs/05 §2).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            theme: "slate".into(),
            // Lime by default (D-132); mirrored by DEFAULT_ACCENT in prefs.svelte.ts.
            accent: "lime".into(),
            onboarded: false,
            filters: None,
            last_update_check_ms: 0,
            news_seen: 0,
            accent_default_v2: false,
            news: true,
            last_seen_version: String::new(),
            extra: serde_json::Map::new(),
        }
    }
}

impl UiPrefs {
    /// Unknown theme/accent names fall back to the defaults; filters must be an object.
    fn normalise(&mut self) {
        if !THEMES.contains(&self.theme.as_str()) {
            self.theme = "slate".into();
        }
        if !ACCENTS.contains(&self.accent.as_str()) {
            self.accent = "lime".into();
        }
        if !matches!(self.filters, Some(Value::Object(_)) | None) {
            self.filters = None;
        }
        // One-time: anyone still on the old default moves to the new one, every other
        // accent is left alone (D-132).
        if !self.accent_default_v2 {
            if self.accent == "amber" {
                self.accent = "lime".into();
            }
            self.accent_default_v2 = true;
        }
    }
}

/// A saved preset of the launch options (D-088).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LaunchProfile {
    pub name: String,
    pub profile_name: String,
    pub extra_args: String,
    pub skip_intro: bool,
    pub no_splash: bool,
    pub no_pause: bool,
    /// Keys this build does not know (row 15, H4).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl Default for LaunchProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            profile_name: String::new(),
            extra_args: String::new(),
            skip_intro: true,
            no_splash: true,
            no_pause: false,
            extra: serde_json::Map::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// `-name=`; empty means "use the Steam persona".
    pub profile_name: String,
    /// Raw extra arguments appended last (docs/02 §5.4).
    pub extra_args: String,
    pub skip_intro: bool,
    pub no_splash: bool,
    pub no_pause: bool,
    /// Saved presets of the five launch options above; the join dialog can launch
    /// with one of them for a single launch (D-088).
    pub launch_profiles: Vec<LaunchProfile>,
    /// Release the Steamworks session after this many idle minutes (0 = never). While
    /// connected, Steam shows the user as playing DayZ and counts playtime (D-077).
    pub steam_idle_minutes: u32,
    /// Record what the launcher does to `logs/launcher.log` and the Logs page. Off
    /// means nothing is written or kept at all (D-169).
    pub logging: bool,
    /// Log areas the user switched off (`steam`, `mods`, `join`, …). Muted areas are
    /// dropped at the source, so they cost nothing (D-172).
    #[serde(default)]
    pub log_muted: Vec<String>,
    /// Set once the one-time move from the old 15-minute idle release to 5 has run
    /// (D-299), as `accent_default_v2` did for the accent (D-132): a saved 15 cannot be
    /// told apart from the old default.
    #[serde(default)]
    pub idle_default_v2: bool,
    pub ui: UiPrefs,
    /// Keys this build does not know (row 15, H4).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            profile_name: String::new(),
            extra_args: String::new(),
            skip_intro: true,
            no_splash: true,
            no_pause: false,
            launch_profiles: Vec::new(),
            // 5, not 15 (D-299): the session holds 9 MB after a start and ~30 MB after a
            // full Refresh until it is released (D-298), and Steam shows the player in
            // DayZ for as long as it lasts. Whatever needs Steam opens it again.
            steam_idle_minutes: 5,
            logging: true,
            log_muted: Vec::new(),
            idle_default_v2: false,
            ui: UiPrefs::default(),
            extra: serde_json::Map::new(),
        }
    }
}

impl Settings {
    /// What `UiPrefs::normalise` does for the whole file, plus the one-time move of the
    /// old 15-minute idle release to the new default; any other value is the player's
    /// own and stays (D-299).
    fn normalise(&mut self) {
        self.ui.normalise();
        self.drop_view_keys();
        // One profile per name, the last as Save keeps it: two of one name, from a hand
        // edit, took the Settings page and every join window down (row 15, F2).
        let mut seen = std::collections::HashSet::new();
        let mut kept: Vec<LaunchProfile> = self
            .launch_profiles
            .drain(..)
            .rev()
            .filter(|p| seen.insert(p.name.clone()))
            .collect();
        kept.reverse();
        self.launch_profiles = kept;
        if !self.idle_default_v2 {
            if self.steam_idle_minutes == 15 {
                self.steam_idle_minutes = 5;
            }
            self.idle_default_v2 = true;
        }
    }

    /// `settings_get` adds these to what it hands the page, and Settings sends the object
    /// back: kept as unknown keys, they were written into the file (D-303).
    fn drop_view_keys(&mut self) {
        for key in ["unreadable", "reset", "keptAs"] {
            self.extra.remove(key);
        }
    }

    /// Idle timeout for the Steam session, `None` when disabled.
    pub fn steam_idle_timeout(&self) -> Option<std::time::Duration> {
        (self.steam_idle_minutes > 0)
            .then(|| std::time::Duration::from_secs(u64::from(self.steam_idle_minutes) * 60))
    }
}

pub struct SettingsStore {
    path: PathBuf,
    current: Mutex<Settings>,
    /// The file exists but could not be read at start — a sharing violation, a
    /// permission. The defaults in memory are not the user's settings, so nothing may
    /// be written over the file until a read succeeds (D-239).
    unread: AtomicBool,
    /// The file could not be parsed and was kept aside (`set_aside`): the settings in
    /// memory are the defaults standing in for it (row 14, H4).
    reset: AtomicBool,
    /// The name of the copy `set_aside` kept of that file.
    kept_as: Mutex<Option<String>>,
}

/// Whether the settings handed out are the player's own (row 14, H4).
#[derive(Debug, Clone, Default)]
pub struct SettingsHealth {
    /// The file exists but still cannot be read.
    pub unreadable: bool,
    /// The file was damaged and set aside this session.
    pub reset: bool,
    /// The file became readable just now and its settings replaced the defaults.
    pub adopted: bool,
    /// The name of the copy kept of a damaged file.
    pub kept_as: Option<String>,
}

/// What a settings file held that this build could use.
struct Parsed {
    settings: Settings,
    /// Where values were that this build could not take (`steamIdleMinutes`, `ui.news`,
    /// `launchProfiles[1].name`): each is back to its default.
    dropped: Vec<String>,
}

/// The file's text: UTF-8 with or without a byte-order mark, or UTF-16 with one. Notepad
/// and PowerShell 5.1 write all three, and a file edited by hand read as damaged for its
/// mark alone (row 15, H3).
fn decode(bytes: &[u8]) -> Result<String, String> {
    let utf16 = |rest: &[u8], little: bool| -> Result<String, String> {
        if !rest.len().is_multiple_of(2) {
            return Err("the file is UTF-16 with an odd number of bytes".into());
        }
        let units = rest.as_chunks::<2>().0.iter().map(|&p| {
            if little {
                u16::from_le_bytes(p)
            } else {
                u16::from_be_bytes(p)
            }
        });
        char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .map_err(|e| e.to_string())
    };
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(rest.to_vec()).map_err(|e| e.to_string());
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())
}

/// The keys of `map` that `T` takes, one at a time over those already kept; the others
/// are named in `dropped` under `at`.
fn keep_valid<T: serde::de::DeserializeOwned>(
    map: serde_json::Map<String, Value>,
    at: &str,
    dropped: &mut Vec<String>,
) -> serde_json::Map<String, Value> {
    let mut kept = serde_json::Map::new();
    for (key, value) in map {
        let mut trial = kept.clone();
        trial.insert(key.clone(), value);
        if serde_json::from_value::<T>(Value::Object(trial.clone())).is_ok() {
            kept = trial;
        } else {
            dropped.push(format!("{at}{key}"));
        }
    }
    kept
}

/// A settings file is a JSON object; anything else is corruption, whatever serde is
/// willing to make of it (D-187). Within the object, a value this build cannot take —
/// minutes of -1, `"news": "no"`, a profile name of `null`, a type a future version
/// changed — costs that value alone: taken whole, one of them sent the launch profiles,
/// the News choice and Recording back to their defaults (row 15, H3).
fn read_settings(bytes: &[u8]) -> Result<Parsed, String> {
    let text = decode(bytes)?;
    let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let Value::Object(mut file) = value else {
        return Err("the file is not a JSON object".into());
    };
    // The whole file when it can be: every file this app writes.
    if let Ok(settings) = serde_json::from_value::<Settings>(Value::Object(file.clone())) {
        return Ok(Parsed {
            settings,
            dropped: Vec::new(),
        });
    }
    let mut dropped = Vec::new();
    match file.remove("ui") {
        Some(Value::Object(ui)) => {
            let ui = keep_valid::<UiPrefs>(ui, "ui.", &mut dropped);
            file.insert("ui".into(), Value::Object(ui));
        }
        Some(_) => dropped.push("ui".into()),
        None => {}
    }
    match file.remove("launchProfiles") {
        Some(Value::Array(items)) => {
            let mut kept = Vec::with_capacity(items.len());
            for (i, item) in items.into_iter().enumerate() {
                match item {
                    Value::Object(p) => {
                        let at = format!("launchProfiles[{i}].");
                        kept.push(Value::Object(keep_valid::<LaunchProfile>(
                            p,
                            &at,
                            &mut dropped,
                        )));
                    }
                    _ => dropped.push(format!("launchProfiles[{i}]")),
                }
            }
            file.insert("launchProfiles".into(), Value::Array(kept));
        }
        Some(_) => dropped.push("launchProfiles".into()),
        None => {}
    }
    let kept = keep_valid::<Settings>(file, "", &mut dropped);
    let settings = serde_json::from_value(Value::Object(kept)).map_err(|e| e.to_string())?;
    Ok(Parsed { settings, dropped })
}

#[cfg(test)]
fn parse_settings(bytes: &[u8]) -> Result<Settings, String> {
    read_settings(bytes).map(|p| p.settings)
}

/// A copy of the file as it was, stamped so a second bad start cannot overwrite the
/// first. Its file name when it was written.
fn keep_copy(path: &Path, bytes: &[u8]) -> Option<PathBuf> {
    let aside = path.with_extension(format!(
        "json.unreadable-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    ));
    std::fs::write(&aside, bytes).is_ok().then_some(aside)
}

/// Keeps a copy of a file that could not be parsed at all. Returns the copy's file name
/// when it was written, for the page to name (D-303).
fn set_aside(path: &Path, bytes: &[u8], e: &str) -> Option<String> {
    let aside = keep_copy(path, bytes);
    crate::log_error!(
        "settings",
        "{} is unreadable ({e}); starting from defaults, copy kept: {}",
        path.display(),
        aside
            .as_deref()
            .map_or("no".into(), |a| a.display().to_string())
    );
    aside.and_then(|a| a.file_name().map(|n| n.to_string_lossy().into_owned()))
}

/// A file read in part: the values that could not be taken are named in the log and a
/// copy of the file as it was is kept; everything else stands (row 15, H3).
fn note_partial(path: &Path, bytes: &[u8], dropped: &[String]) {
    let aside = keep_copy(path, bytes);
    crate::log_warn!(
        "settings",
        "{} has values this version cannot take ({}); they are back to their defaults, the rest was kept; copy kept: {}",
        path.display(),
        dropped.join(", "),
        aside
            .as_deref()
            .map_or("no".into(), |a| a.display().to_string())
    );
}

impl SettingsStore {
    pub fn load(path: &Path) -> Self {
        // Defaults are the right answer for a first run, but silently defaulting on an
        // unreadable file meant the next preference change wrote them over the user's
        // launch profiles for good (D-162). Keep a copy and say so.
        let mut unread = false;
        let mut reset = false;
        let mut kept_as = None;
        let mut current = match std::fs::read(path) {
            Ok(bytes) => match read_settings(&bytes) {
                Ok(p) => {
                    if !p.dropped.is_empty() {
                        note_partial(path, &bytes, &p.dropped);
                    }
                    p.settings
                }
                Err(e) => {
                    kept_as = set_aside(path, &bytes, &e);
                    reset = true;
                    Settings::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
            // D-162 covered the file that reads but does not parse. A file that does not
            // read at all — held open by a scanner, a permission — kept no copy, and the
            // front end's first preference patch, a second after start, wrote the
            // defaults over it (D-239).
            Err(e) => {
                crate::log_error!(
                    "settings",
                    "{} could not be read: {e}; it will not be written until it can be",
                    path.display()
                );
                unread = true;
                Settings::default()
            }
        };
        current.normalise();
        let store = Self {
            path: path.to_path_buf(),
            current: Mutex::new(current),
            unread: AtomicBool::new(unread),
            reset: AtomicBool::new(reset),
            kept_as: Mutex::new(kept_as),
        };
        store.apply_install_choices();
        store
    }

    /// One-shot instructions the installer left behind (D-206).
    ///
    /// The setup wizard offers "Do not show the DayZ news page", and `/NONEWS` does the
    /// same for a silent install. It cannot write `settings.json` itself without risking
    /// the rest of the file on an upgrade, so it leaves a registry value and the
    /// launcher applies it on the next start — then deletes it, so it stays an
    /// instruction rather than becoming a second source of truth the user cannot see.
    fn apply_install_choices(&self) {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
        use winreg::RegKey;

        let Ok(key) = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(INSTALL_CHOICES_KEY, KEY_READ | KEY_SET_VALUE)
        else {
            return;
        };
        if key.get_value::<u32, _>("DisableNews").unwrap_or(0) != 1 {
            return;
        }
        // Persist first, delete second. Deleting first meant a failed write — a full
        // disk, a locked file — threw the instruction away and the news page came back
        // for good; leaving the value in place retries it on the next start (D-209).
        let snapshot = {
            let Ok(mut cur) = self.current.lock() else {
                return;
            };
            if self.settle(&mut cur).is_err() {
                return; // still unreadable: the value stays for the next start
            }
            cur.ui.news.then(|| {
                cur.ui.news = false;
                cur.clone()
            })
        };
        if let Some(snapshot) = snapshot {
            if let Err(e) = self.persist(&snapshot) {
                crate::log_warn!("settings", "installer choice not saved, will retry: {e}");
                return;
            }
            crate::log_info!("settings", "news page switched off by the installer");
        }
        let _ = key.delete_value("DisableNews");
    }

    /// Before the first write after a failed read: reads the file again and adopts it
    /// when it reads now, and refuses the write when it still does not (D-239).
    /// Returns whether the settings in memory were just replaced by the file's.
    fn settle(&self, cur: &mut Settings) -> std::io::Result<bool> {
        if !self.unread.load(Ordering::Acquire) {
            return Ok(false);
        }
        let adopted = match std::fs::read(&self.path) {
            Ok(bytes) => match read_settings(&bytes) {
                Ok(p) => {
                    if !p.dropped.is_empty() {
                        note_partial(&self.path, &bytes, &p.dropped);
                    }
                    let mut s = p.settings;
                    s.normalise();
                    // Adopted means applied: start-up applied the defaults the failed
                    // read left, and nothing else re-read the flags (D-245).
                    crate::log::set_enabled(s.logging);
                    crate::log::set_muted(s.log_muted.clone());
                    *cur = s;
                    true
                }
                // Corrupt after all: what `load` would have done with it.
                Err(e) => {
                    let kept = set_aside(&self.path, &bytes, &e);
                    if let Ok(mut k) = self.kept_as.lock() {
                        *k = kept;
                    }
                    self.reset.store(true, Ordering::Release);
                    false
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => {
                crate::log_warn!(
                    "settings",
                    "{} is still unreadable ({e}); not writing over it",
                    self.path.display()
                );
                return Err(e);
            }
        };
        self.unread.store(false, Ordering::Release);
        crate::log_info!("settings", "{} readable again", self.path.display());
        Ok(adopted)
    }

    /// The settings for the page, and whether they are the player's own. A file that
    /// could not be read is tried again first, as every write does: `get` alone never
    /// re-read it, so the page went on showing the defaults as the player's settings —
    /// the News page back on, logging back on, no launch profiles — with nothing on
    /// screen to say so (row 14, H4).
    pub fn get_checked(&self) -> (Settings, SettingsHealth) {
        let mut cur = self.current.lock().unwrap_or_else(|e| e.into_inner());
        let adopted = self.settle(&mut cur).unwrap_or(false);
        let health = SettingsHealth {
            unreadable: self.unread.load(Ordering::Acquire),
            reset: self.reset.load(Ordering::Acquire),
            adopted,
            kept_as: self.kept_as.lock().ok().and_then(|k| k.clone()),
        };
        (cur.clone(), health)
    }

    pub fn get(&self) -> Settings {
        self.current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Replaces the launch options; the UI preferences on disk are kept, whatever the
    /// caller sent (the Settings view round-trips a possibly stale copy).
    pub fn set_launch(&self, mut s: Settings) -> std::io::Result<()> {
        let mut cur = self.current.lock().unwrap_or_else(|e| e.into_inner());
        // The caller's copy was built from the defaults the failed read left in memory;
        // saving it would replace the launch profiles the file has just shown it holds.
        if self.settle(&mut cur)? {
            return Err(std::io::Error::other(
                "the settings file could not be read at start and has just been read; reopen Settings to see it",
            ));
        }
        s.ui = cur.ui.clone();
        s.drop_view_keys();
        // A value saved from Settings is the player's own: the move off the old default
        // must not run over it at the next start (D-299).
        s.idle_default_v2 = true;
        self.persist(&s)?;
        *cur = s;
        Ok(())
    }

    /// Merges a partial UI-preferences object (camelCase keys) into the stored ones,
    /// validates, persists and returns the result.
    pub fn patch_ui(&self, patch: Value) -> std::io::Result<UiPrefs> {
        let Value::Object(patch) = patch else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "ui patch must be an object",
            ));
        };
        let mut cur = self.current.lock().unwrap_or_else(|e| e.into_inner());
        self.settle(&mut cur)?;
        let mut merged = serde_json::to_value(&cur.ui).map_err(invalid)?;
        if let Value::Object(base) = &mut merged {
            for (k, v) in patch {
                base.insert(k, v);
            }
        }
        let mut ui: UiPrefs = serde_json::from_value(merged).map_err(invalid)?;
        ui.normalise();
        if ui != cur.ui {
            let mut next = cur.clone();
            next.ui = ui.clone();
            self.persist(&next)?;
            *cur = next;
        }
        Ok(ui)
    }

    fn persist(&self, s: &Settings) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(s).map_err(invalid)?;
        // Write beside the file and rename over it (D-159). A plain write truncates
        // first, so a crash mid-write left a short file that `load` silently replaced
        // with the defaults — losing the launch profiles, theme, accent and filters.
        // The settings are written on every preference change, so the window is real.
        let tmp = self.path.with_extension("json.tmp");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&json)?;
            f.sync_all()?;
        }
        // A scanner, a backup or a sync client with the file open for a moment makes the
        // replace fail with "Access is denied" (row 14, H9, reproduced): a short retry
        // rides that out instead of losing the change.
        let mut tries = 0;
        loop {
            match std::fs::rename(&tmp, &self.path) {
                Ok(()) => return Ok(()),
                Err(_) if tries < 10 => {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp);
                    crate::log_error!("settings", "could not replace {}: {e}", self.path.display());
                    return Err(e);
                }
            }
        }
    }
}

fn invalid(e: serde_json::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e)
}

#[cfg(test)]
mod shape_tests {
    use super::*;

    /// serde deserialises a struct from a JSON sequence, so `[]` parsed as a valid
    /// `Settings` and yielded silent defaults — past the very safety net that exists
    /// to catch a corrupt file (D-187).
    #[test]
    fn a_settings_file_must_be_an_object() {
        let dir = std::env::temp_dir().join(format!(
            "dayz-settings-shape-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");

        // A real file survives a round trip, profiles and all.
        let good = Settings {
            profile_name: "Survivor".into(),
            launch_profiles: vec![LaunchProfile {
                name: "night".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_vec(&good).unwrap()).unwrap();
        let read = SettingsStore::load(&path).get();
        assert_eq!(read.profile_name, "Survivor");
        assert_eq!(read.launch_profiles.len(), 1);

        // Each of these is corruption, and each must fall back to defaults *and*
        // leave a copy behind rather than passing silently.
        for bad in ["[]", r#"{"ui": []}"#, r#"["profileName", "x"]"#] {
            std::fs::write(&path, bad).unwrap();
            let s = SettingsStore::load(&path).get();
            assert_eq!(
                s.profile_name, "",
                "{bad} should have fallen back to defaults"
            );
            let kept = std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(Result::ok)
                .any(|e| e.file_name().to_string_lossy().contains("unreadable"));
            assert!(kept, "{bad} should have been copied aside");
            for e in std::fs::read_dir(&dir).unwrap().filter_map(Result::ok) {
                if e.file_name().to_string_lossy().contains("unreadable") {
                    std::fs::remove_file(e.path()).ok();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dzl-settings-{tag}-{}.json", std::process::id()))
    }

    #[test]
    fn roundtrip_and_defaults() {
        let path = temp_path("rt");
        let store = SettingsStore::load(&path);
        assert!(store.get().skip_intro && store.get().no_splash && !store.get().no_pause);
        // A loaded store is always normalised, so it carries the accent marker (D-132).
        let mut fresh = UiPrefs::default();
        fresh.normalise();
        assert_eq!(store.get().ui, fresh);
        assert_eq!(store.get().ui.accent, "lime");
        let mut s = store.get();
        s.profile_name = "Survivor".into();
        s.extra_args = "-cpuCount=8".into();
        store.set_launch(s).unwrap();
        let again = SettingsStore::load(&path);
        assert_eq!(again.get().profile_name, "Survivor");
        assert_eq!(again.get().extra_args, "-cpuCount=8");
        let _ = std::fs::remove_file(&path);
    }

    /// A file that exists but cannot be read is never written over with the defaults:
    /// the first patch is refused while it stays unreadable, and adopts the file the
    /// moment it reads (D-239). A directory in its place fails the read with an error
    /// other than NotFound on every platform.
    #[test]
    fn an_unreadable_file_is_not_overwritten() {
        let path = temp_path("unread");
        std::fs::create_dir_all(&path).unwrap();
        let store = SettingsStore::load(&path);
        assert!(
            store.patch_ui(json!({ "theme": "light" })).is_err(),
            "refused while unreadable"
        );
        assert!(path.is_dir(), "nothing was written in its place");
        std::fs::remove_dir(&path).unwrap();
        std::fs::write(
            &path,
            r#"{"profileName":"Survivor","ui":{"accent":"teal"}}"#,
        )
        .unwrap();
        let ui = store.patch_ui(json!({ "theme": "light" })).unwrap();
        assert_eq!(
            (ui.theme.as_str(), ui.accent.as_str()),
            ("light", "teal"),
            "the file's prefs, patched"
        );
        let on_disk = SettingsStore::load(&path).get();
        assert_eq!(
            on_disk.profile_name, "Survivor",
            "the launch options survived"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unknown_fields_fall_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"profileName":"x"}"#).unwrap();
        assert_eq!(s.profile_name, "x");
        assert!(s.skip_intro);
        assert_eq!(s.ui.accent, "lime");
    }

    #[test]
    fn old_amber_default_moves_to_lime_once() {
        // An install from before D-132: amber and no marker.
        let mut ui = UiPrefs {
            accent: "amber".into(),
            ..UiPrefs::default()
        };
        ui.normalise();
        assert_eq!(ui.accent, "lime");
        assert!(ui.accent_default_v2);
        // Amber picked deliberately afterwards survives every later load.
        ui.accent = "amber".into();
        ui.normalise();
        assert_eq!(ui.accent, "amber");
        // Any other accent is untouched by the migration.
        let mut teal = UiPrefs {
            accent: "teal".into(),
            ..UiPrefs::default()
        };
        teal.normalise();
        assert_eq!(teal.accent, "teal");
    }

    #[test]
    fn old_idle_default_moves_to_five_once() {
        // A new install starts at 5 (D-299).
        assert_eq!(Settings::default().steam_idle_minutes, 5);
        // A file from before D-299: the old default and no marker.
        let old: Settings =
            parse_settings(br#"{"steamIdleMinutes": 15, "logging": true}"#).unwrap();
        let mut s = old.clone();
        s.normalise();
        assert_eq!(s.steam_idle_minutes, 5);
        assert!(s.idle_default_v2);
        // 15 chosen afterwards survives every later load.
        s.steam_idle_minutes = 15;
        s.normalise();
        assert_eq!(s.steam_idle_minutes, 15);
        // Any other value, 0 ("stay connected") included, is the player's own.
        for own in [0, 10, 30] {
            let mut t: Settings =
                parse_settings(format!(r#"{{"steamIdleMinutes": {own}}}"#).as_bytes()).unwrap();
            t.normalise();
            assert_eq!(t.steam_idle_minutes, own);
        }
    }

    #[test]
    fn a_saved_idle_value_is_marked_as_the_players_own() {
        let dir = std::env::temp_dir().join(format!("dzl-idle-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        let _ = std::fs::remove_file(&path);
        let store = SettingsStore::load(&path);
        // Settings sends back an object built before the marker existed.
        let mut sent = store.get();
        sent.steam_idle_minutes = 15;
        sent.idle_default_v2 = false;
        store.set_launch(sent).unwrap();
        let again = SettingsStore::load(&path);
        assert_eq!(again.get().steam_idle_minutes, 15);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn launch_update_keeps_ui_prefs() {
        let path = temp_path("keep");
        let store = SettingsStore::load(&path);
        store
            .patch_ui(json!({ "accent": "green", "onboarded": true }))
            .unwrap();
        // A caller holding an old copy of the prefs must not clobber them.
        let stale = Settings {
            no_pause: true,
            ..Settings::default()
        };
        store.set_launch(stale).unwrap();
        let s = SettingsStore::load(&path).get();
        assert!(s.no_pause);
        assert_eq!(s.ui.accent, "green");
        assert!(s.ui.onboarded);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ui_patch_merges_validates_and_persists() {
        let path = temp_path("patch");
        let store = SettingsStore::load(&path);
        let ui = store
            .patch_ui(json!({ "theme": "light", "filters": { "map": "enoch" } }))
            .unwrap();
        assert_eq!(ui.theme, "light");
        assert_eq!(ui.filters, Some(json!({ "map": "enoch" })));
        // A second partial patch keeps the earlier keys; a bad accent falls back.
        let ui = store
            .patch_ui(json!({ "accent": "neon", "lastUpdateCheckMs": 42 }))
            .unwrap();
        assert_eq!(ui.theme, "light");
        assert_eq!(ui.accent, "lime");
        assert_eq!(ui.last_update_check_ms, 42);
        assert!(store.patch_ui(json!("nope")).is_err());
        let again = SettingsStore::load(&path).get().ui;
        assert_eq!(again, ui);
        let _ = std::fs::remove_file(&path);
    }

    /// The page is told when the settings it gets are the defaults standing in for the
    /// player's: a file that cannot be read, then reads (adopted), and a damaged one set
    /// aside (row 14, H4).
    #[test]
    fn the_page_is_told_when_the_settings_are_not_the_players() {
        let path = temp_path("health");
        let _ = std::fs::remove_file(&path);
        std::fs::create_dir_all(&path).unwrap();
        let store = SettingsStore::load(&path);
        let (_, h) = store.get_checked();
        assert!(h.unreadable && !h.reset && !h.adopted);
        std::fs::remove_dir(&path).unwrap();
        std::fs::write(
            &path,
            br#"{"profileName":"Survivor","steamIdleMinutes":30}"#,
        )
        .unwrap();
        let (s, h) = store.get_checked();
        assert!(
            !h.unreadable && !h.reset && h.adopted,
            "read at last: {h:?}"
        );
        assert_eq!(
            (s.profile_name.as_str(), s.steam_idle_minutes),
            ("Survivor", 30)
        );
        let (_, h) = store.get_checked();
        assert!(!h.adopted, "adopted once");
        let _ = std::fs::remove_file(&path);

        let damaged = temp_path("damaged");
        std::fs::write(&damaged, b"{ not json").unwrap();
        let store = SettingsStore::load(&damaged);
        let (s, h) = store.get_checked();
        assert!(h.reset && !h.unreadable);
        assert_eq!(s.profile_name, "");
        let _ = std::fs::remove_file(&damaged);
        if let Some(dir) = damaged.parent() {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with(&format!(
                    "dzl-settings-damaged-{}.json.unreadable-",
                    std::process::id()
                )) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }

    /// A byte-order mark or UTF-16 is still the file, and a value this build cannot take
    /// costs that value alone (row 15, H3).
    #[test]
    fn a_file_is_read_in_part_rather_than_reset() {
        let body = r#"{"profileName":"Survivor","steamIdleMinutes":-1,"logging":false,
            "ui":{"news":"no","accent":"teal"},
            "launchProfiles":[{"name":null,"extraArgs":"-x"},{"name":"night"},7]}"#;
        let p = read_settings(body.as_bytes()).unwrap();
        let s = &p.settings;
        assert_eq!(s.profile_name, "Survivor");
        assert!(!s.logging, "a good value beside a bad one stays");
        assert_eq!(s.steam_idle_minutes, 5, "-1 minutes is back to the default");
        assert!(s.ui.news, "\"no\" is not a switch");
        assert_eq!(s.ui.accent, "teal");
        assert_eq!(s.launch_profiles.len(), 2);
        assert_eq!(
            (
                s.launch_profiles[0].name.as_str(),
                s.launch_profiles[0].extra_args.as_str()
            ),
            ("", "-x")
        );
        assert_eq!(s.launch_profiles[1].name, "night");
        for at in [
            "steamIdleMinutes",
            "ui.news",
            "launchProfiles[0].name",
            "launchProfiles[2]",
        ] {
            assert!(
                p.dropped.iter().any(|d| d == at),
                "{at} named: {:?}",
                p.dropped
            );
        }

        let plain = br#"{"profileName":"Survivor"}"#;
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice(plain);
        let mut utf16 = vec![0xFF, 0xFE];
        for u in std::str::from_utf8(plain).unwrap().encode_utf16() {
            utf16.extend_from_slice(&u.to_le_bytes());
        }
        for bytes in [bom, utf16] {
            let p = read_settings(&bytes).unwrap();
            assert_eq!(p.settings.profile_name, "Survivor");
            assert!(p.dropped.is_empty());
        }
    }

    /// Keys a newer version wrote survive this one's writes, the page's own additions
    /// to `settings_get` do not reach the file, and a name holds one profile (row 15, H4,
    /// F2).
    #[test]
    fn unknown_keys_survive_and_profiles_are_one_per_name() {
        let path = temp_path("extra");
        std::fs::write(
            &path,
            r#"{"futureKey":1,"ui":{"futureUi":"x"},
                "launchProfiles":[{"name":"Low","futureP":true},{"name":"Low","extraArgs":"-b"}]}"#,
        )
        .unwrap();
        let store = SettingsStore::load(&path);
        let got = store.get();
        assert_eq!(got.launch_profiles.len(), 1, "one profile per name");
        assert_eq!(
            got.launch_profiles[0].extra_args, "-b",
            "the last of the name, as Save keeps"
        );
        // Settings sends back what `settings_get` handed out, view keys included.
        let mut sent = serde_json::to_value(&got).unwrap();
        sent["unreadable"] = json!(false);
        sent["keptAs"] = json!("x");
        store
            .set_launch(serde_json::from_value(sent).unwrap())
            .unwrap();
        store.patch_ui(json!({ "theme": "light" })).unwrap();
        let file: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(file["futureKey"], json!(1));
        assert_eq!(file["ui"]["futureUi"], json!("x"));
        assert_eq!(file["ui"]["theme"], json!("light"));
        assert!(file.get("unreadable").is_none() && file.get("keptAs").is_none());
        let _ = std::fs::remove_file(&path);
    }

    /// A file from before 0.1.78 has no version in it and reads as empty, which the
    /// front end takes as an update to this one; the version it saves is kept (D-301).
    #[test]
    fn the_last_seen_version_starts_empty_and_is_kept() {
        let old: Settings = parse_settings(br#"{"ui":{"onboarded":true}}"#).unwrap();
        assert_eq!(old.ui.last_seen_version, "");
        let path = temp_path("seen");
        let store = SettingsStore::load(&path);
        let ui = store
            .patch_ui(json!({ "lastSeenVersion": "0.1.78" }))
            .unwrap();
        assert_eq!(ui.last_seen_version, "0.1.78");
        assert_eq!(
            SettingsStore::load(&path).get().ui.last_seen_version,
            "0.1.78"
        );
        let _ = std::fs::remove_file(&path);
    }
}
