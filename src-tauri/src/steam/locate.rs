//! Steam libraries and the DayZ install (docs/02 §2).
//!
//! * `<SteamPath>\steamapps\libraryfolders.vdf` lists every library and, per
//!   library, an `apps { appid size }` map: cheap way to know where 221100 lives.
//! * `<lib>\steamapps\appmanifest_221100.acf` → `installdir`, `buildid`, `StateFlags`, …
//! * Game folder = `<lib>\steamapps\common\<installdir>`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

use super::{vdf, DAYZ_APP_ID};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    pub index: u32,
    pub path: PathBuf,
    pub label: String,
    /// appid → size on disk (bytes) as recorded by Steam.
    pub apps: BTreeMap<u32, u64>,
}

impl Library {
    pub fn has(&self, app_id: u32) -> bool {
        self.apps.contains_key(&app_id)
    }
    pub fn steamapps(&self) -> PathBuf {
        self.path.join("steamapps")
    }
}

pub fn libraries(steam_path: &Path) -> AppResult<Vec<Library>> {
    let file = steam_path.join("steamapps").join("libraryfolders.vdf");
    let text = std::fs::read_to_string(&file).map_err(|e| AppError::io(&file, e))?;
    parse_libraryfolders(&text, &file)
}

/// Pure parser; `path` is only used for error messages.
pub fn parse_libraryfolders(text: &str, path: &Path) -> AppResult<Vec<Library>> {
    let doc = vdf::parse(text, path)?;
    let root = doc
        .value
        .get_obj()
        .ok_or_else(|| AppError::parse(path, "root is not an object"))?;
    let mut libs = Vec::new();
    for (key, values) in root.iter() {
        let Ok(index) = key.parse::<u32>() else {
            continue;
        };
        let Some(obj) = values.first().and_then(|v| v.get_obj()) else {
            continue;
        };
        let Some(p) = vdf::get_str(obj, "path") else {
            continue;
        };
        let mut apps = BTreeMap::new();
        if let Some(a) = vdf::get_obj(obj, "apps") {
            for (id, sz) in a.iter() {
                if let (Ok(id), Some(sz)) =
                    (id.parse::<u32>(), sz.first().and_then(|v| v.get_str()))
                {
                    apps.insert(id, sz.parse().unwrap_or(0));
                }
            }
        }
        libs.push(Library {
            index,
            path: vdf::path_value(p),
            label: vdf::get_str(obj, "label").unwrap_or("").to_string(),
            apps,
        });
    }
    libs.sort_by_key(|l| l.index);
    Ok(libs)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppManifest {
    pub app_id: u32,
    pub name: String,
    pub install_dir: String,
    pub build_id: String,
    pub state_flags: u32,
    pub last_updated: u64,
    pub size_on_disk: u64,
}

pub fn parse_appmanifest(text: &str, path: &Path) -> AppResult<AppManifest> {
    let doc = vdf::parse(text, path)?;
    let root = doc
        .value
        .get_obj()
        .ok_or_else(|| AppError::parse(path, "AppState is not an object"))?;
    let install_dir = vdf::get_str(root, "installdir")
        .ok_or_else(|| AppError::parse(path, "missing installdir"))?
        .to_string();
    Ok(AppManifest {
        app_id: vdf::get_u32(root, "appid").unwrap_or(0),
        name: vdf::get_str(root, "name").unwrap_or("").to_string(),
        install_dir,
        build_id: vdf::get_str(root, "buildid").unwrap_or("").to_string(),
        state_flags: vdf::get_u32(root, "StateFlags").unwrap_or(0),
        last_updated: vdf::get_u64(root, "LastUpdated").unwrap_or(0),
        size_on_disk: vdf::get_u64(root, "SizeOnDisk").unwrap_or(0),
    })
}

#[derive(Debug, Clone)]
pub struct GameInstall {
    pub library: Library,
    pub manifest: AppManifest,
    pub folder: PathBuf,
}

impl GameInstall {
    pub fn exe(&self) -> PathBuf {
        self.folder.join("DayZ_x64.exe")
    }
    pub fn battleye_exe(&self) -> PathBuf {
        self.folder.join("DayZ_BE.exe")
    }
    pub fn official_launcher(&self) -> PathBuf {
        self.folder.join("DayZLauncher.exe")
    }
    pub fn workshop_dir(&self) -> PathBuf {
        self.folder.join("!Workshop")
    }
}

/// Finds DayZ by reading `appmanifest_221100.acf` in each library. Libraries whose
/// `apps` map claims 221100 are checked first, then all others.
/// Library folders Steam still lists as holding DayZ whose drive is not mounted
/// right now. `find_dayz` skips them, and the resulting "DayZ is not installed" is
/// simply untrue when the game sits on a disconnected drive (D-160).
pub fn unreachable_dayz_libraries(libs: &[Library]) -> Vec<PathBuf> {
    libs.iter()
        .filter(|l| l.has(DAYZ_APP_ID) && !l.steamapps().is_dir())
        .map(|l| l.path.clone())
        .collect()
}

pub fn find_dayz(libs: &[Library]) -> AppResult<Option<GameInstall>> {
    let ordered = libs
        .iter()
        .filter(|l| l.has(DAYZ_APP_ID))
        .chain(libs.iter().filter(|l| !l.has(DAYZ_APP_ID)));
    for lib in ordered {
        let manifest_path = lib
            .steamapps()
            .join(format!("appmanifest_{DAYZ_APP_ID}.acf"));
        if !manifest_path.is_file() {
            continue;
        }
        let text =
            std::fs::read_to_string(&manifest_path).map_err(|e| AppError::io(&manifest_path, e))?;
        let manifest = parse_appmanifest(&text, &manifest_path)?;
        let folder = lib.steamapps().join("common").join(&manifest.install_dir);
        return Ok(Some(GameInstall {
            library: lib.clone(),
            manifest,
            folder,
        }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBS: &str = include_str!("../../tests/fixtures/libraryfolders.vdf");
    const MANIFEST: &str = include_str!("../../tests/fixtures/appmanifest_221100.acf");

    #[test]
    fn libraryfolders_live_fixture() {
        let libs = parse_libraryfolders(LIBS, Path::new("libraryfolders.vdf")).unwrap();
        assert_eq!(libs.len(), 2);
        assert_eq!(libs[0].index, 0);
        assert_eq!(libs[0].path, PathBuf::from(r"C:\Program Files (x86)\Steam"));
        assert_eq!(libs[0].apps.get(&228980), Some(&417_894_432));
        assert!(!libs[0].has(DAYZ_APP_ID));
        assert_eq!(libs[1].path, PathBuf::from(r"G:\SteamLibrary"));
        assert_eq!(libs[1].apps.get(&DAYZ_APP_ID), Some(&25_563_415_305));
        assert!(libs[1].has(223_350), "dedicated server app is listed too");
    }

    #[test]
    fn appmanifest_live_fixture() {
        let m = parse_appmanifest(MANIFEST, Path::new("appmanifest_221100.acf")).unwrap();
        assert_eq!(m.app_id, DAYZ_APP_ID);
        assert_eq!(m.name, "DayZ");
        assert_eq!(m.install_dir, "DayZ");
        assert_eq!(m.build_id, "24689949");
        assert_eq!(m.state_flags, 4);
        assert_eq!(m.last_updated, 1_786_711_115);
        assert_eq!(m.size_on_disk, 25_563_415_305);
    }
}
