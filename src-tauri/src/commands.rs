//! Tauri IPC surface. Every command that touches files, the registry, the network
//! or the database runs off the UI thread (docs/05 §4).

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::a2s::{self, Client};
use crate::browser::verify::{self, Target, Verdict, Verification};
use crate::browser::{Cache, HistoryEntry, PopulationSample, ServerMods, ServerRow};
use crate::error::{AppError, AppResult};
use crate::launch::{self, LaunchSpec, Launched};
use crate::settings::{Settings, SettingsStore, UiPrefs};
use crate::steam::diagnostics::{self, Diagnostics};
use crate::steam::sdk::{FriendInfo, ItemDetails, SteamStatus, SteamWorker};
use crate::steam::{locate, registry, version, workshop};

/// Shared application state, created in `lib.rs::run` setup.
pub struct AppState {
    pub steam: SteamWorker,
    pub cache: Arc<Mutex<Cache>>,
    pub a2s: Client,
    pub settings: SettingsStore,
    /// A full verification pass is in flight (D-197).
    pub verifying: Arc<AtomicBool>,
    /// A mod scan is in flight (D-197).
    pub scanning: Arc<AtomicBool>,
    /// A DZSA import is in flight. Two of them write the same ~13 000 rows through the
    /// same mutex, emit two `servers:done` and queue two verification passes for one
    /// list; the front end's own guard does not survive a reload (D-209).
    pub dzsa: Arc<AtomicBool>,
    /// A launch is between its first check and the spawn. Two of them both passed the
    /// running-game check, which came after the mod read, and both started
    /// `DayZ_BE.exe` (D-165's symptom, D-295).
    pub launching: Arc<AtomicBool>,
    /// The page's end of the row stream, once it has subscribed (`send_rows`, D-297).
    pub rows_out: Mutex<Option<Channel<InvokeResponseBody>>>,
    /// What start-up did with the cache (`cache_status`, D-303).
    pub cache_opened: CacheOpened,
}

/// What start-up did with the cache: a session in memory, or a damaged file moved aside
/// at this start (row 14, F14 and H2).
#[derive(Debug, Clone, Default)]
pub struct CacheOpened {
    pub in_memory: bool,
    pub moved_to: Option<PathBuf>,
}

/// The cache as the page should know it (row 14, F14, H2 and H9, approved): running in
/// memory, where a damaged file went at this start, and why writes are failing now.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStatus {
    /// Nothing is kept this session, and nothing saved before is shown.
    pub in_memory: bool,
    /// The file name a damaged cache was moved to at this start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moved_to: Option<String>,
    /// Why the cache's writes fail, until one succeeds again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_failure: Option<String>,
}

/// Cheap enough to ask every minute: no lock on the cache (`note_write`).
#[tauri::command]
pub fn cache_status(state: State<'_, AppState>) -> CacheStatus {
    CacheStatus {
        in_memory: state.cache_opened.in_memory,
        moved_to: state
            .cache_opened
            .moved_to
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned()),
        write_failure: crate::browser::cache::write_failure(),
    }
}

/// Sends one row-lifecycle message to the page: a listing batch, a refresh done, a
/// prune, verification results, a mod scan and its start and end. As events, each was
/// evaluated as a script with its JSON pasted in (tauri 2.11.6, `emit_js_script`), and
/// the renderer kept every compiled script until V8 flushed it — about 0.5 MB per
/// 358-row batch, some 50 MB for minutes after a full Refresh (row 13). A channel
/// message over 8 KiB is fetched and parsed as JSON instead, and the channel keeps them
/// in the order they were sent, which the page relies on: a refresh's done after its
/// last batch, verification after the rows it checks (D-297).
///
/// Nothing is sent before the page subscribes; it was not listening to the events these
/// replace either.
pub fn send_rows<T: Serialize + ?Sized>(app: &AppHandle, kind: &str, data: &T) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Some(channel) = state.rows_out.lock().ok().and_then(|c| c.clone()) else {
        return;
    };
    match serde_json::to_string(data) {
        Ok(json) => {
            let body = format!("{{\"kind\":\"{kind}\",\"data\":{json}}}");
            if let Err(e) = channel.send(InvokeResponseBody::Json(body)) {
                crate::log_warn!("ipc", "{kind} not sent to the page: {e}");
            }
        }
        Err(e) => crate::log_warn!("ipc", "{kind} not serialised: {e}"),
    }
}

/// The page subscribes to the row stream once it has read the cache. A reload
/// subscribes again, and replaces the old end (D-297).
#[tauri::command]
pub fn rows_subscribe(state: State<'_, AppState>, channel: Channel<InvokeResponseBody>) {
    if let Ok(mut out) = state.rows_out.lock() {
        *out = Some(channel);
    }
}

/// Clears its flag however the pass ends, including an early `return`.
pub struct InFlight(Arc<AtomicBool>);

impl InFlight {
    /// `None` when one is already running.
    pub fn claim(flag: &Arc<AtomicBool>) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(Arc::clone(flag)))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    name: &'static str,
    version: &'static str,
    tauri: &'static str,
    os: &'static str,
    /// Running with an administrator token (matched to an elevated Steam, D-119): the
    /// News page then keeps the third-party video frame out of the process (D-248).
    elevated: bool,
    /// Windows' own build, "10.0.26200.6584"; `os` only ever says "windows" (row 25).
    windows: Option<String>,
    /// The WebView2 runtime the page runs in.
    webview: Option<String>,
}

/// Static build information for the Settings page (the Diagnostics view went in D-168),
/// the Logs page's copied report and the M0 smoke test. `async`: it reads the registry and
/// asks WebView2 its version, and a plain command runs on the main thread, where the page
/// asked for it twice while it drew (docs/05 §4, row 27).
#[tauri::command(async)]
pub fn app_info() -> AppInfo {
    #[cfg(debug_assertions)]
    eprintln!("[ipc] app_info called from the webview");
    AppInfo {
        name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        tauri: tauri::VERSION,
        os: std::env::consts::OS,
        elevated: crate::proc::current_is_elevated(),
        windows: windows_build(),
        webview: tauri::webview_version().ok(),
    }
}

/// "10.0.26200.6584" from the registry: what a bug report needs to tell one Windows
/// update from another (row 25).
pub(crate) fn windows_build() -> Option<String> {
    let key = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .ok()?;
    let build: String = key.get_value("CurrentBuild").ok()?;
    let major: u32 = key.get_value("CurrentMajorVersionNumber").unwrap_or(10);
    let minor: u32 = key.get_value("CurrentMinorVersionNumber").unwrap_or(0);
    let ubr: u32 = key.get_value("UBR").unwrap_or(0);
    Some(format!("{major}.{minor}.{build}.{ubr}"))
}

/// Full Steam / DayZ / Workshop inventory (M1).
#[tauri::command]
pub async fn diagnostics() -> AppResult<Diagnostics> {
    let result = tauri::async_runtime::spawn_blocking(diagnostics::collect)
        .await
        .map_err(|e| {
            AppError::logged(
                "steam",
                "The check of Steam and DayZ did not finish",
                format!("diagnostics task failed: {e}"),
            )
        })?;
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
    .map_err(|e| {
        AppError::logged(
            "steam",
            "The installed DayZ version could not be read",
            format!("version task failed: {e}"),
        )
    })
}

/// The saved data (the cache) could not do what was asked: a poisoned lock or a lost
/// worker task. One sentence on screen, the detail in the log (row 16).
fn saved_data(detail: impl std::fmt::Display) -> AppError {
    AppError::logged(
        "cache",
        "The launcher's saved data could not be used",
        detail,
    )
}

/// SQLite refused: a file that cannot be written is said in words ("the disk is full"),
/// anything else as `saved_data` (row 16).
fn saved_data_sql(e: rusqlite::Error) -> AppError {
    match crate::browser::cache::file_reason(&e) {
        Some(why) => {
            crate::log_error!("cache", "{e}");
            AppError::Internal(format!(
                "The launcher's saved data could not be written: {why}."
            ))
        }
        None => saved_data(e),
    }
}

/// Recent's line when the join could not be recorded because the saved data was not there.
const SAVED_DATA_UNREADABLE: &str = "the launcher's saved data could not be opened";

/// Steam sent no names or sizes for the mods; joining is not affected (row 16).
const NO_DETAILS: &str = "Steam did not send the mods' names and sizes; downloading still works.";

/// Direct connect to a name that does not resolve (row 16).
fn not_found(host: &str) -> AppError {
    AppError::Internal(format!(
        "Could not find a server called “{host}”. Check the name, or use its IP address."
    ))
}

/// Steamworks thread status (M3).
#[tauri::command]
pub fn steam_status(state: State<'_, AppState>) -> SteamStatus {
    state.steam.status()
}

/// "Start Steam" (row 16). Nothing while a Steam runs: a second `steam.exe` only hands
/// its arguments to the first. The Steamworks session put `SteamAppId` and `SteamGameId`
/// in this process (steamworks 0.13.1 `init_app`); Steam does not inherit them. The
/// session opens on the worker's next try once Steam is up (D-125).
#[tauri::command(async)]
pub fn steam_start() -> Result<(), String> {
    let steam = registry::detect();
    if steam.running {
        return Ok(());
    }
    // Never with rights the player's own programs lack. The path is HKCU's `SteamExe`,
    // which any program of the player can rewrite, and Steam's folder is writable by
    // every user, so from a launcher running as administrator — it matches an elevated
    // Steam, D-120 — one click could start another program with those rights (row 21).
    // Where Windows' shell runs elevated too (the built-in Administrator account, UAC
    // off), every program already has them and nothing is given away.
    if crate::proc::current_is_elevated() && crate::proc::shell_is_elevated() != Some(true) {
        crate::log_warn!(
            "steam",
            "start: refused, the launcher is elevated and the desktop is not"
        );
        return Err("The launcher runs as administrator and Windows does not, so it will not start Steam with those rights. Start Steam from the Start menu.".into());
    }
    let Some(exe) = steam.exe.filter(|p| p.is_file()) else {
        crate::log_warn!("steam", "start: no steam.exe (registry {})", steam.source);
        return Err(
            "Steam is not installed on this PC, or Windows does not know where it is.".into(),
        );
    };
    let mut cmd = std::process::Command::new(&exe);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    // Normal priority whatever the launcher's own: below normal while a game runs, it
    // would pass that on to Steam for the rest of Steam's life (CreateProcessW, S-127;
    // row 28), as the game's launch already avoids.
    use std::os::windows::process::CommandExt;
    match cmd
        .env_remove("SteamAppId")
        .env_remove("SteamGameId")
        .creation_flags(windows_sys::Win32::System::Threading::NORMAL_PRIORITY_CLASS)
        .spawn()
    {
        Ok(child) => {
            crate::log_info!("steam", "start: {} (pid {})", exe.display(), child.id());
            Ok(())
        }
        Err(e) => {
            crate::log_warn!("steam", "start: {}: {e}", exe.display());
            Err("Steam could not be started. The details are on the Logs page.".into())
        }
    }
}

/// The `-cpuCount`, `-maxMem` and `-maxVRAM` a launch adds for this PC beside `extra`,
/// for Settings to show (row 16). The same call the launch makes (D-267); off the main
/// thread, as the first reading asks DXGI for the video memory.
#[tauri::command(async)]
pub fn perf_args(extra: String) -> Vec<String> {
    crate::hardware::launch_args(&crate::hardware::detect(), &extra)
}

/// The part of a name DayZ receives as written (`launch::args::ansi_exact`), for the join
/// dialog to say when characters are lost on the way (row 15, H1, approved).
#[tauri::command]
pub fn name_as_sent(name: String) -> String {
    crate::launch::args::ansi_exact(name.trim())
        .trim()
        .to_string()
}

/// Off the main thread like its two siblings: it waited there on the lock `persist`
/// holds through its rename retries, and since D-302 may read and copy a file that
/// could not be read at start (row 15, H5).
#[tauri::command(async)]
pub fn settings_get(state: State<'_, AppState>) -> SettingsView {
    let (settings, health) = state.settings.get_checked();
    // Adopted late, the file's idle timeout never reached the Steam thread, which kept
    // the default until a save from Settings (row 14, H4).
    if health.adopted {
        state.steam.set_idle_timeout(settings.steam_idle_timeout());
    }
    SettingsView {
        settings,
        unreadable: health.unreadable,
        reset: health.reset,
        kept_as: health.kept_as,
    }
}

/// The settings, and whether they are the player's own (row 14, H4): the page reads
/// the defaults standing in for an unreadable or damaged file as "not read", and keeps
/// its own copies of the choices it caches (the News switch among them).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    #[serde(flatten)]
    pub settings: Settings,
    /// The file exists but cannot be read; nothing is written over it until it can be.
    pub unreadable: bool,
    /// The file was damaged and kept aside; these are the defaults.
    pub reset: bool,
    /// The name of the copy kept of the damaged file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kept_as: Option<String>,
}

/// Replaces the launch options only; UI preferences are written through `ui_prefs_set`.
/// The Steam idle timeout (D-077) is pushed to the running worker at the same time.
/// `async`: a plain command runs on the main thread, and this one ends in an fsync
/// and a rename (D-239).
#[tauri::command(async)]
pub fn settings_set(state: State<'_, AppState>, settings: Settings) -> AppResult<()> {
    // Applied before the write, so turning logging off cannot be the last thing the
    // log records (D-169).
    crate::log::set_enabled(settings.logging);
    crate::log::set_muted(settings.log_muted.clone());
    let written = state.settings.set_launch(settings);
    // A refused save (the file has just been read again, D-239) means the store now
    // holds the file's settings, not the caller's: the flags applied above and the
    // idle timeout follow the store either way (D-245).
    let current = state.settings.get();
    if written.is_err() {
        crate::log::set_enabled(current.logging);
        crate::log::set_muted(current.log_muted.clone());
    }
    state.steam.set_idle_timeout(current.steam_idle_timeout());
    written.map_err(|e| AppError::Internal(format!("settings: {e}")))
}

/// Merges a partial UI-preferences object (theme, accent, filters, onboarded,
/// lastUpdateCheckMs, news, newsSeen) into the settings file and returns the stored
/// result (D-070).
/// Off the main thread for the same reason as `settings_set` (D-239).
#[tauri::command(async)]
pub fn ui_prefs_set(state: State<'_, AppState>, patch: serde_json::Value) -> AppResult<UiPrefs> {
    let saved = state
        .settings
        .patch_ui(patch)
        .map_err(|e| AppError::Internal(format!("ui prefs: {e}")));
    // A file that could not be read at start is often first read by a preference save
    // (the update check's time), which took it over silently: the Steam thread kept the
    // default idle release for the session, and someone who chose 0 was released after
    // five minutes (row 15, H2). Cheap: the worker only stores the value.
    state
        .steam
        .set_idle_timeout(state.settings.get().steam_idle_timeout());
    saved
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedServers {
    /// The field names of `rows`, once (D-284).
    pub keys: &'static [&'static str],
    pub tag_keys: &'static [&'static str],
    pub rows: crate::browser::model::CompactRows,
    /// Unix seconds of the last completed refresh, if any.
    pub last_refresh: Option<i64>,
}

/// Last known server list from the SQLite cache; renders before Steam answers (M3).
#[tauri::command]
pub async fn servers_cached(state: State<'_, AppState>) -> AppResult<CachedServers> {
    let cache = Arc::clone(&state.cache);
    let out = tauri::async_runtime::spawn_blocking(move || {
        let c = cache
            .lock()
            .map_err(|_| saved_data("cache lock poisoned"))?;
        let rows = c.load_all().map_err(saved_data_sql)?;
        let last_refresh = c
            .get_meta("last_refresh")
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok());
        Ok::<_, AppError>(CachedServers {
            keys: crate::browser::model::ROW_KEYS,
            tag_keys: crate::browser::model::TAG_KEYS,
            rows: crate::browser::model::CompactRows(rows),
            last_refresh,
        })
    })
    .await
    .map_err(|e| saved_data(format!("cache task failed: {e}")))??;
    #[cfg(debug_assertions)]
    eprintln!(
        "[ipc] servers_cached: {} rows, last refresh {:?}, {} ms after start",
        out.rows.0.len(),
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
    state
        .steam
        .refresh(parts, force.unwrap_or(false))
        .map_err(AppError::Internal)
}

/// Loads the DZSA list as a fallback when Steam is unavailable (D-089). Rows stream as
/// `servers:dzsa-batch` (D-271), the mod lists go into the cache, a `servers:done` with source
/// `dzsa` closes the import, and populated rows get the usual verification pass.
/// Returns the number of servers imported.
#[tauri::command]
pub async fn servers_dzsa(app: AppHandle, state: State<'_, AppState>) -> AppResult<usize> {
    let Some(_busy) = InFlight::claim(&state.dzsa) else {
        return Err(AppError::Internal(
            "The DZSA list is already being downloaded.".into(),
        ));
    };
    let t0 = Instant::now();
    let rows = crate::browser::dzsa::fetch()
        .await
        .map_err(AppError::Internal)?;
    let n = rows.len();
    let now = ServerRow::now_unix();
    let cache = Arc::clone(&state.cache);
    let mut targets: Vec<Target> = Vec::new();
    let mut with_mods = 0usize;
    // Steam's own batch size: 500 DZSA rows came to ~229 KB an event, over docs/05 §4's
    // ~200 KB, where Steam's 358 are 207 KB (D-284).
    for chunk in rows.chunks(crate::steam::sdk::BATCH_MAX_ROWS) {
        let batch: Vec<ServerRow> = chunk.iter().map(|r| r.row.clone()).collect();
        for r in &batch {
            if r.players > 0 {
                if let Some(mut t) = Target::from_row(r) {
                    // DZSA's own count, of an age nobody knows: judged against a fresh
                    // INFO, not against itself (D-237).
                    t.reported_at = 0;
                    targets.push(t);
                }
            }
        }
        let mods: Vec<(String, Vec<(u64, String)>)> = chunk
            .iter()
            .filter(|r| !r.mods.is_empty())
            .map(|r| (r.row.id.clone(), r.mods.clone()))
            .collect();
        with_mods += mods.len();
        let c = Arc::clone(&cache);
        let for_db = batch.clone();
        // Logging it was not enough: the command still returned Ok(n), still emitted
        // a `servers:done` claiming n responded, and still wrote `last_refresh` - so
        // on an unwritable cache the user was told thirteen thousand servers had been
        // imported, watched the grid fill from the events, and found none of them on
        // the next start. The comment below has named this bug class since D-186; now
        // the result travels (D-220).
        let wrote = tauri::async_runtime::spawn_blocking(move || {
            if let Ok(mut c) = c.lock() {
                // Both were discarded, so a failed write still returned Ok(n) and
                // the mod rows could be written for servers the upsert never stored
                // — the same class of bug fixed for the favourites import (D-186).
                // In words, and noted for the Servers notice, as every other write
                // the player would miss (row 17).
                let upserted = c.upsert_keeping_measured(&for_db);
                crate::browser::cache::note_write(&upserted);
                if let Err(e) = upserted {
                    crate::log_error!(
                        "cache",
                        "DZSA upsert of {} row(s) failed: {e}",
                        for_db.len()
                    );
                    return Err(format!(
                        "The DZSA list could not be saved: {}.",
                        crate::browser::cache::plain_reason(&e)
                    ));
                }
                let stored = c.replace_server_mods_many(&mods, now);
                crate::browser::cache::note_write(&stored);
                if let Err(e) = stored {
                    crate::log_error!(
                        "cache",
                        "DZSA mod lists for {} row(s) failed: {e}",
                        mods.len()
                    );
                    return Err(format!(
                        "The DZSA list's mod lists could not be saved: {}.",
                        crate::browser::cache::plain_reason(&e)
                    ));
                }
                Ok(())
            } else {
                Err("the server cache is unavailable".to_string())
            }
        })
        .await
        .map_err(|e| saved_data(format!("cache task failed: {e}")))?;
        wrote.map_err(AppError::Internal)?;
        // Its own event: the store keeps measured values over these placeholders, as
        // `upsert_keeping_measured` does, and every other batch is a measurement (D-271).
        send_rows(&app, "dzsa-batch", &batch);
    }
    // `last_refresh` deliberately not written here. It seeds the Steam worker's 60 s
    // throttle across restarts (lib.rs), so a DZSA import used to make the *next*
    // start skip its automatic Steam refresh without saying so - and D-096 already
    // says a non-Steam source must not claim the Steam refresh timestamp (D-220).
    let done = crate::steam::sdk::RefreshDone {
        total: n,
        responded: n,
        failed: 0,
        inflated: 0,
        elapsed_ms: t0.elapsed().as_millis() as u64,
        partitions: Vec::new(),
        capped: false,
        stopped_early: false,
        complete: false,
        rejected: false,
        reason: None,
        source: "dzsa",
    };
    // No `servers:mods-done` here any more: it fed a scan summary line that went in
    // D-250, the store reads the stored lists again itself, and a scan still running
    // lost its "reading mod lists" flag to it (D-281).
    send_rows(&app, "done", &done);
    crate::log_info!(
        "steam",
        "DZSA list imported: {n} server(s), {with_mods} with a mod list, in {} ms; verifying {}",
        done.elapsed_ms,
        targets.len()
    );
    if !targets.is_empty() {
        // Every other announcing pass claims this first (D-197, D-204). This one did
        // not, so a second DZSA import - or a Steam refresh whose pass *is* guarded,
        // started when Steam came up mid-import - ran a second 128-permit pool on the
        // same pacer, doubled the NAT flows to the same addresses (D-037), and emitted
        // two `servers:verify-done`: the first cleared the spinner and posted its
        // summary while the other pass was still writing (D-220).
        let client = state.a2s.clone();
        match InFlight::claim(&state.verifying) {
            Some(guard) => {
                tauri::async_runtime::spawn(async move {
                    let _guard = guard;
                    run_verification(app, cache, client, targets, true).await;
                });
            }
            None => {
                crate::log_info!("verify", "a pass is already running; DZSA pass skipped");
                send_rows(
                    &app,
                    "verify-done",
                    &VerifySummary {
                        skipped: true,
                        ..Default::default()
                    },
                );
            }
        }
    } else {
        // Nothing to verify still has to close the pass (D-162): the UI sets
        // "verifying" as soon as the fallback is asked for, and only this event
        // clears it. An empty or all-empty DZSA list left it spinning for ever.
        send_rows(&app, "verify-done", &VerifySummary::default());
    }
    Ok(n)
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
    /// No pass ran: one was already in flight. The UI has to stop waiting either way,
    /// but an all-zero summary reads as "0 verified · 0 fake · 0 offline", which is the
    /// invented result D-159 removed from the LAN path (D-208).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skipped: bool,
}

/// The verification pass's own permit pool, kept at the interactive client's size —
/// the pass is not being made gentler, it is being taken out of the queue the join
/// dialog and the details pane share (D-193).
pub const VERIFY_CONCURRENCY: usize = 128;
/// Rows per `servers:verified` event. docs/05 §4 caps an event at ~200 KB and a
/// `Verification` measures ~314 B (157 KB for 500, D-188), so 128 rows is ~40 KB —
/// small enough that the front end's derived
/// chain shrugs at it, large enough that a pass is not thousands of events.
const VERIFY_FLUSH_ROWS: usize = 128;
/// …and a partial batch goes out anyway once this long has passed, so the last few
/// stragglers of a pass are not held back by the ones that will never answer.
const VERIFY_FLUSH_EVERY: std::time::Duration = std::time::Duration::from_millis(250);
/// How long after a check the servers still offline at its end are read once more:
/// long enough for a scheduled restart to come back (D-268; on-demand checks too since
/// row 14).
const OFFLINE_REREAD_AFTER: std::time::Duration = std::time::Duration::from_secs(240);
/// The second look a server that answered nothing gets, before it is called offline:
/// a longer wait than the first, INFO included (D-047).
const PATIENT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(2500);

/// Verifies targets as a stream (D-193); only the events are batched: emits
/// `servers:verified` (Vec<Verification>) per chunk and persists. With `announce`, also
/// emits `servers:verify-done` (VerifySummary); only the pass after a Steam refresh or
/// a DZSA import announces, so on-demand checks of visible rows never masquerade as a
/// full pass.
pub async fn run_verification(
    app: AppHandle,
    cache: Arc<Mutex<Cache>>,
    client: Client,
    targets: Vec<Target>,
    announce: bool,
) -> VerifySummary {
    let t0 = Instant::now();
    // The automatic pass (announce) relies on Steam's fresh INFO and sends PLAYER only;
    // on-demand checks of visible rows also refresh ping and clock. A target whose INFO
    // is over a minute old is re-read either way (`verify_one`, D-237).
    let with_info = !announce;
    // A standing R11 verdict survives a restart (D-237). Only the automatic pass needs
    // the lookup: its targets come from Steam's batch rows, which carry no verdict; an
    // on-demand check builds its targets from cached rows that already carry it, and
    // ran this full scan — 9.9 ms at 71 000 rows — on every visible-row change. A
    // failed lookup is logged: silently it re-published "verified" over every
    // standing verdict (D-245).
    let mut targets = targets;
    if announce && !targets.is_empty() {
        let c = Arc::clone(&cache);
        let synthetic = tauri::async_runtime::spawn_blocking(move || {
            c.lock()
                .map_err(|_| "cache lock poisoned".to_string())
                .and_then(|c| c.synthetic_ids().map_err(|e| e.to_string()))
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
        match synthetic {
            Ok(synthetic) => {
                for t in &mut targets {
                    t.was_synthetic |= synthetic.contains(&t.id);
                }
            }
            Err(e) => crate::log_warn!("verify", "standing synthetic verdicts not read: {e}"),
        }
    }
    let total = targets.len();
    let by_id: HashMap<String, Target> =
        targets.iter().map(|t| (t.id.clone(), t.clone())).collect();
    let mut latest: HashMap<String, Verdict> = HashMap::with_capacity(total);
    let mut offline: Vec<Target> = Vec::new();
    // Chunks of 500 meant the first result appeared only when the slowest of 500 had
    // finished — measured at 2.95 s against 0.40 s streamed, and the whole pass 17.99 s
    // against 15.00 s, because each barrier idled the pacer while the tail drained.
    // Streaming needs its own permit pool: these tasks hold permits continuously, and
    // in `state.a2s`'s shared FIFO that starved a row click of its three queries by 85×
    // (D-193).
    let client = if announce {
        client.with_concurrency(VERIFY_CONCURRENCY)
    } else {
        // `servers_verify` runs per visible row and is over in a second; giving each
        // call its own 128-permit pool meant an unbounded number of pools against one
        // pacer, which is what a row click then queues behind (D-197).
        client
    };
    let mut pending = verify::verify_stream(&client, targets, with_info);
    let mut batch: Vec<Verification> = Vec::with_capacity(VERIFY_FLUSH_ROWS);
    let mut last_flush = Instant::now();
    // Standing R11 verdicts this pass could not compare (D-237): they get a second
    // PLAYER read once the minimum gap has passed, below.
    let mut standing: Vec<Target> = Vec::new();
    while let Some(v) = pending.next().await {
        // Held back until the second look below has had its say, and the connection
        // with it: published at once, a network drop hid every row it touched (row 14,
        // F1/H1).
        // An "unverifiable" too (INFO answered, the player list did not). After a full
        // Refresh the pass reads INFO first for every target, whose listing is over a
        // minute old by then (D-237). On 2026-09-28, 335 of 2 577 lost the player list
        // behind Steam's own 36 000 pings, against 6 after a short listing. Published at
        // once, each one never counted before stayed hidden until the next pass (row 18, Q29).
        if matches!(v.verdict, Verdict::Offline | Verdict::Unverifiable) {
            if let Some(t) = by_id.get(&v.id) {
                offline.push(t.clone());
            }
            continue;
        }
        if announce && v.reason == verify::CONTINUITY_STANDING {
            if let Some(t) = by_id.get(&v.id) {
                standing.push(t.clone());
            }
        }
        batch.push(v);
        if batch.len() >= VERIFY_FLUSH_ROWS || last_flush.elapsed() >= VERIFY_FLUSH_EVERY {
            publish(&app, &cache, &mut latest, std::mem::take(&mut batch)).await;
            batch.reserve(VERIFY_FLUSH_ROWS);
            last_flush = Instant::now();
        }
    }
    if !batch.is_empty() {
        publish(&app, &cache, &mut latest, batch).await;
    }
    // Second chance for targets that did not answer: a longer timeout, INFO included,
    // so a server that is merely slow or drops PLAYER is not reported as down.
    if !offline.is_empty() {
        let patient = client.clone().with_timeout(PATIENT_TIMEOUT);
        let results = verify::verify_many(&patient, offline, true).await;
        publish_second_look(&app, &cache, &mut latest, results).await;
    }
    // A standing verdict is only ever overturned by a comparison inside R11's window,
    // and the automatic pass runs once a session — so a server judged synthetic
    // once, hidden by the default filter and never clicked, was re-published
    // "synthetic" at every start for good. The pass finishes now; a detached check
    // reads those servers again a minute past the minimum gap, when the sample this
    // pass stored can be compared (D-245).
    if !standing.is_empty() {
        let n = standing.len();
        let app = app.clone();
        let cache = Arc::clone(&cache);
        let client = client.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs_f32(
                verify::CONTINUITY_MIN_GAP_SECS + 60.0,
            ))
            .await;
            let results = verify::verify_many(&client, standing, false).await;
            let healed = results
                .iter()
                .filter(|v| v.verdict != Verdict::Synthetic)
                .count();
            let mut latest = HashMap::with_capacity(n);
            // Through the second look's gate: published straight, a connection that had
            // dropped by then wrote every standing farm "offline" over "synthetic", and
            // the next start's R11 began them at zero strikes (row 18).
            publish_second_look(&app, &cache, &mut latest, results).await;
            crate::log_info!(
                "verify",
                "{n} standing synthetic verdict(s) compared again: {healed} cleared"
            );
        });
    }
    // "Offline" counts as untrusted, and a hidden row gets no on-demand check, so a
    // server restarting as the pass reached it stayed hidden until the next Refresh:
    // 7 of the 2 210 rows Steam listed as populated on 2026-09-25, one of them counted
    // at 8 a minute before its pass. Those still offline are read once more, INFO
    // included, a few minutes on (D-268).
    // On-demand checks too (row 14, F1/H1): a server that restarted while its row was
    // on screen went offline, hidden, and stayed hidden until the next Refresh. One
    // re-read per server at a time, whichever check asked first.
    let uncounted: Vec<Target> = latest
        .iter()
        .filter(|(_, v)| matches!(**v, Verdict::Offline | Verdict::Unverifiable))
        .filter_map(|(id, _)| by_id.get(id).cloned())
        .collect();
    let at = if announce { "the pass" } else { "a check" };
    schedule_reread(&app, &cache, &client, uncounted, at);
    let mut summary = VerifySummary {
        total,
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
    // Only the full pass: the on-demand check runs every time the visible rows
    // change, which would put a line in the log for every scroll (D-168).
    if announce {
        crate::log_info!(
            "verify",
            "pass: {} checked, {} verified, {} inflated, {} unverifiable, {} synthetic, {} offline in {} ms",
            summary.total,
            summary.verified,
            summary.inflated,
            summary.unverifiable,
            summary.synthetic,
            summary.offline,
            summary.elapsed_ms
        );
        shrink_cache(&cache).await;
        send_rows(&app, "verify-done", &summary);
    }
    summary
}

/// Publishes the results of a second look: the servers that answered at once, the
/// silent ones only while this PC can reach the internet. "Offline" counts as untrusted
/// and hides the row, and a hidden row is checked again only when something else looks
/// at it, so a Wi-Fi drop, a router restart or a wake from sleep emptied the list a
/// screen at a time, and nothing but a full Refresh brought the rows back (row 14,
/// F1/H1). With the connection down the rows keep what they had.
async fn publish_second_look(
    app: &AppHandle,
    cache: &Arc<Mutex<Cache>>,
    latest: &mut HashMap<String, Verdict>,
    results: Vec<Verification>,
) {
    let (answered, silent): (Vec<_>, Vec<_>) = results
        .into_iter()
        .partition(|v| v.verdict != Verdict::Offline);
    // Servers that answered in this same look prove the connection. Asked anyway, the
    // "down" cached by a check during a drop (30 s) left a restarting server unrecorded
    // and put the notice back while the others were answering (row 18).
    let proven = !answered.is_empty();
    if proven {
        publish(app, cache, latest, answered).await;
    }
    if silent.is_empty() {
        return;
    }
    if proven || connection_up().await {
        publish(app, cache, latest, silent).await;
    } else {
        crate::log_warn!(
            "verify",
            "{} server(s) did not answer and Steam's web API is out of reach too: the connection looks down, so none was recorded as offline",
            silent.len()
        );
        set_net(app, false);
    }
}

/// The connection looked down at the last check that could tell (D-303).
static NET_DOWN: AtomicBool = AtomicBool::new(false);

/// Tells the page when the checks find this PC's connection down, and when it is back:
/// the Servers header says so while it lasts, so a list that stops changing is not a
/// mystery (row 14, F1, approved). Sent only on a change.
pub fn set_net(app: &AppHandle, up: bool) {
    if NET_DOWN.swap(!up, Ordering::AcqRel) == !up {
        return;
    }
    if !up {
        crate::log_info!(
            "verify",
            "the connection looks down; the list is kept as it was"
        );
    } else {
        crate::log_info!("verify", "servers answer again");
    }
    send_rows(app, "net", &up);
}

/// Servers with a re-read waiting (`OFFLINE_REREAD_AFTER`), so the visible-row checks
/// every minute do not queue a second one behind the first.
static REREAD_PENDING: std::sync::LazyLock<Mutex<HashSet<String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashSet::new()));

/// Rows a check could not count, silent or without a player list, are read once more
/// a few minutes on with INFO included, one re-read per server at a time (D-268; row 14
/// for on-demand checks; row 18 for "unverifiable" and for the details pane).
fn schedule_reread(
    app: &AppHandle,
    cache: &Arc<Mutex<Cache>>,
    client: &Client,
    targets: Vec<Target>,
    at: &'static str,
) {
    let targets: Vec<Target> = targets
        .into_iter()
        .filter(|t| claim_reread(&t.id))
        .collect();
    if targets.is_empty() {
        return;
    }
    let n = targets.len();
    let app = app.clone();
    let cache = Arc::clone(cache);
    let patient = client.clone().with_timeout(PATIENT_TIMEOUT);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(OFFLINE_REREAD_AFTER).await;
        let ids: Vec<String> = targets.iter().map(|t| t.id.clone()).collect();
        let results = verify::verify_many(&patient, targets, true).await;
        let counted = results
            .iter()
            .filter(|v| !matches!(v.verdict, Verdict::Offline | Verdict::Unverifiable))
            .count();
        let mut latest = HashMap::with_capacity(n);
        publish_second_look(&app, &cache, &mut latest, results).await;
        release_reread(&ids);
        crate::log_info!(
            "verify",
            "{n} server(s) not counted at {at} read again: {counted} counted"
        );
    });
}

/// True when this call gets to re-read `id`; false when one is already waiting.
fn claim_reread(id: &str) -> bool {
    REREAD_PENDING
        .lock()
        .map(|mut p| p.insert(id.to_string()))
        .unwrap_or(false)
}

fn release_reread(ids: &[String]) {
    if let Ok(mut p) = REREAD_PENDING.lock() {
        for id in ids {
            p.remove(id);
        }
    }
}

/// How long one answer from `connection_up` stands, so a burst of checks asks once.
const CONNECTION_SEEN_FOR: std::time::Duration = std::time::Duration::from_secs(30);
static CONNECTION_SEEN: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

/// Whether this PC reaches the internet: a TCP connection to Steam's web API, which the
/// launcher already talks to, and nothing sent over it. Asked only when servers stayed
/// silent through a second look, to tell "they are down" from "we are" (row 14,
/// F1/H1).
async fn connection_up() -> bool {
    if let Ok(seen) = CONNECTION_SEEN.lock() {
        if let Some((at, up)) = *seen {
            if at.elapsed() < CONNECTION_SEEN_FOR {
                return up;
            }
        }
    }
    let up = matches!(
        tokio::time::timeout(
            std::time::Duration::from_secs(4),
            tokio::net::TcpStream::connect(("api.steampowered.com", 443)),
        )
        .await,
        Ok(Ok(_))
    );
    if let Ok(mut seen) = CONNECTION_SEEN.lock() {
        *seen = Some((Instant::now(), up));
    }
    up
}

/// The population samples a batch of checks leaves (M6 sparkline): the head-count of
/// every list that was counted. Verified, and inflated as well, whose count is the real
/// one beside a larger claim: a server that claims more than it has got no chart at all
/// (row 26, approved). Never a claim, a fabricated list or a server that did not answer.
/// An inflated server's queue is its claim's, so it is not kept.
fn population_samples(results: &[Verification]) -> Vec<(String, i64, i32, i32)> {
    results
        .iter()
        .filter_map(|v| {
            let queue = match v.verdict {
                Verdict::Verified => v.tags.as_ref().and_then(|t| t.queue).unwrap_or(0) as i32,
                Verdict::Inflated => 0,
                _ => return None,
            };
            v.verified.map(|n| (v.id.clone(), v.verified_at, n, queue))
        })
        .collect()
}

/// Emits and persists one batch of verification results, recording the latest
/// verdict per server so a retry supersedes an earlier "offline". Counted head-counts
/// also become population samples (`population_samples`).
async fn publish(
    app: &AppHandle,
    cache: &Arc<Mutex<Cache>>,
    latest: &mut HashMap<String, Verdict>,
    results: Vec<Verification>,
) {
    for v in &results {
        latest.insert(v.id.clone(), v.verdict);
    }
    // A server that answered is a connection that works, for `connection_up` as well.
    if results.iter().any(|v| v.verdict != Verdict::Offline) {
        set_net(app, true);
        if let Ok(mut seen) = CONNECTION_SEEN.lock() {
            *seen = Some((Instant::now(), true));
        }
    }
    send_rows(app, "verified", &results);
    let c = Arc::clone(cache);
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Ok(mut c) = c.lock() {
            let applied = c.apply_verifications(&results);
            crate::browser::cache::note_write(&applied);
            if let Err(e) = applied {
                // A refusal of the file itself is `note_write`'s, once an episode (row 25).
                if !crate::browser::cache::is_file_failure(&e) {
                    crate::log_error!(
                        "cache",
                        "apply_verifications of {} failed: {e}",
                        results.len()
                    );
                }
            }
            let samples = population_samples(&results);
            if !samples.is_empty() {
                // Through `note_write` as well, and a refusal of the file itself is its to
                // say, once an episode (row 25).
                let added = c.population_add(&samples);
                crate::browser::cache::note_write(&added);
                if let Err(e) = added {
                    if !crate::browser::cache::is_file_failure(&e) {
                        crate::log_error!(
                            "cache",
                            "population_add of {} sample(s) failed: {e}",
                            samples.len()
                        );
                    }
                }
            }
        }
    })
    .await;
}

/// On-demand verification of specific servers (rows on screen, favourites).
#[tauri::command]
pub async fn servers_verify(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> AppResult<VerifySummary> {
    let cache = Arc::clone(&state.cache);
    let lookup = Arc::clone(&cache);
    let targets: Vec<Target> = tauri::async_runtime::spawn_blocking(move || {
        let c = lookup
            .lock()
            .map_err(|_| saved_data("cache lock poisoned"))?;
        Ok::<_, AppError>(
            ids.iter()
                .filter_map(|id| c.get(id).ok().flatten())
                .filter_map(|r| Target::from_row(&r))
                .collect(),
        )
    })
    .await
    .map_err(|e| saved_data(format!("cache task failed: {e}")))??;
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
    /// The mod list the scan recorded, sent only when the server sent none (`scanned_list`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scanned: Option<ScannedList>,
}

/// A scanned mod list as the details pane shows it, with how old it is in words.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScannedList {
    pub age: String,
    pub mods: Vec<ScannedMod>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScannedMod {
    pub workshop_id: u64,
    pub name: String,
}

/// The scan's list for a server that sent none, as the join plan falls back to it
/// (D-209): the pane said only "The server did not send its mod list" while Join
/// downloaded the scanned mods (row 26, approved). Published mods only, as the join
/// uses them (`Cache::server_mods`, D-239).
fn scanned_list((at, mods): crate::browser::cache::ScannedMods, now: i64) -> ScannedList {
    ScannedList {
        age: humanise_age(now.saturating_sub(at)),
        mods: mods
            .into_iter()
            .map(|(workshop_id, name)| ScannedMod { workshop_id, name })
            .collect(),
    }
}

/// Mod lists are re-scanned when older than this.
const MOD_SCAN_MAX_AGE_SECS: i64 = 24 * 3600;

/// How long the scan waits before its second look at the reads its pace lost (D-329).
const SECOND_LOOK_PAUSE: Duration = Duration::from_secs(30);

/// One server's mod list, or `None` when it did not answer or its reply carried no DayZ
/// data. Each id once, where it first appears (D-265), id 0 included: the cache keeps
/// one row per id, so a list sent with every server-side mod counted more in the Mods
/// column than the same list read back after a restart (D-276). The join plan stores the
/// same shape (D-295). A reply with no DayZ data is no mod list: stored as one, the
/// server read as vanilla for a day and a launch whose own read failed started DayZ
/// without its mods, as the join plan already refuses (row 18).
async fn read_mod_list(client: &Client, addr: SocketAddr) -> Option<Vec<(u64, String)>> {
    client
        .rules(addr)
        .await
        .ok()
        .filter(|r| r.value.dayz.is_some())
        .map(|r| r.value.stored_mods())
}

/// The reads the scan's pace lost, once more: after `pause`, sixteen at a time at 40
/// datagrams a second with a 2 s deadline. The scan runs straight after a start's
/// refresh and check pass — thousands of flows in a minute — and there a large scan lost
/// most of its replies (132 of 175 on 2026-09-29), where the same client read 246 of 246
/// on a calm line. Each loss put the server away for six hours, then twelve, then a day,
/// and its next read fell in another start's burst (D-329). A server that does not answer
/// costs one more try. Returns the lists read and the ids that failed again.
async fn second_look(
    client: &Client,
    targets: Vec<(String, SocketAddr)>,
    pause: Duration,
) -> (Vec<(String, Vec<(u64, String)>)>, Vec<String>) {
    tokio::time::sleep(pause).await;
    let patient = client
        .clone()
        .with_concurrency(16)
        .with_rate(40)
        .with_timeout(Duration::from_secs(2))
        .with_retries(1);
    let mut set = tokio::task::JoinSet::new();
    for (id, addr) in targets {
        let c = patient.clone();
        set.spawn(async move { (id, read_mod_list(&c, addr).await) });
    }
    let (mut read, mut again) = (Vec::new(), Vec::new());
    while let Some(joined) = set.join_next().await {
        let Ok((id, mods)) = joined else { continue };
        match mods {
            Some(m) => read.push((id, m)),
            None => again.push(id),
        }
    }
    (read, again)
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModScanSummary {
    pub total: usize,
    pub scanned: usize,
    pub failed: usize,
    pub elapsed_ms: u64,
}

/// Scans the mod lists (A2S_RULES DayZ payload) of populated, modded servers whose
/// stored list is missing or older than a day (D-080), after the verification pass.
/// One datagram each way at the client's pacing; results stream as `servers:mods`
/// batches and the run ends with `servers:mods-done`.
pub async fn run_mod_scan(
    app: AppHandle,
    cache: Arc<Mutex<Cache>>,
    client: Client,
    scanning: Arc<AtomicBool>,
) -> ModScanSummary {
    // One scan at a time, claimed here rather than at a call site so both callers are
    // covered: the button went through `mods_scan`, which guarded, and the automatic
    // scan after a refresh called this directly, which did not (D-204).
    let Some(_guard) = InFlight::claim(&scanning) else {
        crate::log_info!("mods", "scan already running; ignored");
        return ModScanSummary::default();
    };
    let t0 = Instant::now();
    let now = ServerRow::now_unix();
    let (targets, held_back): (Vec<(String, SocketAddr)>, usize) = {
        let c = Arc::clone(&cache);
        tauri::async_runtime::spawn_blocking(move || {
            // Modded servers with real players, not scanned recently. The fake ones
            // that only claim players in INFO (rule R0, ~20 000 rows) are skipped;
            // scanning them would be a 23 000-flow sweep for nothing (D-037).
            // A failed read used to become an empty target list and a "scanned 0"
            // summary with nothing in the log (D-236).
            c.lock()
                .ok()?
                .scan_targets(now, MOD_SCAN_MAX_AGE_SECS)
                .map_err(|e| crate::log_warn!("cache", "mod scan targets unreadable: {e}"))
                .ok()
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
    };
    let mut summary = ModScanSummary {
        total: targets.len(),
        ..Default::default()
    };
    send_rows(&app, "mods-start", &summary);
    if targets.is_empty() {
        if held_back > 0 {
            crate::log_info!(
                "mods",
                "scan: nothing to read, {held_back} held back after failing"
            );
        }
        send_rows(&app, "mods-done", &summary);
        return summary;
    }
    // Gentler than the verification pass: RULES replies are up to ~5 KB each. Its own
    // permit pool, sized so that concurrency ÷ rate (64 ÷ 100 = 0.64 s) stays inside
    // the 1 s deadline — sharing `state.a2s`'s 128 permits put 1.28 s of queue in front
    // of the challenge leg and made the retry do 99 % of the work (D-193).
    let client = client.with_concurrency(64).with_rate(100).with_retries(1);
    let mut second_looks = 0usize;
    for chunk in targets.chunks(200) {
        let mut set = tokio::task::JoinSet::new();
        for (id, addr) in chunk.iter().cloned() {
            let c = client.clone();
            set.spawn(async move { (id, read_mod_list(&c, addr).await) });
        }
        let mut batch: Vec<(String, Vec<(u64, String)>)> = Vec::with_capacity(chunk.len());
        let mut failed: Vec<String> = Vec::new();
        // `while let Some(Ok(..))` stopped at the first cancelled task and dropped the
        // rest of the chunk on the floor; `verify_many` already had the right shape.
        while let Some(joined) = set.join_next().await {
            let Ok((id, mods)) = joined else { continue };
            match mods {
                Some(m) => batch.push((id, m)),
                None => failed.push(id),
            }
        }
        // A chunk in which no server answered is likelier this PC's connection than 200
        // servers at once. Recorded, each of them waited six hours (twelve after a
        // second blip) before it was asked again, and the Mods page and the mod filter
        // went without them (row 14, H6). The scan stops instead, and the next asks again.
        if batch.is_empty() && !failed.is_empty() && !connection_up().await {
            crate::log_warn!(
                "mods",
                "scan stopped: none of {} server(s) answered and Steam's web API is out of reach too, so none was recorded as failed",
                failed.len()
            );
            set_net(&app, false);
            break;
        }
        // Only a server that fails twice waits before its next read (D-329).
        if !failed.is_empty() {
            let lost: Vec<(String, SocketAddr)> = chunk
                .iter()
                .filter(|(id, _)| failed.contains(id))
                .cloned()
                .collect();
            let (read, again) = second_look(&client, lost, SECOND_LOOK_PAUSE).await;
            second_looks += read.len();
            batch.extend(read);
            failed = again;
        }
        summary.scanned += batch.len();
        summary.failed += failed.len();
        let payload: Vec<ServerMods> = batch
            .iter()
            .map(|(id, mods)| ServerMods {
                id: id.clone(),
                mods: mods.iter().map(|(m, _)| *m).collect(),
            })
            .collect();
        // The same handful of popular mods appears on most servers in a chunk, so
        // sending every occurrence made this event ~10x larger than it needs to be
        // (200 servers x ~30 mods vs a few hundred distinct ids) — docs/05 §4 caps
        // an event at ~200 KB (D-164).
        let mut seen: HashSet<u64> = HashSet::with_capacity(256);
        let mut names: Vec<(u64, String)> = Vec::with_capacity(256);
        for (_, mods) in &batch {
            for (id, name) in mods {
                // Id 0 is no Workshop item; the catalogue leaves it out (D-221), and a
                // name sent here put it back in the dropdown until a restart (D-276).
                if *id > 0 && seen.insert(*id) {
                    names.push((*id, name.clone()));
                }
            }
        }
        // Stored first, then sent: sent first, a batch could miss the snapshot of a
        // stored-list read already under way and be overwritten by it in the store
        // (D-281).
        let c = Arc::clone(&cache);
        let failed_store = failed.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            if let Ok(mut c) = c.lock() {
                // One transaction for the chunk, not one per server: measured
                // 43-51 ms against 8-10 ms for 200 servers (D-164).
                if let Err(e) = c.replace_server_mods_many(&batch, now) {
                    crate::log_error!(
                        "cache",
                        "server mods for {} row(s) failed: {e}",
                        batch.len()
                    );
                }
                // So the next pass waits on them instead of asking again (D-244).
                if let Err(e) = c.record_scan_failures(&failed_store, now) {
                    crate::log_warn!("cache", "failed mod reads not recorded: {e}");
                }
            }
        })
        .await;
        // The failed ids too: they have no list and wait before the next read (D-244),
        // so the "not scanned yet" count leaves them out rather than offering a scan
        // that will not ask them (D-276).
        send_rows(&app, "mods", &(payload, names, &failed));
    }
    summary.elapsed_ms = t0.elapsed().as_millis() as u64;
    if summary.total > 0 || held_back > 0 {
        crate::log_info!(
            "mods",
            "scan: {} server(s) read ({second_looks} on a second look), {} failed, {} held back after failing, in {} ms",
            summary.scanned,
            summary.failed,
            held_back,
            summary.elapsed_ms
        );
    }
    #[cfg(debug_assertions)]
    eprintln!(
        "[mods] scan done: total={} scanned={} failed={} in {} ms",
        summary.total, summary.scanned, summary.failed, summary.elapsed_ms
    );
    shrink_cache(&cache).await;
    send_rows(&app, "mods-done", &summary);
    summary
}

/// Workshop items with an update waiting, asked of the running Steam client rather
/// than read from its `.acf` — the file is only as fresh as the last time Steam
/// checked, so a mod its author updated could sit stale indefinitely (D-191).
/// `null` means Steam could not be asked, which is not "nothing is stale": the caller
/// keeps the file's answer in that case.
#[tauri::command]
pub async fn mods_stale(state: State<'_, AppState>, ids: Vec<u64>) -> AppResult<Option<Vec<u64>>> {
    let steam = state.steam.clone_handle();
    tauri::async_runtime::spawn_blocking(move || steam.stale_items(ids))
        .await
        .map_err(|e| {
            AppError::logged(
                "mods",
                "Steam could not be asked about mod updates",
                format!("stale check failed: {e}"),
            )
        })
}

/// Stored mod lists and the mod catalogue for the browser's mod filter (D-080).
#[tauri::command]
pub async fn mods_index(state: State<'_, AppState>) -> AppResult<crate::browser::ModsIndex> {
    let cache = Arc::clone(&state.cache);
    tauri::async_runtime::spawn_blocking(move || {
        let c = cache
            .lock()
            .map_err(|_| saved_data("cache lock poisoned"))?;
        c.mods_index().map_err(saved_data_sql)
    })
    .await
    .map_err(|e| {
        AppError::logged(
            "mods",
            "The servers' mod lists could not be loaded",
            format!("mods index task failed: {e}"),
        )
    })?
}

/// Starts a mod scan now, by the automatic scan's rules: the lists missing or a day old,
/// less the servers waiting after a failed read (D-244). Nothing ever asked for the
/// `force` that skipped both, so it went (D-276). The targets and the outcome arrive as
/// `servers:mods-*` events.
#[tauri::command]
pub async fn mods_scan(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    // The claim lives inside `run_mod_scan` so that the automatic scan after a refresh
    // is covered too — it calls the function directly and took no flag at all, which is
    // two scans at 200 pps over the same chunks (D-204).
    // …but a button press that lands on a running scan has to say so. Without this the
    // command returned Ok, the spawned task logged "ignored" and gave up, and the only
    // thing the user saw was the store's watchdog clearing the flag once its 20-minute
    // deadline passed (D-209).
    if state.scanning.load(Ordering::Acquire) {
        return Err(AppError::Internal("A mod scan is already running.".into()));
    }
    let (app2, cache, client, scanning) = (
        app,
        Arc::clone(&state.cache),
        state.a2s.clone(),
        Arc::clone(&state.scanning),
    );
    tauri::async_runtime::spawn(run_mod_scan(app2, cache, client, scanning));
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeResult {
    pub id: u64,
    pub ok: bool,
    pub error: Option<String>,
}

/// Unsubscribes Workshop items through Steam (mod management, D-075). Steam deletes
/// the files itself, once nothing under app 221100 is running; `!Workshop` junctions
/// are never touched (they may belong to the official launcher).
#[tauri::command]
pub async fn mods_unsubscribe(
    state: State<'_, AppState>,
    ids: Vec<u64>,
) -> AppResult<Vec<UnsubscribeResult>> {
    let steam = state.steam.clone_handle();
    let results = tauri::async_runtime::spawn_blocking(move || steam.unsubscribe(&ids))
        .await
        .map_err(|e| {
            AppError::logged(
                "mods",
                "The unsubscribe did not finish",
                format!("unsubscribe task failed: {e}"),
            )
        })?
        .map_err(AppError::Internal)?;
    let out: Vec<UnsubscribeResult> = results
        .into_iter()
        .map(|(id, r)| UnsubscribeResult {
            id,
            ok: r.is_ok(),
            error: r.err(),
        })
        .collect();
    let failed: Vec<String> = out
        .iter()
        .filter(|r| !r.ok)
        .map(|r| format!("{} ({})", r.id, r.error.as_deref().unwrap_or("?")))
        .collect();
    crate::log_info!(
        "mods",
        "unsubscribed {} of {}{}",
        out.iter().filter(|r| r.ok).count(),
        out.len(),
        if failed.is_empty() {
            String::new()
        } else {
            format!("; failed: {}", failed.join(", "))
        }
    );
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunctionFailure {
    pub name: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunctionCleanup {
    pub removed: Vec<String>,
    pub failed: Vec<JunctionFailure>,
}

/// Removes `!Workshop` junctions whose target folder is gone (D-093, restored to the
/// Mods page in D-170). Only the confirmed button calls this: junctions are shared
/// with the official launcher and are never deleted on the launcher's own initiative.
#[tauri::command]
pub async fn junctions_remove_dangling() -> AppResult<JunctionCleanup> {
    tauri::async_runtime::spawn_blocking(|| {
        let steam = registry::detect();
        let path = steam
            .path
            .ok_or_else(|| AppError::Internal("Steam is not installed on this PC.".into()))?;
        let libs = locate::libraries(&path)?;
        let game = locate::find_dayz(&libs)?.ok_or_else(|| {
            let gone = locate::unreachable_dayz_libraries(&libs);
            AppError::Internal(match gone.first() {
                Some(p) => format!(
                    "Steam has DayZ in {}, but that folder is not reachable; connect the drive and try again.",
                    p.display()
                ),
                None => "DayZ is not installed on this PC.".into(),
            })
        })?;
        let mut out = JunctionCleanup {
            removed: Vec::new(),
            failed: Vec::new(),
        };
        // One line per junction, target included: the counts alone could not say what a
        // clean-up had removed (D-276).
        for (name, target, r) in workshop::remove_dangling(&game.workshop_dir()) {
            let to = target.as_deref().map_or_else(|| "?".into(), |t| t.display().to_string());
            match r {
                Ok(()) => {
                    crate::log_info!("junctions", "removed {name} -> {to}");
                    out.removed.push(name);
                }
                Err(error) => {
                    crate::log_warn!("junctions", "could not remove {name} -> {to}: {error}");
                    out.failed.push(JunctionFailure { name, error });
                }
            }
        }
        if !out.removed.is_empty() || !out.failed.is_empty() {
            crate::log_info!(
                "junctions",
                "removed {} dangling, {} failed",
                out.removed.len(),
                out.failed.len()
            );
        }
        Ok(out)
    })
    .await
    .map_err(|e| AppError::logged("mods", "The mod links could not be checked", format!("junction task failed: {e}")))?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsCached {
    pub items: Vec<crate::news::NewsItem>,
}

fn news_thumb_dir(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| AppError::Internal(format!("no local data dir: {e}")))?
        .join("news-thumbs"))
}

/// The list a check read becomes the cached one: a launch whose own RULES read drops
/// falls back to the cache, which could be a day-old scan rather than the list the
/// player was shown seconds earlier (D-265). In the scan's shape, and sent to the list
/// as a scan's is, so the Mods column agrees with it (D-295); from the details pane as
/// well, which showed its live list beside a column a day old (row 26).
async fn keep_mod_list(
    app: &AppHandle,
    cache: &Arc<Mutex<Cache>>,
    id: &str,
    rules: &a2s::A2sResult<a2s::Reply<a2s::Rules>>,
) {
    let Ok(r) = rules else { return };
    if r.value.dayz.is_none() {
        return;
    }
    let stored = r.value.stored_mods();
    let payload = vec![ServerMods {
        id: id.to_string(),
        mods: stored.iter().map(|(m, _)| *m).collect(),
    }];
    let names: Vec<(u64, String)> = stored.iter().filter(|(m, _)| *m > 0).cloned().collect();
    let list = vec![(id.to_string(), stored)];
    let c = Arc::clone(cache);
    let now = ServerRow::now_unix();
    let saved = tauri::async_runtime::spawn_blocking(move || {
        c.lock().is_ok_and(|mut c| {
            c.replace_server_mods_many(&list, now)
                .map_err(|e| crate::log_warn!("cache", "a server's mod list was not stored: {e}"))
                .is_ok()
        })
    })
    .await
    .unwrap_or(false);
    // Stored first, then sent, as the scan does (D-281).
    if saved {
        send_rows(app, "mods", &(payload, names, Vec::<String>::new()));
    }
}

/// The server's own mod list from its RULES reply, or why there is none. A reply
/// without its DayZ part is no list either: read as "no mods", a modded server was
/// planned and launched bare, and turned away (row 26).
fn mod_list_of(
    rules: &a2s::A2sResult<a2s::Reply<a2s::Rules>>,
) -> Result<Vec<(u64, String)>, String> {
    match rules {
        Ok(r) if r.value.dayz.is_some() => Ok(r.value.required_mods()),
        Ok(_) => Err("the reply had no DayZ part".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// INFO, RULES and PLAYER together, and a second, patient look at what the first lost
/// when PLAYER was among it. The pass and the visible rows have held a lost PLAYER back
/// for that look since D-308 (5); the pane gave it only when INFO was lost too, so one
/// dropped PLAYER reply published "unverifiable" and a never-counted server left the
/// default list for four minutes (row 26). Without one: one timed-out click published
/// "offline" and hid a server verified a minute before (D-272).
async fn details_queries(
    client: &Client,
    addr: SocketAddr,
) -> (
    a2s::A2sResult<a2s::Reply<a2s::Info>>,
    a2s::A2sResult<a2s::Reply<a2s::Rules>>,
    a2s::A2sResult<a2s::Reply<a2s::Players>>,
) {
    let (mut info, mut rules, mut players) =
        tokio::join!(client.info(addr), client.rules(addr), client.players(addr));
    // Another game answering: nothing of this server to look at again.
    let other_game = info.as_ref().is_ok_and(|r| !r.value.is_dayz());
    if players.is_err() && !other_game {
        let patient = client.clone().with_timeout(PATIENT_TIMEOUT);
        let (info_lost, rules_lost) = (info.is_err(), rules.is_err());
        let (i, r, p) = tokio::join!(
            async {
                if info_lost {
                    Some(patient.info(addr).await)
                } else {
                    None
                }
            },
            async {
                if rules_lost {
                    Some(patient.rules(addr).await)
                } else {
                    None
                }
            },
            patient.players(addr)
        );
        if let Some(i) = i {
            info = i;
        }
        if let Some(r) = r {
            rules = r;
        }
        players = p;
    }
    (info, rules, players)
}

/// Latest DayZ news from Steam (D-099); the result is kept in the cache's meta table
/// so the next start paints it before the network answers. Thumbnails of posts that
/// dropped off the list are removed (D-111).
#[tauri::command]
pub async fn news_fetch(app: AppHandle, state: State<'_, AppState>) -> AppResult<NewsCached> {
    let items = crate::news::fetch(60).await.map_err(AppError::Internal)?;
    // A valid reply with no posts is Steam having a bad day, not the news being gone.
    // Writing it over the cache emptied the page and then deleted every thumbnail,
    // because the keep-set was empty too — and since D-122 removed Refresh, the only
    // way back was to wait half an hour (D-194).
    if items.is_empty() {
        crate::log_warn!("news", "Steam returned no posts; keeping the cached ones");
        return news_cached(state).await;
    }
    let json =
        serde_json::to_string(&items).map_err(|e| AppError::Internal(format!("news: {e}")))?;
    let keep: std::collections::HashSet<String> = items.iter().map(|n| n.gid.clone()).collect();
    let dir = news_thumb_dir(&app)?;
    let c = Arc::clone(&state.cache);
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Ok(c) = c.lock() {
            // The fetch time went with the page's last reader of it (D-229, D-257).
            let _ = c.set_meta("news", &json);
        }
        crate::news::prune_thumbnails(&dir, &keep);
    })
    .await;
    Ok(NewsCached { items })
}

/// A downscaled JPEG of a post's picture (D-111), cached on disk; the bytes travel
/// as a raw IPC response, not JSON.
#[tauri::command]
pub async fn news_thumb(
    app: AppHandle,
    gid: String,
    url: String,
    max: Option<u32>,
) -> AppResult<tauri::ipc::Response> {
    let dir = news_thumb_dir(&app)?;
    let bytes = crate::news::thumbnail(url, dir, gid, max.unwrap_or(640))
        .await
        .map_err(AppError::Internal)?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// The last fetched news list, if any.
#[tauri::command]
pub async fn news_cached(state: State<'_, AppState>) -> AppResult<NewsCached> {
    let c = Arc::clone(&state.cache);
    tauri::async_runtime::spawn_blocking(move || {
        let c = c.lock().map_err(|_| saved_data("cache lock poisoned"))?;
        let mut items: Vec<crate::news::NewsItem> = c
            .get_meta("news")
            .ok()
            .flatten()
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default();
        // Stored under an older rule: flagged again as the host flags a fetch (row 24).
        for n in &mut items {
            n.update = n.official && crate::news::is_update_title(&n.title);
        }
        Ok(NewsCached { items })
    })
    .await
    .map_err(|e| {
        AppError::logged(
            "news",
            "The news could not be loaded",
            format!("news task failed: {e}"),
        )
    })?
}

/// A friend's 32×32 Steam avatar for the Friends tab (D-115); `None` until Steam has it.
#[tauri::command]
pub async fn friend_avatar(
    state: State<'_, AppState>,
    steam_id: String,
) -> AppResult<tauri::ipc::Response> {
    let id: u64 = steam_id
        .parse()
        .map_err(|_| AppError::Internal(format!("bad steam id {steam_id}")))?;
    let steam = state.steam.clone_handle();
    let avatar = tauri::async_runtime::spawn_blocking(move || steam.friend_avatar(id))
        .await
        .map_err(|e| {
            AppError::logged(
                "steam",
                "A Steam picture could not be loaded",
                format!("avatar task failed: {e}"),
            )
        })?
        .map_err(AppError::Internal)?;
    // Raw RGBA, always 32x32; empty means Steam has not cached it yet (D-181).
    Ok(tauri::ipc::Response::new(
        avatar.map(|a| a.rgba).unwrap_or_default(),
    ))
}

/// The most recent diagnostic entries, newest last (D-158).
#[tauri::command]
pub fn logs_recent(limit: Option<usize>) -> Vec<crate::log::Entry> {
    crate::log::recent(limit.unwrap_or(200).min(400))
}

/// The log file for the Logs page: where it is, whether it is there, and why it cannot
/// be written while it cannot (row 25).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFile {
    path: Option<String>,
    exists: bool,
    error: Option<String>,
}

/// Where the log file lives, for "show me the folder". `async`: a file check, off the
/// main thread (docs/05 §4, row 27).
#[tauri::command(async)]
pub fn logs_path() -> LogFile {
    let path = crate::log::path();
    LogFile {
        exists: path.as_ref().is_some_and(|p| p.is_file()),
        path: path.map(|p| p.to_string_lossy().into_owned()),
        error: crate::log::file_error(),
    }
}

/// Opens a web page in the player's browser (D-099). While the launcher runs as
/// administrator and the desktop does not — matched to an elevated Steam (D-119), or
/// started with Run as administrator — the opener's in-process ShellExecute started the
/// browser as administrator too, for every site it then showed, the one-click elevation
/// D-312 closed for Start Steam: the desktop's own shell opens the page instead, as the
/// signed-in player (`proc::open_as_desktop_user`, row 27). Web addresses only.
///
/// On the blocking pool, not one of the runtime's four workers: Explorer can take up to
/// 10 s to answer, and a few clicks while it was busy held every worker, the servers'
/// queries with them (row 28).
#[tauri::command]
pub async fn open_link(app: AppHandle, url: String) -> AppResult<()> {
    if !is_web_address(&url) {
        return Err(AppError::logged(
            "app",
            "That link could not be opened",
            format!(
                "not a web address: {}",
                url.chars().take(200).collect::<String>()
            ),
        ));
    }
    let target = url.clone();
    let opened = tauri::async_runtime::spawn_blocking(move || {
        if crate::proc::current_is_elevated() && crate::proc::shell_is_elevated() != Some(true) {
            crate::proc::open_as_desktop_user(&target)
        } else {
            use tauri_plugin_opener::OpenerExt;
            // The browser may start here, as this process's child (row 28).
            crate::proc::at_normal_priority(|| app.opener().open_url(&target, None::<&str>))
                .map_err(|e| e.to_string())
        }
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()));
    opened.map_err(|e| {
        AppError::logged(
            "app",
            "The page could not be opened in your browser",
            format!("{url}: {e}"),
        )
    })
}

/// `http(s)://` and nothing a shell could read as more: no spaces, quotes or control
/// characters, which a well-formed address carries percent-encoded.
fn is_web_address(url: &str) -> bool {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && url.len() <= 2048
        && url.len() > lower.find("//").map_or(0, |i| i + 2)
        && !url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '"')
}

/// The page has kept what it had to before the window closes (`app:closing`, row 27).
#[tauri::command]
pub fn close_ready(app: AppHandle) {
    crate::finish_close(&app, true);
}

/// The WebView's own diagnostics: unhandled errors, failed commands, view timings.
/// Frontend levels are clamped to the same three, and the target is prefixed so the
/// origin is never ambiguous in the file. `async` because the entry is appended to
/// launcher.log, and a plain command runs on the main thread (D-239).
#[tauri::command(async)]
pub fn log_ui(level: String, target: String, message: String, at: Option<u64>) {
    let lvl = match level.as_str() {
        "error" => crate::log::Level::Error,
        "warn" => crate::log::Level::Warn,
        _ => crate::log::Level::Info,
    };
    // One entry is one line in launcher.log, so a message carrying newlines could
    // forge entries and make the log untrustworthy to read back (D-163).
    let flatten = |s: &str, n: usize| -> String {
        s.chars()
            .take(n)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect()
    };
    let target = format!("ui:{}", flatten(&target, 24));
    // At the page's own time when it sends one, so its lines keep their order (row 25);
    // cut to 8 000 here and to the log's own cap after the profile folder is out (row 25).
    crate::log::write_from_page(lvl, &target, flatten(&message, 8000), at);
}

/// Steam friends with presence and, for those in DayZ, their server (D-092).
#[tauri::command]
pub async fn friends_list(state: State<'_, AppState>) -> AppResult<Vec<FriendInfo>> {
    let steam = state.steam.clone_handle();
    tauri::async_runtime::spawn_blocking(move || steam.friends())
        .await
        .map_err(|e| {
            AppError::logged(
                "steam",
                "Your friends could not be loaded",
                format!("friends task failed: {e}"),
            )
        })?
        .map_err(AppError::Internal)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSlots {
    /// The server's own count: this is what decides whether a connection is accepted.
    pub players: i32,
    pub max_players: i32,
    /// Players in the login queue (`lqs` keyword), if the server reports it.
    pub queue: Option<u32>,
    pub ping_ms: u32,
}

/// INFO only: player count, capacity and login queue for the "wait for a free
/// slot" join option (D-074). One datagram each way, safe to poll every 10 s.
#[tauri::command]
pub async fn server_slots(state: State<'_, AppState>, id: String) -> AppResult<ServerSlots> {
    let addr: SocketAddr = id.parse().map_err(|_| {
        AppError::logged(
            "join",
            "That server could not be found",
            format!("bad server id {id}"),
        )
    })?;
    // Another game on the port is not the server this dialog joins (row 18).
    let reply = state
        .a2s
        .info(addr)
        .await
        .ok()
        .filter(|r| r.value.is_dayz())
        .ok_or_else(|| AppError::Internal("The server did not answer.".into()))?;
    Ok(ServerSlots {
        players: reply.value.players as i32,
        max_players: reply.value.max_players as i32,
        queue: reply.value.tags.queue,
        ping_ms: reply.rtt.as_millis() as u32,
    })
}

/// Live INFO + RULES + PLAYER for one server (details pane, join flow).
#[tauri::command]
pub async fn server_details(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> AppResult<ServerDetails> {
    let addr: SocketAddr = id.parse().map_err(|_| {
        AppError::logged(
            "verify",
            "That server could not be found",
            format!("bad server id {id}"),
        )
    })?;
    let client = state.a2s.clone();
    let cache = Arc::clone(&state.cache);
    let cached = cached_row(&cache, &id).await;
    let (reported, max_players) = cached
        .as_ref()
        .map_or((0, 0), |r| (r.players, r.max_players));
    let was_synthetic = cached
        .as_ref()
        .is_some_and(|r| r.verdict.as_deref() == Some("synthetic"));
    let known = cached.as_ref().map(verify::InfoFacts::of_row);

    let (mut info, mut rules, mut players) = details_queries(&client, addr).await;
    // Another game answering on the port is no answer from this server (row 18).
    if info.as_ref().is_ok_and(|r| !r.value.is_dayz()) {
        let other = || a2s::A2sError::Malformed("reply from another game");
        info = Err(other());
        players = Err(other());
        rules = Err(other());
    }
    // With INFO lost, a cached count older than a minute is no count, as in
    // `verify_one` (D-245): judged against a fresh PLAYER, an hour-old 60 read as
    // "advertises 60; 20 actually connected" and hid the row the user had open (D-268).
    let stale = cached.as_ref().is_none_or(|r| {
        ServerRow::now_unix().saturating_sub(r.last_seen) > verify::INFO_FRESH_SECS
    });
    let judged_against = if stale && info.is_err() { -1 } else { reported };
    // With R11, like every other check: `judge` alone published "verified" over a
    // standing "synthetic" verdict each time the row was opened (D-237).
    let (verdict, verified, reason) = verify::judge_with_continuity(
        &id,
        info.as_ref().ok().map(|r| &r.value),
        players.as_ref().map(|r| &r.value),
        judged_against,
        max_players,
        was_synthetic,
    );
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
        // As the pass sends it: a ping never measured takes PLAYER's round trip (D-247),
        // and after a PLAYER-only answer the pane's check left it at "—" (row 26).
        player_rtt_ms: players.as_ref().ok().map(|r| r.rtt.as_millis() as u32),
        keywords: info.as_ref().ok().and_then(|r| r.value.keywords.clone()),
        tags: info.as_ref().ok().map(|r| r.value.tags.clone()),
        verified_at: ServerRow::now_unix(),
        reason,
        facts: info
            .as_ref()
            .ok()
            .map(|r| verify::InfoFacts::of(&r.value))
            .filter(|f| known.as_ref().is_none_or(|k| f.changes(k))),
    };
    // The pane still says what it saw; the row keeps its verdict while this PC cannot
    // reach the internet, as for every other check (row 14, F1/H1).
    if verdict != Verdict::Offline || connection_up().await {
        // Hidden now, and a hidden row is checked again only when something looks at it:
        // read once more a few minutes on, as the list's checks are (row 18).
        if matches!(verdict, Verdict::Offline | Verdict::Unverifiable) {
            if let Some(t) = cached.as_ref().and_then(Target::from_row) {
                schedule_reread(&app, &cache, &client, vec![t], "the details pane");
            }
        }
        // As every check publishes: the verdict sent and stored, and a verified count
        // kept as a population sample. The pane stored the verdict only, so a server
        // opened in it read "No population samples yet" under its own count until the
        // list's check came round (row 26).
        publish(
            &app,
            &cache,
            &mut HashMap::new(),
            vec![verification.clone()],
        )
        .await;
        keep_mod_list(&app, &cache, &id, &rules).await;
    } else {
        crate::log_warn!(
            "verify",
            "{id} did not answer and Steam's web API is out of reach too: the connection looks down, so it was not recorded as offline"
        );
        set_net(&app, false);
    }

    // Only when the server sent no list: its own is the one to show.
    let scanned = match mod_list_of(&rules) {
        Ok(_) => None,
        Err(_) => cached_mods(&cache, &id)
            .await
            .map(|s| scanned_list(s, ServerRow::now_unix())),
    };

    Ok(ServerDetails {
        id,
        scanned,
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
    tauri::async_runtime::spawn_blocking(move || {
        c.lock().ok().and_then(|c| c.get(&key).ok().flatten())
    })
    .await
    .ok()
    .flatten()
}

/// The mod list the last scan recorded for one server, with when it was scanned.
/// `Some((_, []))` means the scan found it vanilla, which is not the same as never
/// having looked.
async fn cached_mods(
    cache: &Arc<Mutex<Cache>>,
    id: &str,
) -> Option<crate::browser::cache::ScannedMods> {
    let c = Arc::clone(cache);
    let key = id.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        c.lock()
            .ok()
            .and_then(|c| c.server_mods(&key).ok().flatten())
    })
    .await
    .ok()
    .flatten()
}

/// The page cache back after a pass or a scan (`Cache::shrink_memory`, D-297).
async fn shrink_cache(cache: &Arc<Mutex<Cache>>) {
    let c = Arc::clone(cache);
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Ok(c) = c.lock() {
            c.shrink_memory();
        }
    })
    .await;
}

/// Keeps what a join's own INFO read said about the game port and the password when it
/// differs from the cached row: only a Steam listing or a direct connect changed them,
/// so after a server moved its game port a launch whose own read dropped went to the
/// old one, and Recent recorded the old one (D-295). The list is sent the row too: it
/// kept the old address, which the details pane showed and copied for the rest of the
/// session (row 23).
async fn keep_join_facts(
    app: &AppHandle,
    cache: &Arc<Mutex<Cache>>,
    row: &ServerRow,
    game_port: u16,
    password: bool,
) {
    if row.game_port == game_port && row.password == password {
        return;
    }
    if row.game_port != game_port {
        crate::log_info!(
            "join",
            "{} answers on game port {game_port} now, not {}",
            row.id,
            row.game_port
        );
    }
    let c = Arc::clone(cache);
    let id = row.id.clone();
    let stored = tauri::async_runtime::spawn_blocking(move || {
        let c = c.lock().ok()?;
        match c.update_join_facts(&id, game_port, password) {
            Ok(n) if n > 0 => c.get(&id).ok().flatten(),
            Ok(_) => None,
            Err(e) => {
                crate::log_warn!("cache", "the join's game port was not kept: {e}");
                None
            }
        }
    })
    .await
    .ok()
    .flatten();
    // Without Steam's flag, which the list keeps as it has it, as for an import (D-281).
    if let Some(mut r) = stored {
        r.steam_empty = None;
        send_rows(app, "batch", &vec![r]);
    }
}

/// Orders two DayZ versions ("1.29.163709") by their numbers; `None` when either is not
/// all numbers (D-296).
fn version_order(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    let parse = |s: &str| {
        s.split('.')
            .map(|p| p.trim().parse::<u64>().ok())
            .collect::<Option<Vec<u64>>>()
    };
    Some(parse(a)?.cmp(&parse(b)?))
}

/// "12 minutes ago" / "3 hours ago" / "2 days ago", for text the user reads once.
fn humanise_age(secs: i64) -> String {
    let plural = |n: i64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match secs {
        s if s < 90 => "just now".to_string(),
        s if s < 90 * 60 => plural(s / 60, "minute"),
        s if s < 36 * 3600 => plural(s / 3600, "hour"),
        s => plural(s / 86_400, "day"),
    }
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

/// Everything the join dialog needs: required mods (live RULES, else the stored list),
/// local state, Steam titles/sizes for every required mod, and preconditions.
#[tauri::command]
pub async fn join_plan(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> AppResult<JoinPlan> {
    let row = cached_row(&state.cache, &id)
        .await
        .ok_or_else(|| AppError::Internal("That server is no longer in the server list.".into()))?;
    let addr: SocketAddr = id.parse().map_err(|_| {
        AppError::logged(
            "join",
            "That server could not be found",
            format!("bad server id {id}"),
        )
    })?;
    // INFO beside RULES: the cached row's game port, password flag and version are
    // only as fresh as the last listing, and verification never updates them, so a
    // server that moved its game port or added a password was planned — and joined —
    // from stale facts (D-265).
    let (rules, info) = tokio::join!(state.a2s.rules(addr), state.a2s.info(addr));
    // Another game answering on the port is no answer from this server (row 18): its game
    // port, password flag and version were planned, joined and kept as this server's (row 28).
    let info = info.ok().map(|r| r.value).filter(|i| i.is_dayz());
    let diag = tauri::async_runtime::spawn_blocking(diagnostics::collect)
        .await
        .map_err(|e| {
            AppError::logged(
                "join",
                "The check of Steam and DayZ did not finish",
                format!("diagnostics task failed: {e}"),
            )
        })??;
    keep_mod_list(&app, &state.cache, &id, &rules).await;
    let server_version = info
        .as_ref()
        .map_or_else(|| row.version.clone(), |i| i.version.clone());
    let password_required = info.as_ref().map_or(row.password, |i| i.password);
    let game_port = info
        .as_ref()
        .and_then(|i| i.game_port)
        .unwrap_or(row.game_port);
    if info.is_some() {
        keep_join_facts(&app, &state.cache, &row, game_port, password_required).await;
    }

    let mut warnings = Vec::new();
    let required: Vec<(u64, String)> = match mod_list_of(&rules) {
        // Published mods only, each once (D-221, D-265).
        Ok(mods) => mods,
        // A server that will not answer RULES is not a vanilla server. Launching
        // without its mods is a kick on arrival, so fall back to the list the mod scan
        // recorded and say how old it is rather than inventing an empty one (D-209).
        Err(e) => match cached_mods(&state.cache, &id).await {
            Some((scanned_at, mods)) if !mods.is_empty() => {
                crate::log_warn!("join", "{id}: no mod list ({e}); using the scan's");
                let age = ServerRow::now_unix().saturating_sub(scanned_at);
                warnings.push(format!(
                    "The server did not send its mod list, so the one read {} is used.",
                    humanise_age(age)
                ));
                mods
            }
            Some(_) => Vec::new(),
            // What `launch_game` will do with it: a modded server with nothing stored is
            // refused there, so the plan must not promise a launch without mods (D-289).
            None if row.tags.modded => {
                crate::log_warn!("join", "{id}: no mod list ({e}) and none scanned");
                warnings.push(
                    "Could not read the server's mod list, and it has never been scanned; Join will not start the game until the list can be read. Try again in a moment.".into(),
                );
                Vec::new()
            }
            None => {
                crate::log_warn!("join", "{id}: no mod list ({e}) and none scanned");
                warnings.push(
                    "Could not read the server's mod list, and it has never been scanned; launching without mods.".into(),
                );
                Vec::new()
            }
        },
    };
    let mut inventory: HashMap<u64, (Option<String>, bool)> = diag
        .workshop
        .as_ref()
        .map(|w| {
            w.items
                .iter()
                .map(|i| (i.id, (i.folder.clone(), i.needs_update)))
                .collect()
        })
        .unwrap_or_default();
    // When Steam's Workshop list is missing or unreadable the launch looks for the
    // content folders itself (D-256), but the plan read the same state as "nothing
    // installed", offered to download every mod, and at the two-minute re-plan blamed
    // the server for changing its mods. Same condition, same fallback (D-265).
    if let (Some(g), false) = (diag.dayz.as_ref(), required.is_empty()) {
        let lib = PathBuf::from(&g.library);
        let ids: Vec<u64> = required.iter().map(|(id, _)| *id).collect();
        let fallback = tauri::async_runtime::spawn_blocking(move || match workshop::read(&lib) {
            Ok(Some(_)) => None,
            _ => Some(workshop::from_folders(&lib, &ids)),
        })
        .await
        .ok()
        .flatten();
        for it in fallback.map(|w| w.items).unwrap_or_default() {
            if let Some(f) = it.folder {
                inventory.insert(it.id, (Some(f.display().to_string()), false));
            }
        }
    }

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
                Ok(Ok((details, missed))) => {
                    let by_id: HashMap<u64, ItemDetails> =
                        details.into_iter().map(|d| (d.id, d)).collect();
                    for m in &mut mods {
                        if let Some(d) = by_id.get(&m.id) {
                            m.title = Some(d.title.clone());
                            m.size = Some(d.file_size);
                        }
                    }
                    // Some pages answered and one did not (D-295).
                    if let Some(e) = missed {
                        crate::log_warn!("join", "workshop details: {e}");
                        warnings.push(NO_DETAILS.into());
                    }
                }
                Ok(Err(e)) => {
                    crate::log_warn!("join", "workshop details: {e}");
                    warnings.push(NO_DETAILS.into());
                }
                Err(e) => {
                    crate::log_warn!("join", "workshop details task failed: {e}");
                    warnings.push(NO_DETAILS.into());
                }
            }
            // `needs_update` above came from `appworkshop_221100.acf`, which is only as
            // fresh as the last time Steam checked; a mod its author updated can read as
            // up to date there and be rejected by the server. The running client knows
            // better, and it only reports items the user is actually subscribed to —
            // Steam does not maintain the others, so an update offered for one is an
            // update that will never arrive. Its answer replaces the file's; the file is
            // the fallback for when it cannot be asked (D-191).
            let installed: Vec<u64> = mods.iter().filter(|m| m.installed).map(|m| m.id).collect();
            if !installed.is_empty() {
                let cmd = state.steam.clone_handle();
                if let Ok(Some(stale)) =
                    tauri::async_runtime::spawn_blocking(move || cmd.stale_items(installed)).await
                {
                    let stale: HashSet<u64> = stale.into_iter().collect();
                    for m in &mut mods {
                        m.needs_update = stale.contains(&m.id);
                    }
                }
            }
        }
    }

    let missing = mods.iter().filter(|m| !m.installed).count();
    let updates = mods
        .iter()
        .filter(|m| m.installed && m.needs_update)
        .count();
    let download_bytes = mods
        .iter()
        .filter(|m| !m.installed || m.needs_update)
        .filter_map(|m| m.size)
        .sum();
    let local_version = diag.dayz.as_ref().and_then(|g| g.game_version.clone());
    // An unknown server version is no mismatch: it read "Server runs  but…" (D-296).
    let version_mismatch = !server_version.is_empty()
        && local_version
            .as_deref()
            .is_some_and(|v| v != server_version);
    if let (true, Some(local)) = (version_mismatch, local_version.as_deref()) {
        // Which side is behind. For hours after a DayZ patch the game is current and the
        // servers are not, and the warning sent the player to Steam, which had nothing
        // to update (D-296).
        let remedy = match version_order(local, &server_version) {
            Some(std::cmp::Ordering::Less) => {
                "; the server will reject the connection until Steam updates your game."
            }
            Some(std::cmp::Ordering::Greater) => {
                "; the server will reject the connection until the server is updated to your version."
            }
            _ => "; the server will reject the connection.",
        };
        warnings.push(format!(
            "Server runs {server_version} but your DayZ is {local}{remedy}"
        ));
    }
    // Without an install, the machine check below says "Steam is not installed" (D-296).
    let steam_installed = diag.steam.path.is_some();
    if steam_installed && !diag.steam.running {
        warnings.push("Steam is not running; DayZ cannot start without it.".into());
    }
    // Re-checked here, not taken from start-up (D-288): the launcher often starts before
    // Steam, and a missing Steam pid reads as "matched", so the start-up answer was
    // usually the wrong one by the time anybody pressed Join. `diag` already has the
    // live pid from this call.
    let elevation = if diag.steam.pid == 0 {
        crate::elevation()
    } else {
        crate::proc::elevation_state(diag.steam.pid)
    };
    if elevation != crate::elevation() {
        crate::log_info!(
            "launch",
            "elevation re-checked against live Steam pid {}: {:?} (start-up said {:?})",
            diag.steam.pid,
            elevation,
            crate::elevation()
        );
    }
    match elevation {
        crate::proc::ElevationState::LauncherHigher => warnings.push(
            "The launcher runs as administrator but Steam does not; DayZ started from here cannot reach Steam. Start the launcher normally, or Steam as administrator.".into(),
        ),
        crate::proc::ElevationState::SteamHigher => warnings.push(
            "Steam runs as administrator but the launcher does not; DayZ started from here cannot reach Steam. Start the launcher as administrator.".into(),
        ),
        crate::proc::ElevationState::Matched => {}
    }
    // Two of the three conditions that grey the Join button out had nothing to say
    // for themselves, so the dialog showed a disabled button and no reason (D-165).
    let game_found = diag.dayz.is_some();
    let battleye_present = diag.dayz.as_ref().is_some_and(|g| g.has_battleye_exe);
    if !game_found {
        // With Steam installed the machine check says it, naming the folder when the
        // library is on a drive that is not connected; this is for when it cannot (D-296).
        if !steam_installed {
            warnings.push(
                "DayZ was not found in any Steam library; install it, or check that its drive is connected."
                    .into(),
            );
        }
    } else if !battleye_present {
        warnings.push(
            "DayZ_BE.exe is missing from the game folder; verify the game files in Steam.".into(),
        );
    }
    // Whatever else the machine check turned up (no library folders, an unreadable
    // Workshop folder, nobody signed in): the user could only see these in
    // Diagnostics before, while the join in front of them quietly failed. Its Steam and
    // BattlEye lines are said above in the join's own words, and each problem once; and
    // an unreadable Workshop list does not stop this plan, which checks the folders
    // itself (D-265), so its "cannot be checked" was untrue here (D-296).
    let (unreadable_head, unreadable_tail) = diagnostics::WORKSHOP_UNREADABLE;
    // Broken mod links are the Mods page's to remove; they do not stop a join (row 16).
    let broken = diagnostics::broken_links(diag.junctions.iter().filter(|j| j.removable).count());
    warnings.extend(
        diag.warnings
            .iter()
            .filter(|w| *w != diagnostics::STEAM_NOT_RUNNING && *w != diagnostics::BATTLEYE_MISSING && **w != broken)
            .map(|w| {
                match w
                    .strip_prefix(unreadable_head)
                    .and_then(|r| r.strip_suffix(unreadable_tail))
                {
                    Some(e) => format!(
                        "Steam's Workshop list could not be read ({e}); installed mods were checked from their folders."
                    ),
                    None => w.clone(),
                }
            }),
    );

    let settings = state.settings.get();
    let profile_name = if settings.profile_name.trim().is_empty() {
        join_persona(&state).await.unwrap_or_default()
    } else {
        settings.profile_name.clone()
    };

    crate::log_info!(
        "join",
        "plan for {} ({id}): {} mod(s) required, {missing} missing, {updates} to update, {} to download, server {}{}{}",
        row.name,
        mods.len(),
        format!("{:.1} MB", download_bytes as f64 / (1024.0 * 1024.0)),
        server_version,
        if version_mismatch {
            format!(" against local {}", local_version.clone().unwrap_or_default())
        } else {
            String::new()
        },
        if warnings.is_empty() {
            String::new()
        } else {
            format!("; {} warning(s): {}", warnings.len(), warnings.join(" | "))
        }
    );
    Ok(JoinPlan {
        id,
        name: row.name,
        ip: row.ip,
        game_port,
        password_required,
        server_version,
        local_version,
        version_mismatch,
        steam_running: diag.steam.running,
        game_found,
        battleye_present,
        // A reply without its DayZ part is no list either (D-323): "Vanilla server" showed
        // beside the warning that the list could not be read (row 28).
        rules_ok: mod_list_of(&rules).is_ok(),
        missing,
        updates,
        download_bytes,
        mods,
        profile_name,
        warnings,
    })
}

/// The name a join sends when Settings sets none: the account signed in to Steam now,
/// or the status's copy when Steam cannot be asked (row 15, H8).
async fn join_persona(state: &State<'_, AppState>) -> Option<String> {
    let steam = state.steam.clone_handle();
    let live = tauri::async_runtime::spawn_blocking(move || steam.persona())
        .await
        .ok()
        .flatten();
    // The last session's name only while Steam is up: in the seconds after an account
    // switch the status is not, and the previous account's name went to DayZ (row 17).
    // With no name at all DayZ takes Steam's own.
    let status = state.steam.status();
    live.or_else(|| status.initialized.then_some(status.persona).flatten())
}

/// Subscribe + download the given Workshop items; progress via `mods:progress`,
/// completion via `mods:done` (both carry `job`). `async`: the request is logged to
/// the file first, which a plain command would do on the main thread (D-239).
#[tauri::command(async)]
pub fn mods_sync(
    state: State<'_, AppState>,
    job: u64,
    ids: Vec<u64>,
    subscribe: Option<bool>,
) -> AppResult<()> {
    crate::log_info!(
        "mods",
        "download requested for {} item(s): {}",
        ids.len(),
        ids.iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );
    // A join subscribes to what the server needs; the Mods page's Update passes false,
    // so it can only update what is still subscribed (D-276).
    state
        .steam
        .sync(job, ids, subscribe.unwrap_or(true))
        .map_err(AppError::Internal)
}

/// Builds junctions and the argument line, then starts `DayZ_BE.exe`. Returns
/// `Launched` once it has started, and emits `launch:exited` ({pid, code}) when it
/// ends (the `launch:started` event went in D-229).
#[tauri::command]
pub async fn launch_game(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    password: Option<String>,
    profile: Option<String>,
) -> AppResult<Launched> {
    // One launch at a time, and a running game refused before anything is read or
    // linked. The check came after RULES, INFO and the junctions with nothing held
    // across it, so two overlapping calls both passed it and both started DayZ (D-295).
    let Some(_launching) = InFlight::claim(&state.launching) else {
        return Err(AppError::Internal("DayZ is already starting.".into()));
    };
    refuse_if_running()?;
    let row = cached_row(&state.cache, &id)
        .await
        .ok_or_else(|| AppError::Internal("That server is no longer in the server list.".into()))?;
    let addr: SocketAddr = id.parse().map_err(|_| {
        AppError::logged(
            "launch",
            "That server could not be found",
            format!("bad server id {id}"),
        )
    })?;
    // INFO beside RULES for the game port, as in the plan (D-265), and kept when it
    // moved (D-295).
    let (rules, info) = tokio::join!(state.a2s.rules(addr), state.a2s.info(addr));
    // Another game answering on the port is no answer from this server (row 18): its game
    // port, password flag and version were planned, joined and kept as this server's (row 28).
    let info = info.ok().map(|r| r.value).filter(|i| i.is_dayz());
    let game_port = info
        .as_ref()
        .and_then(|i| i.game_port)
        .unwrap_or(row.game_port);
    if let Some(i) = &info {
        keep_join_facts(&app, &state.cache, &row, game_port, i.password).await;
    }
    let required: Vec<(u64, String)> = match mod_list_of(&rules) {
        // Published mods only, each once (D-221, D-265).
        Ok(mods) => mods,
        Err(e) => {
            // This used to fall back to an empty list, which does not mean "no mods" —
            // it means "we could not ask". DayZ then started vanilla and connected to a
            // modded server, which rejects it, and the only trace was a log line saying
            // "0 mod link(s)". RULES is the lossiest of the three queries, and the join
            // dialog plans against a *different* one, so a plan that listed twelve mods
            // could still launch with none (D-190).
            // "Never scanned" and "scanned, and every mod on it is server-side" are
            // different answers: the second launches without mods, as the join plan
            // already said it would (D-239).
            match cached_mods(&state.cache, &id).await {
                Some((_, mods)) => {
                    if !mods.is_empty() {
                        crate::log_warn!(
                            "launch",
                            "the server did not answer RULES ({e}); using the {} mod(s) from the last scan",
                            mods.len()
                        );
                    }
                    mods
                }
                None if row.tags.modded => {
                    crate::log_warn!(
                        "join",
                        "{id}: no mod list ({e}) and none cached; not started"
                    );
                    return Err(AppError::Internal(format!(
                        "Could not read {}'s mod list, so DayZ was not started: without its mods the server would turn you away. Try again in a moment.",
                        row.name
                    )));
                }
                None => Vec::new(),
            }
        }
    };
    // A saved launch profile can override the current launch settings for this
    // launch only (D-088); an unknown name falls back to the current settings.
    let settings = {
        let mut s = state.settings.get();
        if let Some(p) = profile
            .as_deref()
            .and_then(|n| s.launch_profiles.iter().find(|p| p.name == n).cloned())
        {
            s.profile_name = p.profile_name;
            s.extra_args = p.extra_args;
            s.skip_intro = p.skip_intro;
            s.no_splash = p.no_splash;
            s.no_pause = p.no_pause;
        }
        s
    };
    let persona = if settings.profile_name.trim().is_empty() {
        join_persona(&state).await
    } else {
        None
    };

    let row_for_spec = row.clone();
    let (game_dir, links, spec) = tauri::async_runtime::spawn_blocking(move || {
        let row = row_for_spec;
        let steam = registry::detect();
        if !steam.running {
            // In the log under Launching, not only as the page's failed call (row 25).
            crate::log_warn!("launch", "refused: Steam is not running");
            return Err(AppError::Internal("Steam is not running. Start Steam, then join again.".into()));
        }
        let path = steam
            .path
            .ok_or_else(|| AppError::Internal("Steam is not installed on this PC.".into()))?;
        let libs = locate::libraries(&path)?;
        let game = locate::find_dayz(&libs)?
            .ok_or_else(|| AppError::Internal("DayZ is not installed on this PC.".into()))?;
        // A vanilla server needs no Workshop list, and one that will not parse — a power
        // cut mid-write (D-194) — refused every launch, vanilla included, while the join
        // plan had already turned the same error into a warning. The content folders
        // are what the junctions point at (D-239).
        let ws = if required.is_empty() {
            None
        } else {
            let ids = || required.iter().map(|(id, _)| *id).collect::<Vec<u64>>();
            match workshop::read(&game.library.path) {
                Ok(Some(ws)) => Some(ws),
                // No manifest at all, which deleting it as Workshop troubleshooting
                // leaves behind, is the same case as one that will not parse: the
                // folders can still be there. It read as "nothing installed" and
                // refused the launch with "sync mods first" (D-256).
                Ok(None) => {
                    crate::log_warn!(
                        "launch",
                        "there is no Workshop list; looking for the mod folders directly"
                    );
                    Some(workshop::from_folders(&game.library.path, &ids()))
                }
                Err(e) => {
                    crate::log_warn!(
                        "launch",
                        "the Workshop list is unreadable ({e}); looking for the mod folders directly"
                    );
                    Some(workshop::from_folders(&game.library.path, &ids()))
                }
            }
        };
        let by_id: HashMap<u64, (PathBuf, Option<String>)> = ws
            .map(|w| {
                w.items
                    .into_iter()
                    .filter_map(|i| i.folder.map(|f| (i.id, (f, i.meta_name))))
                    .collect()
            })
            .unwrap_or_default();
        let mut items = Vec::with_capacity(required.len());
        for (id, name) in &required {
            let (folder, meta) = by_id.get(id).cloned().ok_or_else(|| {
                crate::log_warn!("launch", "refused: {name} ({id}) is not downloaded");
                AppError::Internal(format!(
                    "{name} is not downloaded yet. Download the missing mods, then join."
                ))
            })?;
            items.push((*id, folder, meta));
        }
        let links = launch::ensure_junctions(&game.folder, &items)?;
        let spec = LaunchSpec {
            // RULES lists a server's mods in the reverse of its own `-mod=`, and DayZ
            // reads `-mod=` back to front: the official launcher started `@CF;@Dabs
            // Framework;@DayZ-Editor` and the engine then loaded Dabs before CF, and our
            // own launches passed CF last. Passing RULES as it came ran every modded
            // server's load order backwards on the client; reversed, it matches what the
            // server's admin wrote and what the official launcher does (RPT logs,
            // S-89; D-008 corrected by D-265).
            mod_paths: links.iter().rev().map(|l| l.junction.clone()).collect(),
            ip: row.ip.clone(),
            game_port,
            password,
            profile_name: Some(if settings.profile_name.trim().is_empty() {
                persona.unwrap_or_default()
            } else {
                settings.profile_name.clone()
            }),
            skip_intro: settings.skip_intro,
            no_splash: settings.no_splash,
            no_pause: settings.no_pause,
            // Read here, off the async threads: the first call enumerates the GPUs.
            perf_args: crate::hardware::launch_args(
                &crate::hardware::detect(),
                &settings.extra_args,
            ),
            extra_args: settings.extra_args.clone(),
        };
        Ok::<_, AppError>((game.folder, links, spec))
    })
    .await
    .map_err(|e| AppError::logged("launch", "DayZ could not be started", format!("launch task failed: {e}")))??;

    let args = launch::build_args(&spec);
    // Spawn FIRST, then step aside (D-119, corrected in D-151): a child started by a
    // BELOW_NORMAL parent inherits that class, so lowering the launcher before the spawn
    // handed DayZ itself a below-normal priority — the opposite of the intent.
    // Asked again here: the game may have been started from Steam while the mod list
    // was read.
    refuse_if_running()?;
    let (mut child, mut launched) = match launch::spawn(&game_dir, &args) {
        Ok(v) => v,
        Err(e) => {
            crate::log_error!("launch", "spawn failed for {id}: {e}");
            return Err(e);
        }
    };
    let created: Vec<&str> = links
        .iter()
        .filter(|l| l.created)
        .map(|l| l.name.as_str())
        .collect();
    if !created.is_empty() {
        crate::log_info!(
            "mods",
            "created {} junction(s): {}",
            created.len(),
            created.join(", ")
        );
    }
    crate::log_info!(
        "launch",
        "started pid {} for {} with {} mod link(s), {} chars of arguments",
        launched.pid,
        id,
        links.len(),
        launched.command_line.len()
    );
    crate::proc::set_priority(crate::proc::Priority::BelowNormal);
    {
        let c = Arc::clone(&state.cache);
        // The port DayZ was started with, which Recent's "Join again" checks the
        // server's answer against (D-265, D-295).
        let row_for_history = ServerRow {
            game_port,
            ..row.clone()
        };
        let mods = links.len();
        // Q22: a join that is not recorded is exactly the reported symptom, so the
        // failure has to leave a trace (D-162) — and, since row 14 (F15, approved), be
        // said: the dialog shows "Not added to Recent: …".
        // In the player's words, the error itself in the log (row 16).
        launched.history_error = tauri::async_runtime::spawn_blocking(move || {
            let Ok(c) = c.lock() else {
                crate::log_error!("cache", "history_add: the cache lock is poisoned");
                return Err(SAVED_DATA_UNREADABLE);
            };
            let added = c.history_add(&row_for_history, mods);
            crate::browser::cache::note_write(&added);
            added.map_err(|e| {
                crate::log_error!("cache", "history_add failed: {e}");
                crate::browser::cache::plain_reason(&e)
            })
        })
        .await
        .unwrap_or_else(|e| {
            crate::log_error!("cache", "history_add task failed: {e}");
            Err(SAVED_DATA_UNREADABLE)
        })
        .err()
        .map(str::to_string);
    }
    #[cfg(debug_assertions)]
    eprintln!(
        "[launch] pid {} with {} mod junction(s) ({} created): {}",
        launched.pid,
        links.len(),
        links.iter().filter(|l| l.created).count(),
        launched.command_line
    );
    let pid = launched.pid;
    tauri::async_runtime::spawn_blocking(move || {
        let code = child.wait().ok().and_then(|s| s.code());
        #[cfg(debug_assertions)]
        eprintln!("[launch] pid {pid} exited with {code:?}");
        crate::proc::set_priority(crate::proc::Priority::High);
        crate::log_info!("launch", "pid {pid} exited with code {code:?}");
        let _ = app.emit("launch:exited", &LaunchExited { pid, code });
    });
    Ok(launched)
}

/// Refuses a launch while DayZ runs: a second copy fights the first for the game's own
/// single-instance lock, and its immediate exit used to restore the launcher's priority
/// while the real game was still playing (D-119, D-165).
fn refuse_if_running() -> AppResult<()> {
    if let Some(pid) = crate::steam::registry::process::find_named("DayZ_x64.exe")
        .or_else(|| crate::steam::registry::process::find_named("DayZ_BE.exe"))
    {
        crate::log_warn!("launch", "DayZ is already running as pid {pid}");
        return Err(AppError::Internal(
            "DayZ is already running; close it before joining another server.".into(),
        ));
    }
    Ok(())
}

/// Whether DayZ is running, for the confirmation before "Install and restart" (D-280):
/// the update restarts only the launcher, and a player in a game should know first.
/// Off the main thread, where a plain command would run (D-239).
#[tauri::command]
pub async fn game_running() -> AppResult<bool> {
    tauri::async_runtime::spawn_blocking(|| {
        crate::steam::registry::process::find_named("DayZ_x64.exe")
            .or_else(|| crate::steam::registry::process::find_named("DayZ_BE.exe"))
            .is_some()
    })
    .await
    .map_err(|e| {
        AppError::logged(
            "launch",
            "The check for a running DayZ did not finish",
            format!("process check failed: {e}"),
        )
    })
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
}

#[tauri::command]
pub async fn favourites_list(state: State<'_, AppState>) -> AppResult<Vec<Favourite>> {
    let c = Arc::clone(&state.cache);
    tauri::async_runtime::spawn_blocking(move || {
        let c = c.lock().map_err(|_| saved_data("cache lock poisoned"))?;
        let list = c.favourites().map_err(saved_data_sql)?;
        Ok(list.into_iter().map(|(id, _)| Favourite { id }).collect())
    })
    .await
    .map_err(|e| saved_data(format!("cache task failed: {e}")))?
}

#[tauri::command]
pub async fn favourite_set(state: State<'_, AppState>, id: String, on: bool) -> AppResult<()> {
    let c = Arc::clone(&state.cache);
    tauri::async_runtime::spawn_blocking(move || {
        let c = c.lock().map_err(|_| saved_data("cache lock poisoned"))?;
        let set = c.favourite_set(&id, on);
        crate::browser::cache::note_write(&set);
        set.map_err(saved_data_sql)
    })
    .await
    .map_err(|e| saved_data(format!("cache task failed: {e}")))?
}

#[tauri::command]
pub async fn history_list(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> AppResult<Vec<HistoryEntry>> {
    let c = Arc::clone(&state.cache);
    let limit = limit.unwrap_or(50).min(500);
    tauri::async_runtime::spawn_blocking(move || {
        let c = c.lock().map_err(|_| saved_data("cache lock poisoned"))?;
        c.history(limit).map_err(saved_data_sql)
    })
    .await
    .map_err(|e| saved_data(format!("cache task failed: {e}")))?
}

/// Removes every join from the history (the Recent view's confirmed "Clear list",
/// D-130). Returns how many rows went.
#[tauri::command]
pub async fn history_clear(state: State<'_, AppState>) -> AppResult<usize> {
    let c = Arc::clone(&state.cache);
    tauri::async_runtime::spawn_blocking(move || {
        let c = c.lock().map_err(|_| saved_data("cache lock poisoned"))?;
        c.history_clear().map_err(saved_data_sql)
    })
    .await
    .map_err(|e| saved_data(format!("cache task failed: {e}")))?
}

/// Verified head-count samples for one server over the last `hours` (default 72).
#[tauri::command]
pub async fn population_history(
    state: State<'_, AppState>,
    id: String,
    hours: Option<u32>,
) -> AppResult<Vec<PopulationSample>> {
    let c = Arc::clone(&state.cache);
    let since = ServerRow::now_unix() - hours.unwrap_or(72) as i64 * 3600;
    tauri::async_runtime::spawn_blocking(move || {
        let c = c.lock().map_err(|_| saved_data("cache lock poisoned"))?;
        c.population(&id, since).map_err(saved_data_sql)
    })
    .await
    .map_err(|e| saved_data(format!("cache task failed: {e}")))?
}

/// Query ports to try when the typed port turned out to be a game port: game+1..+3
/// (most hosts), then Steam's defaults. One host often runs several DayZ servers
/// (51.81.8.81 answers on 27016 *and* 27017, D-053), so callers must match the
/// reply's game port rather than accept the first answer.
/// The game port to look for and the query ports to try once the typed port did not
/// answer as the server: the typed port is the game port unless the caller knows it, and
/// then that game port answering for itself is tried too; the typed port is not asked
/// twice.
fn fallback_query_ports(port: u16, expect_game_port: Option<u16>) -> (u16, Vec<u16>) {
    let game_port = expect_game_port.unwrap_or(port);
    let mut ports = candidate_query_ports(game_port);
    if expect_game_port.is_some() {
        ports.insert(0, game_port);
    }
    ports.retain(|&p| p != port);
    (game_port, ports)
}

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
async fn probe_server(
    client: &Client,
    ip: std::net::IpAddr,
    ports: &[u16],
    expect_game_port: Option<u16>,
) -> Option<ServerRow> {
    let probe = client
        .clone()
        .with_timeout(std::time::Duration::from_millis(1500))
        .with_retries(0);
    // All candidates at once, answered in list order. One after another, a game port
    // typed where the host uses Steam's default query port waited out three timeouts
    // first (~6 s), and a server that was down took ~13.5 s to say so (D-239). Nine
    // datagrams at most, inside the client's own pacing and permits (D-037).
    let mut set = tokio::task::JoinSet::new();
    for (i, &qport) in ports.iter().enumerate() {
        let probe = probe.clone();
        set.spawn(async move { (i, qport, probe.info(SocketAddr::new(ip, qport)).await) });
    }
    let mut settled = vec![false; ports.len()];
    let mut found: Vec<Option<ServerRow>> = vec![None; ports.len()];
    let mut next = 0;
    while let Some(joined) = set.join_next().await {
        let Ok((i, qport, reply)) = joined else {
            continue;
        };
        settled[i] = true;
        if let Ok(reply) = reply {
            let dayz = reply.value.is_dayz();
            // Anything else answering on that port, or a sibling server on the same host.
            let ours = expect_game_port.is_none_or(|gp| reply.value.game_port == Some(gp));
            if dayz && ours {
                found[i] = Some(ServerRow::from_info(
                    &ip.to_string(),
                    qport,
                    &reply.value,
                    reply.rtt.as_millis() as u32,
                ));
            }
        }
        // The first candidate in list order wins, as soon as every earlier one is in.
        while next < ports.len() && settled[next] {
            if let Some(row) = found[next].take() {
                return Some(row);
            }
            next += 1;
        }
    }
    // Only reached with a slot that never settled (its task failed): what answered.
    found.into_iter().flatten().next()
}

/// Adds a server by `host:port` (game or query port, hostname allowed): probes
/// A2S_INFO on likely query ports, stores the row, emits it as a batch, verifies it.
#[tauri::command]
pub async fn direct_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    address: String,
    expect_game_port: Option<u16>,
) -> AppResult<ServerRow> {
    let text = address.trim().trim_start_matches("steam://connect/");
    let (host, port) = match text.rsplit_once(':') {
        Some((h, p)) => (
            h.trim_matches(|c| c == '[' || c == ']'),
            p.parse::<u16>()
                .map_err(|_| AppError::Internal(format!("“{address}” has no valid port.")))?,
        ),
        None => (text, 2302),
    };
    if host.is_empty() {
        return Err(AppError::Internal(
            "Enter an address like 51.81.8.81:2302.".into(),
        ));
    }
    let ip: std::net::IpAddr = match host.parse() {
        Ok(ip) => ip,
        Err(_) => tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| {
                crate::log_info!("join", "cannot resolve {host}: {e}");
                not_found(host)
            })?
            .map(|a| a.ip())
            .find(|ip| ip.is_ipv4())
            .ok_or_else(|| not_found(host))?,
    };
    crate::log_info!("join", "direct connect to {address}");
    // The typed port may be the query port (answers INFO directly) or the game port
    // (then find the sibling query port whose reply advertises exactly that game port).
    // Recent and Friends know the game port they want: every reply is checked against
    // it, the first one too. Anything answering on a stored port was taken as the
    // server, so an entry whose port another server holds now opened that one (row 23).
    let row = match probe_server(&state.a2s, ip, &[port], expect_game_port).await {
        Some(r) => r,
        None => {
            let (game_port, ports) = fallback_query_ports(port, expect_game_port);
            probe_server(&state.a2s, ip, &ports, Some(game_port))
                .await
                .ok_or_else(|| {
                    // The ports tried go to the log; the player gets a sentence (row 14,
                    // F16, approved), in Direct connect and on the Friends page alike.
                    crate::log_info!("join", "no DayZ server answered at {ip}:{port}; also tried query ports {ports:?} for that game port");
                    AppError::Internal(format!("No DayZ server answered at {ip}:{port}. Check the address, or the server may be offline."))
                })?
        }
    };
    let stored = vec![row.clone()];
    let c = Arc::clone(&state.cache);
    let id = row.id.clone();
    // The write was discarded and not even logged, while the command still returned
    // the row and emitted `servers:batch` - so on an unwritable cache the server
    // appeared in the grid and then `join_plan` and `launch_game`, which both start
    // from `cached_row(..).ok_or("unknown server")`, refused it with nothing in the
    // log to explain why. Every other cache write on this path reports (D-220).
    // The upsert keeps a stored verdict; the probe's row has none, and a check built
    // from it alone published "verified" over a standing "synthetic" whenever R11 had
    // no sample yet, as the details pane once did (D-237, D-268).
    let was_synthetic = tauri::async_runtime::spawn_blocking(move || match c.lock() {
        Ok(mut c) => {
            let upserted = c.upsert(&stored);
            crate::browser::cache::note_write(&upserted);
            upserted.map_err(|e| {
                crate::log_error!("cache", "direct connect upsert failed: {e}");
                format!(
                    "The server could not be saved: {}.",
                    crate::browser::cache::plain_reason(&e)
                )
            })?;
            Ok(c.get(&id)
                .ok()
                .flatten()
                .is_some_and(|r| r.verdict.as_deref() == Some("synthetic")))
        }
        Err(_) => Err("the server cache is unavailable".to_string()),
    })
    .await
    .map_err(|e| saved_data(format!("cache task failed: {e}")))?
    .map_err(AppError::Internal)?;
    send_rows(&app, "batch", &vec![row.clone()]);
    if let Some(mut t) = Target::from_row(&row) {
        t.was_synthetic |= was_synthetic;
        tauri::async_runtime::spawn(run_verification(
            app.clone(),
            Arc::clone(&state.cache),
            state.a2s.clone(),
            vec![t],
            false,
        ));
    }
    Ok(row)
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported: usize,
    pub already: usize,
    /// Added from the XML data only because the server did not answer A2S now.
    pub unreachable: usize,
    /// Entries whose address is not an IP, which the import cannot ask (row 23).
    pub skipped: usize,
    /// The official launcher has no favourites file on this PC: nothing to import, which
    /// is not an error (row 14, F7).
    pub missing: bool,
}

fn import_failed(n: usize, e: &str) -> AppError {
    crate::log_error!("cache", "favourite import of {n} row(s) failed: {e}");
    AppError::Internal(format!(
        "Read {n} favourite{} but could not save them. The details are on the Logs page.",
        if n == 1 { "" } else { "s" }
    ))
}

/// Imports the official launcher's favourites (docs/02 §7): probes each server,
/// stores a row (live or from the XML), and marks it favourite.
#[tauri::command]
pub async fn import_official_favourites(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ImportResult> {
    let path = crate::steam::official::favourites_path()
        .ok_or_else(|| AppError::Internal("LOCALAPPDATA is not set".into()))?;
    let entries = match crate::steam::official::read_favourites(&path) {
        Ok(entries) => entries,
        // No file is no favourites. The welcome's import button showed a new player
        // "i/o error at C:\Users\<name>\…\FavouriteServers.xml … (os error 2)" (row 14,
        // F7, approved).
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            crate::log_info!("app", "no official favourites file at {}", path.display());
            return Ok(ImportResult {
                missing: true,
                ..Default::default()
            });
        }
        Err(e) => return Err(AppError::io(&path, e)),
    };
    let mut result = ImportResult::default();
    let existing: std::collections::HashSet<String> = {
        let c = Arc::clone(&state.cache);
        tauri::async_runtime::spawn_blocking(move || {
            c.lock()
                .ok()
                .and_then(|c| c.favourites().ok())
                .unwrap_or_default()
                .into_iter()
                .map(|(id, _)| id)
                .collect()
        })
        .await
        .unwrap_or_default()
    };
    // Each row with whether it came from the XML alone, because the server did not
    // answer the probe.
    let mut new_rows: Vec<(ServerRow, bool)> = Vec::new();
    let mut targets = Vec::new();
    // Probe them together rather than one after another (D-188). Awaited in sequence,
    // a list with dead entries costs the sum of its timeouts: measured 21.9 s for 50
    // favourites, 75.9 s when none of them answered, against 1.63 s either way this
    // way. The client's own 128-permit semaphore and 400 pps pacer already bound the
    // burst, so the traffic shape is unchanged (D-037).
    let mut probes = tokio::task::JoinSet::new();
    let (entries, repeats) = crate::steam::official::distinct(entries);
    // The file listing a server twice counted it as imported twice (row 23).
    result.already += repeats;
    for e in entries {
        let id = ServerRow::id_for(&e.query_ip, e.query_port);
        if existing.contains(&id) {
            result.already += 1;
            continue;
        }
        let Ok(ip) = e.query_ip.parse::<std::net::IpAddr>() else {
            result.skipped += 1;
            continue;
        };
        let client = state.a2s.clone();
        probes.spawn(async move {
            let probed = probe_server(&client, ip, &[e.query_port], None).await;
            (e, id, probed)
        });
    }
    // Left out of every count before (row 23): counted for the page, and in the log.
    if result.skipped > 0 {
        crate::log_warn!(
            "app",
            "official favourites: {} entr{} with an address that is not an IP skipped",
            result.skipped,
            if result.skipped == 1 { "y" } else { "ies" }
        );
    }
    while let Some(joined) = probes.join_next().await {
        let Ok((e, id, probed)) = joined else {
            continue;
        };
        let (row, from_xml) = match probed {
            Some(r) => (r, false),
            None => {
                result.unreachable += 1;
                let xml_row = ServerRow {
                    id: id.clone(),
                    country: ServerRow::country_for(&e.query_ip),
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
                };
                (xml_row, true)
            }
        };
        if let Some(mut t) = Target::from_row(&row) {
            // The file's players and slots are of an age nobody knows: judged against a
            // fresh INFO, never against themselves, as the DZSA import's are (D-237). With
            // INFO lost, R13 held a head-count against the file's slots (row 28).
            if from_xml {
                t.reported_at = 0;
            }
            targets.push(t);
        }
        new_rows.push((row, from_xml));
        result.imported += 1;
    }
    if !new_rows.is_empty() {
        let c = Arc::clone(&state.cache);
        let n = new_rows.len();
        // The count was already tallied above, so swallowing these writes reported a
        // successful import that saved nothing (D-162). Fail loudly instead.
        type Stored = (Vec<ServerRow>, HashSet<String>);
        let stored = tauri::async_runtime::spawn_blocking(move || -> Result<Stored, String> {
            let mut c = c
                .lock()
                .map_err(|_| "the cache lock is poisoned".to_string())?;
            // A server that did not answer this one probe but is already in the list
            // keeps the row the list has. The XML's copy is however old the official
            // launcher's file is — name, version, no description, 0 players, 0 ms — and
            // writing it over a fresh Steam row left a false version warning in the join
            // plan that no later check repaired (D-239).
            let mut shown = Vec::with_capacity(new_rows.len());
            let mut to_store = Vec::with_capacity(new_rows.len());
            for (row, from_xml) in new_rows {
                if from_xml {
                    if let Some(cached) = c.get(&row.id).map_err(|e| e.to_string())? {
                        // Sent without Steam's word on it: on `servers:batch` a row that
                        // says Steam listed it populated counts as listed by this refresh,
                        // and kept a vouch the cache withdrew (D-271, D-281).
                        shown.push(ServerRow {
                            steam_empty: None,
                            ..cached
                        });
                        continue;
                    }
                }
                to_store.push(row.clone());
                shown.push(row);
            }
            c.upsert(&to_store).map_err(|e| e.to_string())?;
            // One transaction and one checkpoint for the whole import: per favourite
            // it measured 111.8 ms for 50 against 6.7 ms this way (D-175).
            let ids: Vec<String> = shown.iter().map(|r| r.id.clone()).collect();
            c.favourites_set_many(&ids).map_err(|e| e.to_string())?;
            // The upsert keeps a stored verdict and the probed rows carry none, so
            // the checks below take a standing "synthetic" from here (D-268).
            let synthetic = c.synthetic_ids().unwrap_or_default();
            Ok((shown, synthetic))
        })
        .await;
        let (shown, synthetic) = match stored {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return Err(import_failed(n, &e)),
            Err(e) => return Err(import_failed(n, &e.to_string())),
        };
        for t in &mut targets {
            t.was_synthetic |= synthetic.contains(&t.id);
        }
        // In Steam's batch size, like the DZSA import: a long favourites file sent as
        // one event went over docs/05 §4's ~200 KB from about 360 rows (D-287).
        for chunk in shown.chunks(crate::steam::sdk::BATCH_MAX_ROWS) {
            send_rows(&app, "batch", chunk);
        }
        tauri::async_runtime::spawn(run_verification(
            app.clone(),
            Arc::clone(&state.cache),
            state.a2s.clone(),
            targets,
            false,
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{details_queries, mod_list_of};
    use crate::a2s::{self, Client};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::net::UdpSocket;

    /// A server on 127.0.0.1 answering INFO and RULES at once and PLAYER after
    /// `player_ms`, with the challenge A2S asks for first.
    async fn slow_player_server(player_ms: u64) -> std::net::SocketAddr {
        let info: &'static [u8] = include_bytes!("../tests/fixtures/a2s/kingofgames.info.bin");
        let rules: &'static [u8] = include_bytes!("../tests/fixtures/a2s/kingofgames.rules.0.bin");
        let player: &'static [u8] = include_bytes!("../tests/fixtures/a2s/kingofgames.player.bin");
        let sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let addr = sock.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 1400];
            loop {
                let Ok((n, from)) = sock.recv_from(&mut buf).await else {
                    return;
                };
                let kind = buf.get(4).copied();
                if buf[..n].ends_with(&[0xFF; 4]) {
                    let _ = sock
                        .send_to(&[0xFF, 0xFF, 0xFF, 0xFF, 0x41, 1, 2, 3, 4], from)
                        .await;
                    continue;
                }
                let (reply, delay) = match kind {
                    Some(0x54) => (info, 0),
                    Some(0x56) => (rules, 0),
                    Some(0x55) => (player, player_ms),
                    _ => continue,
                };
                let sock = Arc::clone(&sock);
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    let _ = sock.send_to(reply, from).await;
                });
            }
        });
        addr
    }

    /// A server on 127.0.0.1 that drops everything for its first `silent_ms`, as a line
    /// busy with a start's passes did, then answers RULES, with the challenge first.
    async fn rules_server_silent_for(silent_ms: u64) -> std::net::SocketAddr {
        let rules: &'static [u8] = include_bytes!("../tests/fixtures/a2s/kingofgames.rules.0.bin");
        let sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let addr = sock.local_addr().unwrap();
        let t0 = std::time::Instant::now();
        tokio::spawn(async move {
            let mut buf = [0u8; 1400];
            loop {
                let Ok((n, from)) = sock.recv_from(&mut buf).await else {
                    return;
                };
                if t0.elapsed() < Duration::from_millis(silent_ms) {
                    continue;
                }
                if buf[..n].ends_with(&[0xFF; 4]) {
                    let _ = sock
                        .send_to(&[0xFF, 0xFF, 0xFF, 0xFF, 0x41, 1, 2, 3, 4], from)
                        .await;
                } else if buf.get(4) == Some(&0x56) {
                    let _ = sock.send_to(rules, from).await;
                }
            }
        });
        addr
    }

    /// D-329: a mod list the scan's pace lost gets a second look after the pause, and
    /// only a server that fails twice is recorded as failing.
    #[tokio::test]
    async fn a_lost_mod_list_gets_a_second_look() {
        let fast = Client::new(8)
            .with_rate(0)
            .with_timeout(Duration::from_millis(200))
            .with_retries(0);
        let busy = rules_server_silent_for(800).await;
        assert!(
            super::read_mod_list(&fast, busy).await.is_none(),
            "lost in the burst"
        );
        let (read, again) = super::second_look(
            &fast,
            vec![("busy".into(), busy)],
            Duration::from_millis(800),
        )
        .await;
        assert_eq!(read.len(), 1);
        assert!(!read[0].1.is_empty(), "the list itself");
        assert!(again.is_empty());
        let dead = rules_server_silent_for(60_000).await;
        let (read, again) =
            super::second_look(&fast, vec![("dead".into(), dead)], Duration::ZERO).await;
        assert!(read.is_empty());
        assert_eq!(again, vec!["dead".to_string()]);
    }

    /// Row 26: a PLAYER reply the first look lost gets the patient second look, while
    /// INFO answered; the pane published "unverifiable" from the first alone.
    #[tokio::test]
    async fn a_lost_player_reply_gets_a_second_look() {
        let addr = slow_player_server(1500).await;
        let client = Client::new(8)
            .with_timeout(Duration::from_millis(1000))
            .with_retries(0);
        let (info, _rules, players) = details_queries(&client, addr).await;
        assert!(info.is_ok(), "INFO answered at once");
        assert!(players.is_ok(), "the 2.5 s look waits for the 1.5 s answer");
    }

    /// Row 26: a RULES reply without its DayZ part is no mod list, as a failed read is not.
    #[test]
    fn a_reply_without_its_dayz_part_is_no_mod_list() {
        let bytes = include_bytes!("../tests/fixtures/a2s/kingofgames.rules.0.bin");
        let a2s::packet::Datagram::Single(payload) = a2s::packet::classify(bytes).unwrap() else {
            panic!("single")
        };
        let rules = a2s::rules::parse(payload).unwrap();
        assert!(rules.dayz.is_some());
        let reply = |value: a2s::Rules| -> a2s::A2sResult<a2s::Reply<a2s::Rules>> {
            Ok(a2s::Reply {
                value,
                rtt: Duration::from_millis(20),
            })
        };
        assert!(mod_list_of(&reply(rules.clone())).is_ok());
        let bare = a2s::Rules {
            dayz: None,
            ..rules
        };
        assert_eq!(
            mod_list_of(&reply(bare)),
            Err("the reply had no DayZ part".to_string())
        );
        assert!(mod_list_of(&Err(a2s::A2sError::Timeout)).is_err());
    }

    use super::{population_samples, scanned_list, ScannedList, ScannedMod};
    use crate::browser::verify::{Verdict, Verification};

    /// Row 26 (approved): an inflated check leaves its real count as a sample, without
    /// its claim's queue; a claim, a fabricated list or no answer leaves none.
    #[test]
    fn counted_checks_leave_population_samples() {
        let check = |id: &str, verdict: Verdict, verified: Option<i32>| Verification {
            id: id.into(),
            verdict,
            reported: 60,
            verified,
            max_players: 60,
            ping_ms: Some(30),
            player_rtt_ms: Some(31),
            keywords: None,
            tags: Some(crate::a2s::DayzTags::parse("lqs3")),
            verified_at: 1_790_000_000,
            reason: String::new(),
            facts: None,
        };
        let samples = population_samples(&[
            check("a", Verdict::Verified, Some(58)),
            check("b", Verdict::Inflated, Some(12)),
            check("c", Verdict::Synthetic, Some(60)),
            check("d", Verdict::Unverifiable, None),
            check("e", Verdict::Offline, None),
        ]);
        assert_eq!(
            samples,
            vec![
                ("a".to_string(), 1_790_000_000, 58, 3),
                ("b".to_string(), 1_790_000_000, 12, 0)
            ]
        );
    }

    /// Row 26 (approved): the scan's list reaches the pane with its age in words and in
    /// the shape the pane reads.
    #[test]
    fn a_scanned_list_says_how_old_it_is() {
        let now = 1_790_000_000;
        let list = scanned_list((now - 2 * 3600, vec![(1559212036, "CF".into())]), now);
        assert_eq!(
            list,
            ScannedList {
                age: "2 hours ago".into(),
                mods: vec![ScannedMod {
                    workshop_id: 1559212036,
                    name: "CF".into()
                }]
            }
        );
        assert_eq!(
            serde_json::to_value(&list).unwrap(),
            serde_json::json!({ "age": "2 hours ago", "mods": [{ "workshopId": 1559212036u64, "name": "CF" }] })
        );
    }

    use super::is_web_address;

    /// Row 27: `open_link` takes web addresses and nothing a shell reads as more.
    #[test]
    fn only_web_addresses_are_opened() {
        assert!(is_web_address(
            "https://steamcommunity.com/sharedfiles/filedetails/?id=1559212036"
        ));
        assert!(is_web_address("http://www.pcgamer.com/x"));
        assert!(is_web_address(
            "HTTPS://store.steampowered.com/news/app/221100"
        ));
        for bad in [
            "https://",
            "file:///C:/Windows/System32/calc.exe",
            "mailto:someone@example.com",
            r"C:\Windows\notepad.exe",
            "https://example.com/a b",
            "https://example.com/\"--x",
            "https://example.com/\u{7}",
            "steam://run/221100",
        ] {
            assert!(!is_web_address(bad), "{bad}");
        }
        assert!(!is_web_address(&format!(
            "https://example.com/{}",
            "a".repeat(2100)
        )));
    }

    use super::fallback_query_ports;

    /// Row 23: a caller that knows the game port has it tried as well, the typed port is
    /// not asked twice, and a typed address alone keeps the old candidates.
    #[test]
    fn fallback_ports_look_for_the_known_game_port() {
        let (gp, ports) = fallback_query_ports(2303, Some(2302));
        assert_eq!(gp, 2302);
        assert_eq!(ports[0], 2302);
        assert!(!ports.contains(&2303), "{ports:?}");
        assert!(ports.contains(&27016));
        let (gp, ports) = fallback_query_ports(2302, None);
        assert_eq!(gp, 2302);
        assert_eq!(ports, super::candidate_query_ports(2302));
        let (gp, ports) = fallback_query_ports(2302, Some(2302));
        assert_eq!(gp, 2302);
        assert!(!ports.contains(&2302), "{ports:?}");
    }

    use super::version_order;
    use std::cmp::Ordering;

    /// The join's version warning names the side that is behind (D-296).
    #[test]
    fn versions_order_by_their_numbers() {
        assert_eq!(
            version_order("1.29.163709", "1.29.163709"),
            Some(Ordering::Equal)
        );
        assert_eq!(
            version_order("1.28.161464", "1.29.163709"),
            Some(Ordering::Less)
        );
        assert_eq!(
            version_order("1.29.163709", "1.28.161464"),
            Some(Ordering::Greater)
        );
        // By number, not by text: 9 is older than 10.
        assert_eq!(version_order("1.9.1", "1.10.1"), Some(Ordering::Less));
        assert_eq!(version_order("1.29", "1.29.163709"), Some(Ordering::Less));
        assert_eq!(version_order("1.29.x", "1.29.163709"), None);
        assert_eq!(version_order("", "1.29.163709"), None);
    }
}
