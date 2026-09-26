//! One-shot inventory of Steam, libraries, the DayZ install, Workshop items and
//! `!Workshop` junctions. Serialised to the join plan and the Mods page; the Diagnostics view it was written for went in D-168.

use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use crate::error::AppResult;

use super::{locate, registry, version, workshop, DAYZ_APP_ID};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamInfo {
    pub path: Option<String>,
    pub exe: Option<String>,
    pub source: &'static str,
    pub running: bool,
    pub pid: u32,
    pub registry_pid: u32,
    pub active_user: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryInfo {
    pub path: String,
    pub label: String,
    pub app_count: usize,
    pub has_dayz: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameInfo {
    pub library: String,
    pub folder: String,
    pub exe: String,
    pub exe_version: Option<String>,
    pub game_version: Option<String>,
    pub build_id: String,
    pub last_updated: u64,
    pub size_on_disk: u64,
    pub state_flags: u32,
    pub has_battleye_exe: bool,
    pub has_official_launcher: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopItemInfo {
    pub id: u64,
    pub size: u64,
    pub time_updated: u64,
    pub folder: Option<String>,
    pub meta_name: Option<String>,
    pub mod_name: Option<String>,
    pub needs_update: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopInfo {
    pub acf_path: String,
    pub needs_update: bool,
    pub needs_download: bool,
    pub last_build_id: String,
    pub size_on_disk: u64,
    pub items: Vec<WorkshopItemInfo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunctionInfo {
    pub name: String,
    pub target: Option<String>,
    pub workshop_id: Option<u64>,
    pub target_exists: bool,
    /// What the clean-up would remove: target certainly gone, and a Workshop item (D-276).
    pub removable: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub steam: SteamInfo,
    pub libraries: Vec<LibraryInfo>,
    pub dayz: Option<GameInfo>,
    pub workshop: Option<WorkshopInfo>,
    pub junctions: Vec<JunctionInfo>,
    pub warnings: Vec<String>,
    pub timing_ms: u64,
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

pub fn collect() -> AppResult<Diagnostics> {
    let t0 = Instant::now();
    let mut warnings = Vec::new();

    let steam = registry::detect();
    if steam.path.is_none() {
        warnings.push("Steam is not installed (no registry key).".into());
    } else if !steam.running {
        warnings.push("Steam is not running; the server list and Workshop need it.".into());
    } else if steam.active_user == 0 {
        warnings.push("Steam is running but nobody is logged in.".into());
    }

    // A truncated `libraryfolders.vdf` or `appworkshop_221100.acf` — a power cut
    // mid-write is enough — used to propagate with `?` and fail the whole collect,
    // which `join_plan` turns into a dead join dialog for *every* server, vanilla ones
    // included. None of these files is needed to connect; each becomes a warning and
    // the parts that did parse are still reported (D-194).
    let libs = match &steam.path {
        Some(p) => match locate::libraries(p) {
            Ok(l) => l,
            Err(e) => {
                warnings.push(format!("Steam's library list could not be read ({e})."));
                Vec::new()
            }
        },
        None => Vec::new(),
    };

    let game = match locate::find_dayz(&libs) {
        Ok(g) => g,
        Err(e) => {
            warnings.push(format!("DayZ's install could not be read ({e})."));
            None
        }
    };
    let mut dayz = None;
    let mut workshop = None;
    let mut junctions = Vec::new();

    if let Some(g) = &game {
        let exe = g.exe();
        let ver = version::read(&exe);
        if !exe.is_file() {
            warnings.push(format!("{} is missing.", s(&exe)));
        }
        let has_be = g.battleye_exe().is_file();
        if !has_be {
            warnings
                .push("DayZ_BE.exe is missing; BattlEye servers will reject the client.".into());
        }
        dayz = Some(GameInfo {
            library: s(&g.library.path),
            folder: s(&g.folder),
            exe: s(&exe),
            exe_version: ver.as_ref().map(|v| v.product.clone()),
            game_version: ver.as_ref().map(|v| v.game_string()),
            build_id: g.manifest.build_id.clone(),
            last_updated: g.manifest.last_updated,
            size_on_disk: g.manifest.size_on_disk,
            state_flags: g.manifest.state_flags,
            has_battleye_exe: has_be,
            has_official_launcher: g.official_launcher().is_file(),
        });

        let workshop_read = match workshop::read(&g.library.path) {
            Ok(Some(w)) => Some(w),
            // No list with no item folders is "nothing installed": an empty section, and
            // the Mods page says so. No section at all is left to mean "could not be
            // read", which the page reports with the warning that says why (D-256). But
            // deleting the list is ordinary Workshop troubleshooting and leaves the
            // folders, which the join and the launch already fall back to (D-256,
            // D-265): the page said "no Workshop mods installed" and the badge cleared
            // over mods that were there (D-276).
            Ok(None) => {
                let ids = workshop::installed_ids(&g.library.path);
                if !ids.is_empty() {
                    warnings.push(format!(
                        "Steam's Workshop list is missing; {} mod(s) are listed from their folders, without sizes or update times.",
                        ids.len()
                    ));
                }
                Some(workshop::from_folders(&g.library.path, &ids))
            }
            Err(e) => {
                warnings.push(format!(
                    "Steam's Workshop list could not be read ({e}); installed mods cannot be checked."
                ));
                None
            }
        };
        if let Some(ws) = workshop_read {
            let missing = ws.items.iter().filter(|i| i.folder.is_none()).count();
            if missing > 0 {
                warnings.push(format!(
                    "{missing} Workshop item(s) are listed as installed but have no folder."
                ));
            }
            for i in ws
                .items
                .iter()
                .filter(|i| matches!(i.meta_published_id, Some(p) if p != i.id))
            {
                warnings.push(format!(
                    "Workshop folder {} contains meta.cpp for publishedid {} (mismatch).",
                    i.id,
                    i.meta_published_id.unwrap_or(0)
                ));
            }
            workshop = Some(WorkshopInfo {
                acf_path: s(&ws.acf_path),
                needs_update: ws.needs_update,
                needs_download: ws.needs_download,
                last_build_id: ws.last_build_id.clone(),
                size_on_disk: ws.size_on_disk,
                items: ws
                    .items
                    .iter()
                    .map(|i| WorkshopItemInfo {
                        id: i.id,
                        size: i.size,
                        time_updated: i.time_updated,
                        folder: i.folder.as_deref().map(s),
                        meta_name: i.meta_name.clone(),
                        mod_name: i.mod_name.clone(),
                        needs_update: i.needs_update(),
                    })
                    .collect(),
            });
        }

        junctions = workshop::junctions(&g.workshop_dir())
            .into_iter()
            .map(|j| JunctionInfo {
                name: j.name,
                target: j.target.as_deref().map(s),
                workshop_id: j.workshop_id,
                target_exists: j.target_exists,
                removable: j.removable,
            })
            .collect();
        let dangling = junctions.iter().filter(|j| j.removable).count();
        if dangling > 0 {
            warnings.push(format!(
                "{dangling} junction(s) in !Workshop point at missing folders."
            ));
        }
    } else if steam.path.is_some() {
        // A library on a disconnected drive is skipped by `find_dayz`, and "not
        // installed" is then untrue (D-160). The Mods page shows this line (D-256).
        warnings.push(match locate::unreachable_dayz_libraries(&libs).first() {
            Some(p) => format!(
                "Steam has DayZ in {}, but that folder is not reachable; connect the drive and try again.",
                p.display()
            ),
            None => format!("DayZ (app {DAYZ_APP_ID}) is not installed in any Steam library."),
        });
    }

    Ok(Diagnostics {
        steam: SteamInfo {
            path: steam.path.as_deref().map(s),
            exe: steam.exe.as_deref().map(s),
            source: steam.source,
            running: steam.running,
            pid: steam.pid,
            registry_pid: steam.registry_pid,
            active_user: steam.active_user,
        },
        libraries: libs
            .iter()
            .map(|l| LibraryInfo {
                path: s(&l.path),
                label: l.label.clone(),
                app_count: l.apps.len(),
                has_dayz: l.has(DAYZ_APP_ID),
            })
            .collect(),
        dayz,
        workshop,
        junctions,
        warnings,
        timing_ms: t0.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs against the real machine. `cargo test -- --ignored --nocapture live_diagnostics`
    /// prints the JSON so it can be compared with docs/02-dayz-launch-mechanics.md.
    #[test]
    #[ignore = "reads the live Steam install"]
    fn live_diagnostics() {
        let d = collect().expect("collect");
        println!("{}", serde_json::to_string_pretty(&d).unwrap());
        assert!(
            d.timing_ms < 2_000,
            "diagnostics must stay fast: {} ms",
            d.timing_ms
        );
    }
}
