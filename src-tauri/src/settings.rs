//! Launch settings persisted as JSON in the app config directory (docs/05 §2).
//! UI-only preferences (theme, filters) stay in the WebView's localStorage.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            profile_name: String::new(),
            extra_args: String::new(),
            skip_intro: true,
            no_splash: true,
            no_pause: false,
        }
    }
}

pub struct SettingsStore {
    path: PathBuf,
    current: Mutex<Settings>,
}

impl SettingsStore {
    pub fn load(path: &Path) -> Self {
        let current = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
            .unwrap_or_default();
        Self {
            path: path.to_path_buf(),
            current: Mutex::new(current),
        }
    }

    pub fn get(&self) -> Settings {
        self.current.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set(&self, s: Settings) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(&s)?;
        std::fs::write(&self.path, json)?;
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = s;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let path = std::env::temp_dir().join(format!("dzl-settings-{}.json", std::process::id()));
        let store = SettingsStore::load(&path);
        assert!(store.get().skip_intro && store.get().no_splash && !store.get().no_pause);
        let mut s = store.get();
        s.profile_name = "Survivor".into();
        s.extra_args = "-cpuCount=8".into();
        store.set(s).unwrap();
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
    }
}
