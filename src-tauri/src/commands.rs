//! Tauri IPC surface. Every command that touches files, the registry, the network
//! or the database runs off the UI thread (docs/05 §4).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::a2s::{self, Client};
use crate::browser::verify::{self, Target, Verdict, Verification};
use crate::browser::{Cache, HistoryEntry, PopulationSample, ServerRow};
use crate::error::{AppError, AppResult};
use crate::launch::{self, LaunchSpec, Launched};
use crate::settings::{Settings, SettingsStore, UiPrefs};
use crate::steam::diagnostics::{self, Diagnostics};
use crate::steam::sdk::{ItemDetails, SteamStatus, SteamWorker};
use crate::steam::{locate, registry, version, workshop};

/// Shared application state, created in `lib.rs::run` setup.
pub struct AppState {
    pub steam: SteamWorker,
    pub cache: Arc<Mutex<Cache>>,
    pub a2s: Client,
    pub settings: SettingsStore,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    name: &'static str,
    version: &'static str,
    tauri: &'static str,
    os: &'static str,
}

/// Static build information for the About/Diagnostics views and the M0 smoke test.
#[tauri::command]
pub fn app_info() -> AppInfo {
    #[cfg(debug_assertions)]
    eprintln!("[ipc] app_info called from the webview");
    AppInfo {
        name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        tauri: tauri::VERSION,
        os: std::env::consts::OS,
    }
}

/// Full Steam / DayZ / Workshop inventory (M1).
#[tauri::command]
pub async fn diagnostics() -> AppResult<Diagnostics> {
    let result = tauri::async_runtime::spawn_blocking(diagnostics::collect)
        .await
        .map_err(|e| AppError::Internal(format!("diagnostics task failed: {e}")))?;
    #[cfg(debug_assertions)]
    if let Ok(d) = &result {
        eprintln!(
            "[ipc] diagnostics: steam_running={} libraries={} dayz={} workshop_items={} junctions={} warnings={} in {} ms",
            d.steam.running,
            d.libraries.len(),
            d.dayz.is_some(),
            d.workshop.as_ref().map_or(0, |w| w.items.len()),
            d.junctions.len(),
            d.warnings.len(),
            d.timing_ms
        );
    }
    result
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsExport {
    pub path: String,
    pub bytes: usize,
}

/// Writes a support report (app info, Steam status, settings, full diagnostics)
/// as JSON into the app's local data folder and returns its path.
#[tauri::command]
pub async fn diagnostics_export(app: AppHandle, state: State<'_, AppState>) -> AppResult<DiagnosticsExport> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| AppError::Internal(format!("no local data dir: {e}")))?;
    let steam = state.steam.status();
    let settings = state.settings.get();
    let last_refresh = {
        let c = Arc::clone(&state.cache);
        tauri::async_runtime::spawn_blocking(move || c.lock().ok().and_then(|c| c.get_meta("last_refresh").ok().flatten()))
            .await
            .ok()
            .flatten()
    };
    let diag = tauri::async_runtime::spawn_blocking(diagnostics::collect)
        .await
        .map_err(|e| AppError::Internal(format!("diagnostics task failed: {e}")))??;
    let report = serde_json::json!({
        "app": { "name": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION"), "tauri": tauri::VERSION, "os": std::env::consts::OS },
        "generatedAt": ServerRow::now_unix(),
        "steam": steam,
        "settings": settings,
        "lastRefresh": last_refresh,
        "diagnostics": diag,
    });
    let json = serde_json::to_vec_pretty(&report).map_err(|e| AppError::Internal(format!("json: {e}")))?;
    let path = dir.join(format!("diagnostics-{}.json", ServerRow::now_unix()));
    std::fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
    std::fs::write(&path, &json).map_err(|e| AppError::io(&path, e))?;
    Ok(DiagnosticsExport {
        path: path.to_string_lossy().into_owned(),
        bytes: json.len(),
    })
}

/// Installed DayZ version in A2S form (`1.29.163709`), for the "version = mine" filter.
#[tauri::command]
pub async fn local_game_version() -> AppResult<Option<String>> {
    tauri::async_runtime::spawn_blocking(|| {
        let steam = registry::detect();
        let path = steam.path?;
        let libs = locate::libraries(&path).ok()?;
        let game = locate::find_dayz(&libs).ok().flatten()?;
        version::read(&game.exe()).map(|v| v.game_string())
    })
    .await
    .map_err(|e| AppError::Internal(format!("version task failed: {e}")))
}

/// Steamworks thread status (M3).
#[tauri::command]
pub fn steam_status(state: State<'_, AppState>) -> SteamStatus {
    state.steam.status()
}

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> Settings {
    state.settings.get()
}

/// Replaces the launch options only; UI preferences are written through `ui_prefs_set`.
#[tauri::command]
pub fn settings_set(state: State<'_, AppState>, settings: Settings) -> AppResult<()> {
    state.settings.set_launch(settings).map_err(|e| AppError::Internal(format!("settings: {e}")))
}

/// Merges a partial UI-preferences object (theme, accent, filters, onboarded,
/// lastUpdateCheckMs) into the settings file and returns the stored result (D-070).
#[tauri::command]
pub fn ui_prefs_set(state: State<'_, AppState>, patch: serde_json::Value) -> AppResult<UiPrefs> {
    state.settings.patch_ui(patch).map_err(|e| AppError::Internal(format!("ui prefs: {e}")))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedServers {
    pub rows: Vec<ServerRow>,
    /// Unix seconds of the last completed refresh, if any.
    pub last_refresh: Option<i64>,
}

/// Last known server list from the SQLite cache; renders before Steam answers (M3).
#[tauri::command]
pub async fn servers_cached(state: State<'_, AppState>) -> AppResult<CachedServers> {
    let cache = Arc::clone(&state.cache);
    let out = tauri::async_runtime::spawn_blocking(move || {
        let c = cache.lock().map_err(|_| AppError::Internal("cache lock poisoned".into()))?;
        let rows = c.load_all().map_err(|e| AppError::Internal(format!("cache: {e}")))?;
        let last_refresh = c.get_meta("last_refresh").ok().flatten().and_then(|v| v.parse().ok());
        Ok::<_, AppError>(CachedServers { rows, last_refresh })
    })
    .await
    .map_err(|e| AppError::Internal(format!("cache task failed: {e}")))??;
    #[cfg(debug_assertions)]
    eprintln!(
        "[ipc] servers_cached: {} rows, last refresh {:?}, {} ms after start",
        out.rows.len(),
        out.last_refresh,
        crate::uptime_ms()
    );
    Ok(out)
}

/// Starts a Steamworks internet-server-list refresh; rows stream back as
/// `servers:batch` events, completion as `servers:done` (M3). `full` adds the
/// empty-server partitions (minutes); explicit `partitions` override both.
/// Returns `false` when skipped (already refreshing, or too soon and not forced).
#[tauri::command]
pub fn servers_refresh(
    state: State<'_, AppState>,
    partitions: Option<Vec<HashMap<String, String>>>,
    full: Option<bool>,
    force: Option<bool>,
) -> AppResult<bool> {
    let parts = match partitions {
        Some(p) if !p.is_empty() => p,
        _ if full.unwrap_or(false) => crate::steam::sdk::full_partitions(),
        _ => Vec::new(),
    };
    state.steam.refresh(parts, force.unwrap_or(false)).map_err(AppError::Internal)
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifySummary {
    pub total: usize,
    pub verified: usize,
    pub inflated: usize,
    pub unverifiable: usize,
    pub synthetic: usize,
    pub offline: usize,
    pub elapsed_ms: u64,
}

/// Verifies targets in chunks: emits `servers:verified` (Vec<Verification>) per
/// chunk and persists. With `announce`, also emits `servers:verify-done`
/// (VerifySummary); only the automatic post-refresh pass announces, so on-demand
/// checks of visible rows never masquerade as the full pass.
pub async fn run_verification(app: AppHandle, cache: Arc<Mutex<Cache>>, client: Client, targets: Vec<Target>, announce: bool) -> VerifySummary {
    let t0 = Instant::now();
    // The automatic pass (announce) relies on Steam's fresh INFO and sends PLAYER only;
    // on-demand checks of visible rows also refresh ping and clock.
    let with_info = !announce;
    let by_id: HashMap<String, Target> = targets.iter().map(|t| (t.id.clone(), t.clone())).collect();
    let mut latest: HashMap<String, Verdict> = HashMap::with_capacity(targets.len());
    let mut offline: Vec<Target> = Vec::new();
    for chunk in targets.chunks(500) {
        let results = verify::verify_many(&client, chunk.to_vec(), with_info).await;
        for v in &results {
            if v.verdict == Verdict::Offline {
                if let Some(t) = by_id.get(&v.id) {
                    offline.push(t.clone());
                }
            }
        }
        publish(&app, &cache, &mut latest, results).await;
    }
    // Second chance for targets that did not answer: a longer timeout, INFO included,
    // so a server that is merely slow or drops PLAYER is not reported as down.
    if !offline.is_empty() {
        let patient = client.clone().with_timeout(std::time::Duration::from_millis(2500));
        let results = verify::verify_many(&patient, offline, true).await;
        publish(&app, &cache, &mut latest, results).await;
    }
    let mut summary = VerifySummary {
        total: targets.len(),
        elapsed_ms: t0.elapsed().as_millis() as u64,
        ..Default::default()
    };
    for verdict in latest.values() {
        match verdict {
            Verdict::Verified => summary.verified += 1,
            Verdict::Inflated => summary.inflated += 1,
            Verdict::Unverifiable => summary.unverifiable += 1,
            Verdict::Synthetic => summary.synthetic += 1,
            Verdict::Offline => summary.offline += 1,
        }
    }
    #[cfg(debug_assertions)]
    eprintln!(
        "[verify] {} done: total={} verified={} inflated={} unverifiable={} synthetic={} offline={} in {} ms",
        if announce { "pass" } else { "on-demand" },
        summary.total,
        summary.verified,
        summary.inflated,
        summary.unverifiable,
        summary.synthetic,
        summary.offline,
        summary.elapsed_ms
    );
    if announce {
        let _ = app.emit("servers:verify-done", &summary);
    }
    summary
}

/// Emits and persists one batch of verification results, recording the latest
/// verdict per server so a retry supersedes an earlier "offline". Verified
/// head-counts also become population samples (M6 sparkline).
async fn publish(app: &AppHandle, cache: &Arc<Mutex<Cache>>, latest: &mut HashMap<String, Verdict>, results: Vec<Verification>) {
    for v in &results {
        latest.insert(v.id.clone(), v.verdict);
    }
    let _ = app.emit("servers:verified", &results);
    let c = Arc::clone(cache);
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Ok(mut c) = c.lock() {
            if let Err(e) = c.apply_verifications(&results) {
                eprintln!("[cache] apply_verifications failed: {e}");
            }
            let samples: Vec<(String, i64, i32, i32)> = results
                .iter()
                .filter(|v| v.verdict == Verdict::Verified)
                .filter_map(|v| {
                    let queue = v.tags.as_ref().and_then(|t| t.queue).unwrap_or(0) as i32;
                    v.verified.map(|n| (v.id.clone(), v.verified_at, n, queue))
                })
                .collect();
            if !samples.is_empty() {
                if let Err(e) = c.population_add(&samples) {
                    eprintln!("[cache] population_add failed: {e}");
                }
            }
        }
    })
    .await;
}

/// On-demand verification of specific servers (rows on screen, favourites).
#[tauri::command]
pub async fn servers_verify(app: AppHandle, state: State<'_, AppState>, ids: Vec<String>) -> AppResult<VerifySummary> {
    let cache = Arc::clone(&state.cache);
    let lookup = Arc::clone(&cache);
    let targets: Vec<Target> = tauri::async_runtime::spawn_blocking(move || {
        let c = lookup.lock().map_err(|_| AppError::Internal("cache lock poisoned".into()))?;
        Ok::<_, AppError>(
            ids.iter()
                .filter_map(|id| c.get(id).ok().flatten())
                .filter_map(|r| Target::from_row(&r))
                .collect(),
        )
    })
    .await
    .map_err(|e| AppError::Internal(format!("cache task failed: {e}")))??;
    Ok(run_verification(app, cache, state.a2s.clone(), targets, false).await)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDetails {
    pub id: String,
    pub info: Option<a2s::Info>,
    pub info_rtt_ms: Option<u32>,
    pub rules: Option<a2s::Rules>,
    pub rules_error: Option<String>,
    pub players: Option<a2s::Players>,
    pub verification: Verification,
}

/// Live INFO + RULES + PLAYER for one server (details pane, join flow).
#[tauri::command]
pub async fn server_details(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<ServerDetails> {
    let addr: SocketAddr = id.parse().map_err(|_| AppError::Internal(format!("bad server id {id}")))?;
    let client = state.a2s.clone();
    let cache = Arc::clone(&state.cache);
    let (reported, max_players) = cached_counts(&cache, &id).await;

    let (info, rules, players) = tokio::join!(client.info(addr), client.rules(addr), client.players(addr));
    let (verdict, verified, reason) = verify::judge(info.as_ref().ok().map(|r| &r.value), players.as_ref().map(|r| &r.value), reported, max_players);
    let (reported, max_players) = info
        .as_ref()
        .ok()
        .map(|r| (r.value.players as i32, r.value.max_players as i32))
        .unwrap_or((reported, max_players));
    let verification = Verification {
        id: id.clone(),
        verdict,
        reported,
        verified,
        max_players,
        ping_ms: info.as_ref().ok().map(|r| r.rtt.as_millis() as u32),
        keywords: info.as_ref().ok().and_then(|r| r.value.keywords.clone()),
        tags: info.as_ref().ok().map(|r| r.value.tags.clone()),
        verified_at: ServerRow::now_unix(),
        reason,
    };
    let persist = vec![verification.clone()];
    let _ = app.emit("servers:verified", &persist);
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Ok(mut c) = cache.lock() {
            let _ = c.apply_verifications(&persist);
        }
    })
    .await;

    Ok(ServerDetails {
        id,
        info_rtt_ms: info.as_ref().ok().map(|r| r.rtt.as_millis() as u32),
        info: info.ok().map(|r| r.value),
        rules_error: rules.as_ref().err().map(|e| e.to_string()),
        rules: rules.ok().map(|r| r.value),
        players: players.ok().map(|r| r.value),
        verification,
    })
}

async fn cached_row(cache: &Arc<Mutex<Cache>>, id: &str) -> Option<ServerRow> {
    let c = Arc::clone(cache);
    let key = id.to_string();
    tauri::async_runtime::spawn_blocking(move || c.lock().ok().and_then(|c| c.get(&key).ok().flatten()))
        .await
        .ok()
        .flatten()
}

async fn cached_counts(cache: &Arc<Mutex<Cache>>, id: &str) -> (i32, i32) {
    cached_row(cache, id).await.map(|r| (r.players, r.max_players)).unwrap_or((0, 0))
}

// ---------------------------------------------------------------------------
// M5: join plan, mod sync, launch
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModPlanItem {
    pub id: u64,
    /// `mod.cpp` name as the server reports it.
    pub name: String,
    /// Workshop title and size from Steam, when the details query succeeded.
    pub title: Option<String>,
    pub size: Option<u64>,
    pub installed: bool,
    pub needs_update: bool,
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinPlan {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub game_port: u16,
    pub password_required: bool,
    pub server_version: String,
    pub local_version: Option<String>,
    pub version_mismatch: bool,
    pub steam_running: bool,
    pub game_found: bool,
    pub battleye_present: bool,
    pub rules_ok: bool,
    pub mods: Vec<ModPlanItem>,
    pub missing: usize,
    pub updates: usize,
    pub download_bytes: u64,
    pub profile_name: String,
    pub warnings: Vec<String>,
}

/// Everything the join dialog needs: required mods (live RULES), local state, Steam
/// titles/sizes for anything missing, and preconditions.
#[tauri::command]
pub async fn join_plan(state: State<'_, AppState>, id: String) -> AppResult<JoinPlan> {
    let row = cached_row(&state.cache, &id)
        .await
        .ok_or_else(|| AppError::Internal(format!("unknown server {id}")))?;
    let addr: SocketAddr = id.parse().map_err(|_| AppError::Internal(format!("bad server id {id}")))?;
    let rules = state.a2s.rules(addr).await;
    let diag = tauri::async_runtime::spawn_blocking(diagnostics::collect)
        .await
        .map_err(|e| AppError::Internal(format!("diagnostics task failed: {e}")))??;

    let mut warnings = Vec::new();
    let required: Vec<(u64, String)> = match &rules {
        Ok(r) => r.value.dayz.as_ref().map(|d| d.mods.iter().map(|m| (m.workshop_id, m.name.clone())).collect()).unwrap_or_default(),
        Err(e) => {
            warnings.push(format!("Could not read the server's mod list ({e}); launching without mods."));
            Vec::new()
        }
    };
    let inventory: HashMap<u64, (Option<String>, bool)> = diag
        .workshop
        .as_ref()
        .map(|w| w.items.iter().map(|i| (i.id, (i.folder.clone(), i.needs_update))).collect())
        .unwrap_or_default();

    let mut mods: Vec<ModPlanItem> = required
        .iter()
        .map(|(id, name)| {
            let (folder, needs_update) = inventory.get(id).cloned().unwrap_or((None, false));
            ModPlanItem {
                id: *id,
                name: name.clone(),
                title: None,
                size: None,
                installed: folder.is_some(),
                needs_update,
                folder,
            }
        })
        .collect();

    // Titles and sizes from Steam for the whole list (one query per 50 ids).
    if !mods.is_empty() {
        let ids: Vec<u64> = mods.iter().map(|m| m.id).collect();
        let worker_status = state.steam.status();
        if worker_status.initialized {
            let cmd = state.steam.clone_handle();
            match tauri::async_runtime::spawn_blocking(move || cmd.item_details(&ids)).await {
                Ok(Ok(details)) => {
                    let by_id: HashMap<u64, ItemDetails> = details.into_iter().map(|d| (d.id, d)).collect();
                    for m in &mut mods {
                        if let Some(d) = by_id.get(&m.id) {
                            m.title = Some(d.title.clone());
                            m.size = Some(d.file_size);
                        }
                    }
                }
                Ok(Err(e)) => warnings.push(format!("Workshop details unavailable: {e}")),
                Err(e) => warnings.push(format!("Workshop details task failed: {e}")),
            }
        }
    }

    let missing = mods.iter().filter(|m| !m.installed).count();
    let updates = mods.iter().filter(|m| m.installed && m.needs_update).count();
    let download_bytes = mods.iter().filter(|m| !m.installed || m.needs_update).filter_map(|m| m.size).sum();
    let local_version = diag.dayz.as_ref().and_then(|g| g.game_version.clone());
    let version_mismatch = local_version.as_deref().is_some_and(|v| v != row.version);
    if version_mismatch {
        warnings.push(format!(
            "Server runs {} but your DayZ is {}; the server will reject the connection until Steam updates the game.",
            row.version,
            local_version.clone().unwrap_or_default()
        ));
    }
    if !diag.steam.running {
        warnings.push("Steam is not running; DayZ cannot start without it.".into());
    }
    let settings = state.settings.get();
    let profile_name = if settings.profile_name.trim().is_empty() {
        worker_persona(&state).unwrap_or_default()
    } else {
        settings.profile_name.clone()
    };

    Ok(JoinPlan {
        id,
        name: row.name,
        ip: row.ip,
        game_port: row.game_port,
        password_required: row.password,
        server_version: row.version,
        local_version,
        version_mismatch,
        steam_running: diag.steam.running,
        game_found: diag.dayz.is_some(),
        battleye_present: diag.dayz.as_ref().is_some_and(|g| g.has_battleye_exe),
        rules_ok: rules.is_ok(),
        missing,
        updates,
        download_bytes,
        mods,
        profile_name,
        warnings,
    })
}

fn worker_persona(state: &State<'_, AppState>) -> Option<String> {
    state.steam.status().persona
}

/// Subscribe + download the given Workshop items; progress via `mods:progress`,
/// completion via `mods:done` (both carry `job`).
#[tauri::command]
pub fn mods_sync(state: State<'_, AppState>, job: u64, ids: Vec<u64>) -> AppResult<()> {
    state.steam.sync(job, ids).map_err(AppError::Internal)
}

/// Builds junctions and the argument line, then starts `DayZ_BE.exe`. Emits
/// `launch:started` (Launched) now and `launch:exited` ({pid, code}) later.
#[tauri::command]
pub async fn launch_game(app: AppHandle, state: State<'_, AppState>, id: String, password: Option<String>) -> AppResult<Launched> {
    let row = cached_row(&state.cache, &id)
        .await
        .ok_or_else(|| AppError::Internal(format!("unknown server {id}")))?;
    let addr: SocketAddr = id.parse().map_err(|_| AppError::Internal(format!("bad server id {id}")))?;
    let rules = state.a2s.rules(addr).await;
    let required: Vec<(u64, String)> = match &rules {
        Ok(r) => r.value.dayz.as_ref().map(|d| d.mods.iter().map(|m| (m.workshop_id, m.name.clone())).collect()).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let settings = state.settings.get();
    let persona = worker_persona(&state);

    let row_for_spec = row.clone();
    let (game_dir, links, spec) = tauri::async_runtime::spawn_blocking(move || {
        let row = row_for_spec;
        let steam = registry::detect();
        if !steam.running {
            return Err(AppError::Internal("Steam is not running".into()));
        }
        let path = steam.path.ok_or_else(|| AppError::Internal("Steam is not installed".into()))?;
        let libs = locate::libraries(&path)?;
        let game = locate::find_dayz(&libs)?.ok_or_else(|| AppError::Internal("DayZ is not installed".into()))?;
        let ws = workshop::read(&game.library.path)?;
        let by_id: HashMap<u64, (PathBuf, Option<String>)> = ws
            .map(|w| w.items.into_iter().filter_map(|i| i.folder.map(|f| (i.id, (f, i.meta_name)))).collect())
            .unwrap_or_default();
        let mut items = Vec::with_capacity(required.len());
        for (id, name) in &required {
            let (folder, meta) = by_id
                .get(id)
                .cloned()
                .ok_or_else(|| AppError::Internal(format!("mod {name} ({id}) is not installed; sync mods first")))?;
            items.push((*id, folder, meta));
        }
        let links = launch::ensure_junctions(&game.folder, &items)?;
        let spec = LaunchSpec {
            mod_paths: links.iter().map(|l| l.junction.clone()).collect(),
            ip: row.ip.clone(),
            game_port: row.game_port,
            password,
            profile_name: Some(if settings.profile_name.trim().is_empty() { persona.unwrap_or_default() } else { settings.profile_name.clone() }),
            skip_intro: settings.skip_intro,
            no_splash: settings.no_splash,
            no_pause: settings.no_pause,
            extra_args: settings.extra_args.clone(),
        };
        Ok::<_, AppError>((game.folder, links, spec))
    })
    .await
    .map_err(|e| AppError::Internal(format!("launch task failed: {e}")))??;

    let args = launch::build_args(&spec);
    let (mut child, launched) = launch::spawn(&game_dir, &args)?;
    {
        let c = Arc::clone(&state.cache);
        let row_for_history = row.clone();
        let mods = links.len();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            if let Ok(c) = c.lock() {
                let _ = c.history_add(&row_for_history, mods);
            }
        })
        .await;
    }
    #[cfg(debug_assertions)]
    eprintln!("[launch] pid {} with {} mod junction(s) ({} created): {}", launched.pid, links.len(), links.iter().filter(|l| l.created).count(), launched.command_line);
    let _ = app.emit("launch:started", &launched);
    let pid = launched.pid;
    tauri::async_runtime::spawn_blocking(move || {
        let code = child.wait().ok().and_then(|s| s.code());
        #[cfg(debug_assertions)]
        eprintln!("[launch] pid {pid} exited with {code:?}");
        let _ = app.emit("launch:exited", &LaunchExited { pid, code });
    });
    Ok(launched)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchExited {
    pub pid: u32,
    pub code: Option<i32>,
}

// ---------------------------------------------------------------------------
// M6: favourites, history, population, direct connect, import
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Favourite {
    pub id: String,
    pub added_at: i64,
}

#[tauri::command]
pub async fn favourites_list(state: State<'_, AppState>) -> AppResult<Vec<Favourite>> {
    let c = Arc::clone(&state.cache);
    tauri::async_runtime::spawn_blocking(move || {
        let c = c.lock().map_err(|_| AppError::Internal("cache lock poisoned".into()))?;
        let list = c.favourites().map_err(|e| AppError::Internal(format!("cache: {e}")))?;
        Ok(list.into_iter().map(|(id, added_at)| Favourite { id, added_at }).collect())
    })
    .await
    .map_err(|e| AppError::Internal(format!("cache task failed: {e}")))?
}

#[tauri::command]
pub async fn favourite_set(state: State<'_, AppState>, id: String, on: bool) -> AppResult<()> {
    let c = Arc::clone(&state.cache);
    tauri::async_runtime::spawn_blocking(move || {
        let c = c.lock().map_err(|_| AppError::Internal("cache lock poisoned".into()))?;
        c.favourite_set(&id, on).map_err(|e| AppError::Internal(format!("cache: {e}")))
    })
    .await
    .map_err(|e| AppError::Internal(format!("cache task failed: {e}")))?
}

#[tauri::command]
pub async fn history_list(state: State<'_, AppState>, limit: Option<usize>) -> AppResult<Vec<HistoryEntry>> {
    let c = Arc::clone(&state.cache);
    let limit = limit.unwrap_or(50).min(500);
    tauri::async_runtime::spawn_blocking(move || {
        let c = c.lock().map_err(|_| AppError::Internal("cache lock poisoned".into()))?;
        c.history(limit).map_err(|e| AppError::Internal(format!("cache: {e}")))
    })
    .await
    .map_err(|e| AppError::Internal(format!("cache task failed: {e}")))?
}

/// Verified head-count samples for one server over the last `hours` (default 72).
#[tauri::command]
pub async fn population_history(state: State<'_, AppState>, id: String, hours: Option<u32>) -> AppResult<Vec<PopulationSample>> {
    let c = Arc::clone(&state.cache);
    let since = ServerRow::now_unix() - hours.unwrap_or(72) as i64 * 3600;
    tauri::async_runtime::spawn_blocking(move || {
        let c = c.lock().map_err(|_| AppError::Internal("cache lock poisoned".into()))?;
        c.population(&id, since).map_err(|e| AppError::Internal(format!("cache: {e}")))
    })
    .await
    .map_err(|e| AppError::Internal(format!("cache task failed: {e}")))?
}

/// Query ports to try when the typed port turned out to be a game port: game+1..+3
/// (most hosts), then Steam's defaults. One host often runs several DayZ servers
/// (51.81.8.81 answers on 27016 *and* 27017, D-053), so callers must match the
/// reply's game port rather than accept the first answer.
fn candidate_query_ports(game_port: u16) -> Vec<u16> {
    let mut v = vec![
        game_port.saturating_add(1),
        game_port.saturating_add(2),
        game_port.saturating_add(3),
        27016,
        27017,
        27018,
        27019,
        27020,
        27015,
    ];
    let mut seen = std::collections::HashSet::new();
    v.retain(|p| *p != game_port && *p != 0 && seen.insert(*p));
    v
}

/// INFO-probes `ports` in order; with `expect_game_port`, only a reply whose EDF
/// game port matches is accepted.
async fn probe_server(client: &Client, ip: std::net::IpAddr, ports: &[u16], expect_game_port: Option<u16>) -> Option<ServerRow> {
    let probe = client.clone().with_timeout(std::time::Duration::from_millis(1500)).with_retries(0);
    for &qport in ports {
        let addr = SocketAddr::new(ip, qport);
        if let Ok(reply) = probe.info(addr).await {
            if reply.value.app_id != 0 && reply.value.app_id != crate::steam::DAYZ_APP_ID {
                continue; // something else answered on that port
            }
            if let Some(gp) = expect_game_port {
                if reply.value.game_port != Some(gp) {
                    continue; // a sibling server on the same host
                }
            }
            return Some(ServerRow::from_info(&ip.to_string(), qport, &reply.value, reply.rtt.as_millis() as u32));
        }
    }
    None
}

/// Adds a server by `host:port` (game or query port, hostname allowed): probes
/// A2S_INFO on likely query ports, stores the row, emits it as a batch, verifies it.
#[tauri::command]
pub async fn direct_connect(app: AppHandle, state: State<'_, AppState>, address: String) -> AppResult<ServerRow> {
    let text = address.trim().trim_start_matches("steam://connect/");
    let (host, port) = match text.rsplit_once(':') {
        Some((h, p)) => (h.trim_matches(|c| c == '[' || c == ']'), p.parse::<u16>().map_err(|_| AppError::Internal(format!("bad port in {address}")))?),
        None => (text, 2302),
    };
    if host.is_empty() {
        return Err(AppError::Internal("enter an address like 51.81.8.81:2402".into()));
    }
    let ip: std::net::IpAddr = match host.parse() {
        Ok(ip) => ip,
        Err(_) => tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| AppError::Internal(format!("cannot resolve {host}: {e}")))?
            .map(|a| a.ip())
            .find(|ip| ip.is_ipv4())
            .ok_or_else(|| AppError::Internal(format!("{host} has no IPv4 address")))?,
    };
    // The typed port may be the query port (answers INFO directly) or the game port
    // (then find the sibling query port whose reply advertises exactly that game port).
    let row = match probe_server(&state.a2s, ip, &[port], None).await {
        Some(r) => r,
        None => {
            let ports = candidate_query_ports(port);
            probe_server(&state.a2s, ip, &ports, Some(port))
                .await
                .ok_or_else(|| AppError::Internal(format!("no DayZ server answered at {ip}:{port}; also tried query ports {ports:?} for that game port")))?
        }
    };
    let stored = vec![row.clone()];
    let c = Arc::clone(&state.cache);
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Ok(mut c) = c.lock() {
            let _ = c.upsert(&stored);
        }
    })
    .await;
    let _ = app.emit("servers:batch", &vec![row.clone()]);
    if let Some(t) = Target::from_row(&row) {
        tauri::async_runtime::spawn(run_verification(app.clone(), Arc::clone(&state.cache), state.a2s.clone(), vec![t], false));
    }
    Ok(row)
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub total: usize,
    pub imported: usize,
    pub already: usize,
    /// Added from the XML data only because the server did not answer A2S now.
    pub unreachable: usize,
    pub path: String,
}

/// Imports the official launcher's favourites (docs/02 §7): probes each server,
/// stores a row (live or from the XML), and marks it favourite.
#[tauri::command]
pub async fn import_official_favourites(app: AppHandle, state: State<'_, AppState>) -> AppResult<ImportResult> {
    let path = crate::steam::official::favourites_path().ok_or_else(|| AppError::Internal("LOCALAPPDATA is not set".into()))?;
    let entries = crate::steam::official::read_favourites(&path).map_err(|e| AppError::io(&path, e))?;
    let mut result = ImportResult {
        total: entries.len(),
        path: path.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let existing: std::collections::HashSet<String> = {
        let c = Arc::clone(&state.cache);
        tauri::async_runtime::spawn_blocking(move || c.lock().ok().and_then(|c| c.favourites().ok()).unwrap_or_default().into_iter().map(|(id, _)| id).collect())
            .await
            .unwrap_or_default()
    };
    let mut new_rows = Vec::new();
    let mut targets = Vec::new();
    for e in entries {
        let id = ServerRow::id_for(&e.query_ip, e.query_port);
        if existing.contains(&id) {
            result.already += 1;
            continue;
        }
        let ip: std::net::IpAddr = match e.query_ip.parse() {
            Ok(ip) => ip,
            Err(_) => continue,
        };
        let row = match probe_server(&state.a2s, ip, &[e.query_port], None).await {
            Some(r) => r,
            None => {
                result.unreachable += 1;
                ServerRow {
                    id: id.clone(),
                    ip: e.query_ip.clone(),
                    game_port: e.game_port,
                    query_port: e.query_port,
                    name: e.name.clone(),
                    map: e.map.clone(),
                    description: String::new(),
                    players: 0,
                    max_players: e.max_players,
                    bots: 0,
                    password: e.password,
                    secure: true,
                    server_version: e.server_version,
                    version: ServerRow::version_string(e.server_version),
                    ping_ms: 0,
                    tags: crate::a2s::DayzTags::parse(&e.tags),
                    keywords: e.tags.clone(),
                    steam_id: 0,
                    last_seen: ServerRow::now_unix(),
                    verified_players: None,
                    steam_empty: None,
                    verified_at: None,
                    verdict: None,
                }
            }
        };
        if let Some(t) = Target::from_row(&row) {
            targets.push(t);
        }
        new_rows.push(row);
        result.imported += 1;
    }
    if !new_rows.is_empty() {
        let c = Arc::clone(&state.cache);
        let rows = new_rows.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            if let Ok(mut c) = c.lock() {
                let _ = c.upsert(&rows);
                for r in &rows {
                    let _ = c.favourite_set(&r.id, true);
                }
            }
        })
        .await;
        let _ = app.emit("servers:batch", &new_rows);
        tauri::async_runtime::spawn(run_verification(app.clone(), Arc::clone(&state.cache), state.a2s.clone(), targets, false));
    }
    Ok(result)
}
