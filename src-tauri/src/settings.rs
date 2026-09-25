//! Settings persisted as JSON in the app config directory (docs/05 §2): launch
//! options plus the UI preferences (theme, accent, filters, onboarding, last update
//! check). The UI ones used to live only in the WebView's localStorage, which can be
//! reset or detached from the app; the file is the source of truth and localStorage
//! is an instant-start cache (D-070).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Where the installer leaves its one-shot choices: `${MANUPRODUCTKEY}` in the NSIS
/// template, which is `HKCUSoftware<manufacturer><product name>` (D-206).
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
    pub ui: UiPrefs,
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
            steam_idle_minutes: 15,
            logging: true,
            log_muted: Vec::new(),
            ui: UiPrefs::default(),
        }
    }
}

impl Settings {
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
}

/// A settings file is a JSON object. Anything else is corruption, whatever serde is
/// willing to make of it.
fn parse_settings(bytes: &[u8]) -> Result<Settings, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if !value.is_object() {
        return Err("the file is not a JSON object".into());
    }
    if let Some(ui) = value.get("ui") {
        if !ui.is_object() {
            return Err("\"ui\" is not a JSON object".into());
        }
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// Keeps a copy of a file that could not be parsed, stamped so a second bad start
/// cannot overwrite the first copy.
fn set_aside(path: &Path, bytes: &[u8], e: &str) {
    let aside = path.with_extension(format!(
        "json.unreadable-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    ));
    let saved = std::fs::write(&aside, bytes).is_ok();
    crate::log_error!(
        "settings",
        "{} is unreadable ({e}); starting from defaults, copy kept: {saved} ({})",
        path.display(),
        aside.display()
    );
}

impl SettingsStore {
    pub fn load(path: &Path) -> Self {
        // Defaults are the right answer for a first run, but silently defaulting on an
        // unreadable file meant the next preference change wrote them over the user's
        // launch profiles for good (D-160). Keep a copy and say so.
        let mut unread = false;
        let mut current = match std::fs::read(path) {
            Ok(bytes) => match parse_settings(&bytes) {
                Ok(s) => s,
                Err(e) => {
                    set_aside(path, &bytes, &e);
                    Settings::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
            // D-160 covered the file that reads but does not parse. A file that does not
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
        current.ui.normalise();
        let store = Self {
            path: path.to_path_buf(),
            current: Mutex::new(current),
            unread: AtomicBool::new(unread),
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
            Ok(bytes) => match parse_settings(&bytes) {
                Ok(mut s) => {
                    s.ui.normalise();
                    // Adopted means applied: start-up applied the defaults the failed
                    // read left, and nothing else re-read the flags (D-245).
                    crate::log::set_enabled(s.logging);
                    crate::log::set_muted(s.log_muted.clone());
                    *cur = s;
                    true
                }
                // Corrupt after all: what `load` would have done with it.
                Err(e) => {
                    set_aside(&self.path, &bytes, &e);
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
        match std::fs::rename(&tmp, &self.path) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                crate::log_error!("settings", "could not replace {}: {e}", self.path.display());
                Err(e)
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
}
