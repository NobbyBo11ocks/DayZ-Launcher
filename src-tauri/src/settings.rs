//! Settings persisted as JSON in the app config directory (docs/05 §2): launch
//! options plus the UI preferences (theme, accent, filters, onboarding, last update
//! check). The UI ones used to live only in the WebView's localStorage, which can be
//! reset or detached from the app; the file is the source of truth and localStorage
//! is an instant-start cache (D-070).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
}

impl SettingsStore {
    pub fn load(path: &Path) -> Self {
        let mut current = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
            .unwrap_or_default();
        current.ui.normalise();
        Self {
            path: path.to_path_buf(),
            current: Mutex::new(current),
        }
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
