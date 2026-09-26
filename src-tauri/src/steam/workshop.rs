//! Installed Workshop items and the `!Workshop` junction folder (docs/02 §3–4).
//!
//! * `<lib>\steamapps\workshop\appworkshop_221100.acf`:
//!   `WorkshopItemsInstalled { id { size timeupdated manifest } }` and
//!   `WorkshopItemDetails { id { timeupdated latest_timeupdated … } }`;
//!   `latest_timeupdated > timeupdated` means Steam knows a newer version.
//! * Item folder `<lib>\steamapps\workshop\content\221100\<id>` with `meta.cpp` / `mod.cpp`.
//! * `<game>\!Workshop\@<meta.cpp name>` NTFS junctions → item folders (official launcher).

use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use crate::error::{AppError, AppResult};

use super::cpp::CppValues;
use super::registry::strip_verbatim;
use super::{vdf, DAYZ_APP_ID};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkshopItem {
    pub id: u64,
    pub size: u64,
    pub time_updated: u64,
    pub latest_time_updated: Option<u64>,
    pub manifest: String,
    /// Filled by [`enrich`]: folder on disk (if present), the two names and the
    /// `publishedid` that `meta.cpp` claims (should equal `id`).
    pub folder: Option<PathBuf>,
    pub meta_name: Option<String>,
    pub meta_published_id: Option<u64>,
    pub mod_name: Option<String>,
}

impl WorkshopItem {
    pub fn needs_update(&self) -> bool {
        matches!(self.latest_time_updated, Some(latest) if latest > self.time_updated)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workshop {
    pub acf_path: PathBuf,
    pub needs_update: bool,
    pub needs_download: bool,
    pub last_build_id: String,
    pub size_on_disk: u64,
    pub items: Vec<WorkshopItem>,
}

pub fn acf_path(library: &Path) -> PathBuf {
    library
        .join("steamapps")
        .join("workshop")
        .join(format!("appworkshop_{DAYZ_APP_ID}.acf"))
}

pub fn content_dir(library: &Path) -> PathBuf {
    library
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(DAYZ_APP_ID.to_string())
}

/// Reads and enriches the workshop manifest of one library. `Ok(None)` when the
/// library has no DayZ workshop manifest at all.
pub fn read(library: &Path) -> AppResult<Option<Workshop>> {
    let path = acf_path(library);
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    let mut ws = parse_appworkshop(&text, &path)?;
    enrich(&mut ws, &content_dir(library));
    Ok(Some(ws))
}

/// Workshop ids with a content folder in this library, for when the manifest is missing.
pub fn installed_ids(library: &Path) -> Vec<u64> {
    let Ok(entries) = std::fs::read_dir(content_dir(library)) else {
        return Vec::new();
    };
    let mut ids: Vec<u64> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.parse().ok()))
        .collect();
    ids.sort_unstable();
    ids
}

/// The named items as their content folders show them, without the manifest: what a
/// launch falls back to when `appworkshop_221100.acf` will not parse. The manifest
/// adds sizes and update times; the folders are what the junctions point at (D-194,
/// D-239).
pub fn from_folders(library: &Path, ids: &[u64]) -> Workshop {
    let mut ws = Workshop {
        acf_path: acf_path(library),
        needs_update: false,
        needs_download: false,
        last_build_id: String::new(),
        size_on_disk: 0,
        items: ids
            .iter()
            .map(|&id| WorkshopItem {
                id,
                size: 0,
                time_updated: 0,
                latest_time_updated: None,
                manifest: String::new(),
                folder: None,
                meta_name: None,
                meta_published_id: None,
                mod_name: None,
            })
            .collect(),
    };
    enrich(&mut ws, &content_dir(library));
    ws
}

/// Pure parser over the ACF text.
pub fn parse_appworkshop(text: &str, path: &Path) -> AppResult<Workshop> {
    let doc = vdf::parse(text, path)?;
    let root = doc
        .value
        .get_obj()
        .ok_or_else(|| AppError::parse(path, "AppWorkshop is not an object"))?;
    let details = vdf::get_obj(root, "WorkshopItemDetails");
    let mut items = Vec::new();
    if let Some(installed) = vdf::get_obj(root, "WorkshopItemsInstalled") {
        for (id, values) in installed.iter() {
            let Ok(id_num) = id.parse::<u64>() else {
                continue;
            };
            let Some(obj) = values.first().and_then(|v| v.get_obj()) else {
                continue;
            };
            let latest = details
                .and_then(|d| vdf::get_obj(d, id))
                .and_then(|d| vdf::get_u64(d, "latest_timeupdated"));
            items.push(WorkshopItem {
                id: id_num,
                size: vdf::get_u64(obj, "size").unwrap_or(0),
                time_updated: vdf::get_u64(obj, "timeupdated").unwrap_or(0),
                latest_time_updated: latest,
                manifest: vdf::get_str(obj, "manifest").unwrap_or("").to_string(),
                folder: None,
                meta_name: None,
                meta_published_id: None,
                mod_name: None,
            });
        }
    }
    items.sort_by_key(|i| i.id);
    Ok(Workshop {
        acf_path: path.to_path_buf(),
        needs_update: vdf::get_flag(root, "NeedsUpdate"),
        needs_download: vdf::get_flag(root, "NeedsDownload"),
        last_build_id: vdf::get_str(root, "LastBuildID").unwrap_or("").to_string(),
        size_on_disk: vdf::get_u64(root, "SizeOnDisk").unwrap_or(0),
        items,
    })
}

/// Looks each item up on disk and reads its names.
pub fn enrich(ws: &mut Workshop, content: &Path) {
    for item in &mut ws.items {
        let dir = content.join(item.id.to_string());
        if !dir.is_dir() {
            continue;
        }
        if let Some(meta) = CppValues::read(&dir.join("meta.cpp")) {
            item.meta_name = meta.get("name").map(str::to_string);
            item.meta_published_id = meta.get_u64("publishedid");
        }
        item.mod_name =
            CppValues::read(&dir.join("mod.cpp")).and_then(|c| c.get("name").map(str::to_string));
        item.folder = Some(dir);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Junction {
    /// Folder name inside `!Workshop`, e.g. `@CF`, for display.
    pub name: String,
    /// The junction itself, exactly as listed. A removal deletes this path: rebuilt from
    /// the display name, which is lossy for a name stored as unpaired UTF-16, it could
    /// be a different, live junction (D-276).
    pub path: PathBuf,
    pub target: Option<PathBuf>,
    /// Last path component of the target when it is a Workshop ID.
    pub workshop_id: Option<u64>,
    /// The target folder is there.
    pub target_exists: bool,
    /// The target is gone for certain and was a Workshop item's folder: the only kind
    /// the clean-up removes (D-093, D-276).
    pub removable: bool,
}

/// What can be said about a junction's target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetState {
    Present,
    Gone,
    Unknown,
}

fn target_state(target: &Path) -> TargetState {
    match std::fs::metadata(target) {
        Ok(m) if m.is_dir() => TargetState::Present,
        Ok(_) => TargetState::Unknown,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => TargetState::Gone,
        // Access denied, a locked volume, a device not ready: `is_dir` read all of these
        // as "gone", and the clean-up removed junctions to folders that were there (D-276).
        Err(_) => TargetState::Unknown,
    }
}

/// A junction target as a path Windows can open. `junction::get_target` strips only
/// `\??\`, so a target written against a volume GUID came back as the relative
/// `Volume{…}\…`, which resolved against the working directory and always read as
/// gone (D-276). Any other relative target cannot be judged at all.
fn openable_target(raw: &Path) -> Option<PathBuf> {
    let t = strip_verbatim(raw);
    if t.is_absolute() {
        return Some(t);
    }
    let s = t.to_string_lossy();
    (s.starts_with("Volume{") || s.starts_with("GLOBALROOT"))
        .then(|| PathBuf::from(format!(r"\\?\{s}")))
}

/// `…\steamapps\workshop\content\221100\<id>`, where the official launcher and this one
/// point every junction they make. A junction a modder made to a folder of their own is
/// not ours to judge, even when that folder's drive is not mounted (D-276).
fn is_workshop_item_dir(target: &Path) -> bool {
    let tail: Vec<String> = target
        .components()
        .rev()
        .take(5)
        .map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect();
    tail.len() == 5
        && !tail[0].is_empty()
        && tail[0].bytes().all(|b| b.is_ascii_digit())
        && tail[1] == DAYZ_APP_ID.to_string()
        && tail[2] == "content"
        && tail[3] == "workshop"
        && tail[4] == "steamapps"
}

/// Lists junctions in the given `!Workshop` folder. Plain files and folders (such
/// as the `!DO_NOT_CHANGE_FILES_IN_THESE_FOLDERS` marker) are skipped.
pub fn junctions(workshop_dir: &Path) -> Vec<Junction> {
    let Ok(entries) = std::fs::read_dir(workshop_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // `symlink_metadata` does not follow the link, so dangling junctions (target
        // folder deleted) are still seen. `junction::exists` follows and would drop them.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
            continue;
        }
        // Reads the reparse data directly; works whether or not the target exists.
        let raw = match junction::get_target(&path) {
            Ok(t) => t,
            Err(_) => continue, // a symlink or other reparse point, not a junction
        };
        let openable = openable_target(&raw);
        let state = openable
            .as_deref()
            .map_or(TargetState::Unknown, target_state);
        let removable =
            state == TargetState::Gone && openable.as_deref().is_some_and(is_workshop_item_dir);
        let target = openable.unwrap_or_else(|| strip_verbatim(&raw));
        let workshop_id = target
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.parse::<u64>().ok());
        out.push(Junction {
            name: entry.file_name().to_string_lossy().into_owned(),
            path,
            target: Some(target),
            workshop_id,
            target_exists: state == TargetState::Present,
            removable,
        });
    }
    out.sort_by_key(|j| j.name.to_lowercase());
    out
}

/// Outcome of [`remove_dangling`] for one junction: its name, its target and the result.
pub type JunctionRemoval = (String, Option<PathBuf>, Result<(), String>);

/// Removes junctions in `workshop_dir` whose Workshop target folder is certainly gone
/// (D-093, D-276): the reparse point is deleted, then the empty directory that carried
/// it. The inventory is re-read here, and each junction is checked again on the very
/// path about to be deleted, so a target that came back — or one that cannot be read —
/// is left alone, and live junctions are never touched whoever created them.
pub fn remove_dangling(workshop_dir: &Path) -> Vec<JunctionRemoval> {
    junctions(workshop_dir)
        .into_iter()
        .filter(|j| j.removable)
        .filter(|j| {
            junction::get_target(&j.path)
                .ok()
                .and_then(|t| openable_target(&t))
                .is_some_and(|t| target_state(&t) == TargetState::Gone && is_workshop_item_dir(&t))
        })
        .map(|j| {
            let r = junction::delete(&j.path)
                .and_then(|()| std::fs::remove_dir(&j.path))
                .map_err(|e| e.to_string());
            (j.name, j.target, r)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACF: &str = include_str!("../../tests/fixtures/appworkshop_221100.acf");

    /// Real NTFS junctions in a temp folder: one live, one whose target was deleted.
    #[test]
    fn remove_dangling_keeps_live_junctions() {
        let tmp = std::env::temp_dir().join(format!("dzl-dangling-test-{}", std::process::id()));
        let ws = tmp.join("!Workshop");
        let content = tmp.join(r"steamapps\workshop\content\221100");
        let live_target = content.join("1559212036");
        let gone_target = content.join("999");
        // A modder's own folder, not a Workshop item: gone too, and still not ours (D-276).
        let own_target = tmp.join("P").join("@DevMod");
        for d in [&live_target, &gone_target, &own_target, &ws] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(live_target.join("meta.cpp"), "name = \"CF\";").unwrap();
        std::fs::write(ws.join("!DO_NOT_CHANGE_FILES_IN_THESE_FOLDERS"), "").unwrap();
        junction::create(&live_target, ws.join("@CF")).unwrap();
        junction::create(&gone_target, ws.join("@Gone")).unwrap();
        junction::create(&own_target, ws.join("@DevMod")).unwrap();
        std::fs::remove_dir(&gone_target).unwrap();
        std::fs::remove_dir(&own_target).unwrap();

        let listed = junctions(&ws);
        let removable: Vec<&str> = listed
            .iter()
            .filter(|j| j.removable)
            .map(|j| j.name.as_str())
            .collect();
        assert_eq!(removable, ["@Gone"]);
        let dev = listed.iter().find(|j| j.name == "@DevMod").unwrap();
        assert!(!dev.target_exists && !dev.removable, "{dev:?}");

        let removed = remove_dangling(&ws);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].0, "@Gone");
        assert!(removed[0].2.is_ok(), "{:?}", removed[0].2);
        assert!(std::fs::symlink_metadata(ws.join("@Gone")).is_err());

        let after = junctions(&ws);
        assert_eq!(
            after.iter().map(|j| j.name.as_str()).collect::<Vec<_>>(),
            ["@CF", "@DevMod"],
            "the live junction and the one outside the Workshop both stay"
        );
        assert!(after[0].target_exists);
        assert!(
            live_target.join("meta.cpp").is_file(),
            "live target untouched"
        );
        assert!(remove_dangling(&ws).is_empty(), "nothing left to remove");

        for j in ["@CF", "@DevMod"] {
            let _ = junction::delete(ws.join(j));
            let _ = std::fs::remove_dir(ws.join(j));
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn junction_targets_are_read_as_windows_would() {
        // `\??\Volume{…}\…` comes back from the reparse data without its prefix (D-276).
        assert_eq!(
            openable_target(Path::new(
                r"Volume{0b1d9f7e-0000-0000-0000-100000000000}\SteamLibrary\x"
            )),
            Some(PathBuf::from(
                r"\\?\Volume{0b1d9f7e-0000-0000-0000-100000000000}\SteamLibrary\x"
            ))
        );
        assert_eq!(
            openable_target(Path::new(
                r"\\?\G:\SteamLibrary\steamapps\workshop\content\221100\1559212036"
            )),
            Some(PathBuf::from(
                r"G:\SteamLibrary\steamapps\workshop\content\221100\1559212036"
            ))
        );
        assert_eq!(openable_target(Path::new(r"some\relative\dir")), None);
        assert!(is_workshop_item_dir(Path::new(
            r"G:\Lib\SteamApps\Workshop\Content\221100\1559212036"
        )));
        assert!(is_workshop_item_dir(Path::new(
            r"\\?\Volume{0b1d9f7e-0000-0000-0000-100000000000}\Lib\steamapps\workshop\content\221100\42"
        )));
        assert!(!is_workshop_item_dir(Path::new(r"P:\@DevMod")));
        assert!(!is_workshop_item_dir(Path::new(
            r"G:\Lib\steamapps\workshop\content\221100\@CF"
        )));
        assert!(!is_workshop_item_dir(Path::new(
            r"G:\Lib\steamapps\workshop\content\107410\42"
        )));
    }

    #[test]
    fn appworkshop_live_fixture() {
        let ws = parse_appworkshop(ACF, Path::new("appworkshop_221100.acf")).unwrap();
        assert!(!ws.needs_update);
        assert!(!ws.needs_download);
        assert_eq!(ws.last_build_id, "24689949");
        assert_eq!(ws.size_on_disk, 193_223_329);
        assert_eq!(ws.items.len(), 4);
        let cf = ws
            .items
            .iter()
            .find(|i| i.id == 1_559_212_036)
            .expect("CF present");
        assert_eq!(cf.size, 527_656);
        assert_eq!(cf.time_updated, 1_771_519_119);
        assert_eq!(cf.latest_time_updated, Some(1_771_519_119));
        assert_eq!(cf.manifest, "4114705373119672275");
        assert!(!cf.needs_update());
        assert!(
            ws.items.windows(2).all(|w| w[0].id < w[1].id),
            "sorted by id"
        );
    }

    #[test]
    fn needs_update_when_latest_is_newer() {
        let item = WorkshopItem {
            id: 1,
            size: 0,
            time_updated: 10,
            latest_time_updated: Some(11),
            manifest: String::new(),
            folder: None,
            meta_name: None,
            meta_published_id: None,
            mod_name: None,
        };
        assert!(item.needs_update());
    }
}
