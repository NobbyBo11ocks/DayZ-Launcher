//! Tauri IPC surface. Every command that touches files, the registry, the network
//! or the database runs off the UI thread (docs/05 §4).

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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
}

/// Static build information for the Settings page (the Diagnostics view went in D-168) and the M0 smoke test.
#[tauri::command]
pub fn app_info() -> AppInfo {
    #[cfg(debug_assertions)]
    eprintln!("[ipc] app_info called from the webview");
    AppInfo {
        name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        tauri: tauri::VERSION,
        os: std::env::consts::OS,
        elevated: crate::proc::current_is_elevated(),
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
/// lastUpdateCheckMs) into the settings file and returns the stored result (D-070).
/// Off the main thread for the same reason as `settings_set` (D-239).
#[tauri::command(async)]
pub fn ui_prefs_set(state: State<'_, AppState>, patch: serde_json::Value) -> AppResult<UiPrefs> {
    state
        .settings
        .patch_ui(patch)
        .map_err(|e| AppError::Internal(format!("ui prefs: {e}")))
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
        let c = cache
            .lock()
            .map_err(|_| AppError::Internal("cache lock poisoned".into()))?;
        let rows = c
            .load_all()
            .map_err(|e| AppError::Internal(format!("cache: {e}")))?;
        let last_refresh = c
            .get_meta("last_refresh")
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok());
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
    state
        .steam
        .refresh(parts, force.unwrap_or(false))
        .map_err(AppError::Internal)
}

/// Loads the DZSA list as a fallback when Steam is unavailable (D-089). Rows stream as
/// `servers:batch`, the mod lists go into the cache, a `servers:done` with source
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
    for chunk in rows.chunks(500) {
        let batch: Vec<ServerRow> = chunk.iter().map(|r| r.row.clone()).collect();
        for r in &batch {
            if r.players > 0 {
                if let Some(mut t) = Target::from_row(r) {
                    // DZSA's own count, of an age nobody knows: judged against a fresh
                    // INFO, not against itself (D-236).
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
                if let Err(e) = c.upsert_keeping_measured(&for_db) {
                    crate::log_error!(
                        "cache",
                        "DZSA upsert of {} row(s) failed: {e}",
                        for_db.len()
                    );
                    return Err(format!("could not store the server list: {e}"));
                }
                if let Err(e) = c.replace_server_mods_many(&mods, now) {
                    crate::log_error!(
                        "cache",
                        "DZSA mod lists for {} row(s) failed: {e}",
                        mods.len()
                    );
                    return Err(format!("could not store the mod lists: {e}"));
                }
                Ok(())
            } else {
                Err("the server cache is unavailable".to_string())
            }
        })
        .await
        .map_err(|e| AppError::Internal(format!("cache task failed: {e}")))?;
        wrote.map_err(AppError::Internal)?;
        // Its own event: the store keeps measured values over these placeholders, as
        // `upsert_keeping_measured` does, and every other batch is a measurement (D-271).
        let _ = app.emit("servers:dzsa-batch", &batch);
    }
    // `last_refresh` deliberately not written here. It seeds the Steam worker's 60 s
    // throttle across restarts (lib.rs), so a DZSA import used to make the *next*
    // start skip its automatic Steam refresh without saying so - and D-160 already
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
    let _ = app.emit("servers:done", &done);
    let _ = app.emit(
        "servers:mods-done",
        &ModScanSummary {
            total: with_mods,
            scanned: with_mods,
            failed: 0,
            elapsed_ms: done.elapsed_ms,
        },
    );
    #[cfg(debug_assertions)]
    eprintln!(
        "[dzsa] imported {n} servers ({with_mods} with mods) in {} ms; verifying {}",
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
                let _ = app.emit(
                    "servers:verify-done",
                    &VerifySummary {
                        skipped: true,
                        ..Default::default()
                    },
                );
            }
        }
    } else {
        // Nothing to verify still has to close the pass (D-160): the UI sets
        // "verifying" as soon as the fallback is asked for, and only this event
        // clears it. An empty or all-empty DZSA list left it spinning for ever.
        let _ = app.emit("servers:verify-done", &VerifySummary::default());
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

/// Verifies targets as a stream (D-193); only the events are batched: emits `servers:verified` (Vec<Verification>) per
/// chunk and persists. With `announce`, also emits `servers:verify-done`
/// (VerifySummary); only the automatic post-refresh pass announces, so on-demand
/// checks of visible rows never masquerade as the full pass.
/// The verification pass's own permit pool, kept at the interactive client's size —
/// the pass is not being made gentler, it is being taken out of the queue the join
/// dialog and the details pane share (D-193).
pub const VERIFY_CONCURRENCY: usize = 128;
/// Rows per `servers:verified` event. docs/05 §4 caps an event at ~200 KB and a row
/// measures 558 B, so 128 rows is ~71 KB — small enough that the front end's derived
/// chain shrugs at it, large enough that a pass is not thousands of events.
const VERIFY_FLUSH_ROWS: usize = 128;
/// …and a partial batch goes out anyway once this long has passed, so the last few
/// stragglers of a pass are not held back by the ones that will never answer.
const VERIFY_FLUSH_EVERY: std::time::Duration = std::time::Duration::from_millis(250);
/// How long after a pass the servers still offline at its end are read once more:
/// long enough for a scheduled restart to come back (D-268).
const OFFLINE_REREAD_AFTER: std::time::Duration = std::time::Duration::from_secs(240);
/// The second look a server that answered nothing gets, before it is called offline:
/// a longer wait than the first, INFO included (D-160).
const PATIENT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(2500);

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
    // is over a minute old is re-read either way (`verify_one`, D-236).
    let with_info = !announce;
    // A standing R11 verdict survives a restart (D-236). Only the automatic pass needs
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
    // Standing R11 verdicts this pass could not compare (D-236): they get a second
    // PLAYER read once the minimum gap has passed, below.
    let mut standing: Vec<Target> = Vec::new();
    while let Some(v) = pending.next().await {
        if v.verdict == Verdict::Offline {
            if let Some(t) = by_id.get(&v.id) {
                offline.push(t.clone());
            }
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
        publish(&app, &cache, &mut latest, results).await;
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
            publish(&app, &cache, &mut latest, results).await;
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
    let still_offline: Vec<Target> = if announce {
        latest
            .iter()
            .filter(|(_, v)| **v == Verdict::Offline)
            .filter_map(|(id, _)| by_id.get(id).cloned())
            .collect()
    } else {
        Vec::new()
    };
    if !still_offline.is_empty() {
        let n = still_offline.len();
        let app = app.clone();
        let cache = Arc::clone(&cache);
        let patient = client.clone().with_timeout(PATIENT_TIMEOUT);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(OFFLINE_REREAD_AFTER).await;
            let results = verify::verify_many(&patient, still_offline, true).await;
            let back = results
                .iter()
                .filter(|v| v.verdict != Verdict::Offline)
                .count();
            let mut latest = HashMap::with_capacity(n);
            publish(&app, &cache, &mut latest, results).await;
            crate::log_info!(
                "verify",
                "{n} server(s) offline at the pass read again: {back} answered"
            );
        });
    }
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
        let _ = app.emit("servers:verify-done", &summary);
    }
    summary
}

/// Emits and persists one batch of verification results, recording the latest
/// verdict per server so a retry supersedes an earlier "offline". Verified
/// head-counts also become population samples (M6 sparkline).
async fn publish(
    app: &AppHandle,
    cache: &Arc<Mutex<Cache>>,
    latest: &mut HashMap<String, Verdict>,
    results: Vec<Verification>,
) {
    for v in &results {
        latest.insert(v.id.clone(), v.verdict);
    }
    let _ = app.emit("servers:verified", &results);
    let c = Arc::clone(cache);
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Ok(mut c) = c.lock() {
            if let Err(e) = c.apply_verifications(&results) {
                crate::log_error!(
                    "cache",
                    "apply_verifications of {} failed: {e}",
                    results.len()
                );
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
                    crate::log_error!(
                        "cache",
                        "population_add of {} sample(s) failed: {e}",
                        samples.len()
                    );
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
            .map_err(|_| AppError::Internal("cache lock poisoned".into()))?;
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

/// Mod lists are re-scanned when older than this.
const MOD_SCAN_MAX_AGE_SECS: i64 = 24 * 3600;

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
    use crate::browser::ServerMods;

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
    let _ = app.emit("servers:mods-start", &summary);
    if targets.is_empty() {
        if held_back > 0 {
            crate::log_info!(
                "mods",
                "scan: nothing to read, {held_back} held back after failing"
            );
        }
        let _ = app.emit("servers:mods-done", &summary);
        return summary;
    }
    // Gentler than the verification pass: RULES replies are up to ~5 KB each. Its own
    // permit pool, sized so that concurrency ÷ rate (64 ÷ 100 = 0.64 s) stays inside
    // the 1 s deadline — sharing `state.a2s`'s 128 permits put 1.28 s of queue in front
    // of the challenge leg and made the retry do 99 % of the work (D-193).
    let client = client.with_concurrency(64).with_rate(100).with_retries(1);
    for chunk in targets.chunks(200) {
        let mut set = tokio::task::JoinSet::new();
        for (id, addr) in chunk.iter().cloned() {
            let c = client.clone();
            set.spawn(async move {
                let mods = c.rules(addr).await.ok().map(|r| {
                    r.value.dayz.map_or_else(Vec::new, |d| {
                        // Each id once, where it first appears (D-265), id 0 included:
                        // the cache keeps one row per id, so a list sent with every
                        // server-side mod counted more in the Mods column than the same
                        // list read back after a restart (D-276).
                        let mut seen = HashSet::new();
                        d.mods
                            .into_iter()
                            .filter(|m| seen.insert(m.workshop_id))
                            .map(|m| (m.workshop_id, m.name))
                            .collect::<Vec<(u64, String)>>()
                    })
                });
                (id, mods)
            });
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
        // an event at ~200 KB (D-160).
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
        // The failed ids too: they have no list and wait before the next read (D-244),
        // so the "not scanned yet" count leaves them out rather than offering a scan
        // that will not ask them (D-276).
        let _ = app.emit("servers:mods", &(payload, names, &failed));
        let c = Arc::clone(&cache);
        let _ = tauri::async_runtime::spawn_blocking(move || {
            if let Ok(mut c) = c.lock() {
                // One transaction for the chunk, not one per server: measured
                // 43-51 ms against 8-10 ms for 200 servers (D-160).
                if let Err(e) = c.replace_server_mods_many(&batch, now) {
                    crate::log_error!(
                        "cache",
                        "server mods for {} row(s) failed: {e}",
                        batch.len()
                    );
                }
                // So the next pass waits on them instead of asking again (D-244).
                if let Err(e) = c.record_scan_failures(&failed, now) {
                    crate::log_warn!("cache", "failed mod reads not recorded: {e}");
                }
            }
        })
        .await;
    }
    summary.elapsed_ms = t0.elapsed().as_millis() as u64;
    if summary.total > 0 || held_back > 0 {
        crate::log_info!(
            "mods",
            "scan: {} server(s) read, {} failed, {} held back after failing, in {} ms",
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
    let _ = app.emit("servers:mods-done", &summary);
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
        .map_err(|e| AppError::Internal(format!("stale check failed: {e}")))
}

/// Stored mod lists and the mod catalogue for the browser's mod filter (D-080).
#[tauri::command]
pub async fn mods_index(state: State<'_, AppState>) -> AppResult<crate::browser::ModsIndex> {
    let cache = Arc::clone(&state.cache);
    tauri::async_runtime::spawn_blocking(move || {
        let c = cache
            .lock()
            .map_err(|_| AppError::Internal("cache lock poisoned".into()))?;
        c.mods_index()
            .map_err(|e| AppError::Internal(format!("cache: {e}")))
    })
    .await
    .map_err(|e| AppError::Internal(format!("mods index task failed: {e}")))?
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
    // thing the user saw was a watchdog clearing the flag ~2 min later (D-209).
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
        .map_err(|e| AppError::Internal(format!("unsubscribe task failed: {e}")))?
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
            .ok_or_else(|| AppError::Internal("Steam is not installed".into()))?;
        let libs = locate::libraries(&path)?;
        let game = locate::find_dayz(&libs)?.ok_or_else(|| {
            let gone = locate::unreachable_dayz_libraries(&libs);
            AppError::Internal(match gone.first() {
                Some(p) => format!(
                    "Steam has DayZ in {}, but that folder is not reachable; connect the drive and try again.",
                    p.display()
                ),
                None => "DayZ is not installed".into(),
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
    .map_err(|e| AppError::Internal(format!("junction task failed: {e}")))?
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
        let c = c
            .lock()
            .map_err(|_| AppError::Internal("cache lock poisoned".into()))?;
        let items = c
            .get_meta("news")
            .ok()
            .flatten()
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default();
        Ok(NewsCached { items })
    })
    .await
    .map_err(|e| AppError::Internal(format!("news task failed: {e}")))?
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
        .map_err(|e| AppError::Internal(format!("avatar task failed: {e}")))?
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

/// Where the log file lives, for "show me the folder".
#[tauri::command]
pub fn logs_path() -> Option<String> {
    crate::log::path().map(|p| p.to_string_lossy().into_owned())
}

/// The WebView's own diagnostics: unhandled errors, failed commands, view timings.
/// Frontend levels are clamped to the same four, and the target is prefixed so the
/// origin is never ambiguous in the file.
#[tauri::command]
pub fn log_ui(level: String, target: String, message: String) {
    let lvl = match level.as_str() {
        "error" => crate::log::Level::Error,
        "warn" => crate::log::Level::Warn,
        _ => crate::log::Level::Info,
    };
    // One entry is one line in launcher.log, so a message carrying newlines could
    // forge entries and make the log untrustworthy to read back (D-160).
    let flatten = |s: &str, n: usize| -> String {
        s.chars()
            .take(n)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect()
    };
    let target = format!("ui:{}", flatten(&target, 24));
    crate::log::write(lvl, &target, flatten(&message, 2000));
}

/// Steam friends with presence and, for those in DayZ, their server (D-092).
#[tauri::command]
pub async fn friends_list(state: State<'_, AppState>) -> AppResult<Vec<FriendInfo>> {
    let steam = state.steam.clone_handle();
    tauri::async_runtime::spawn_blocking(move || steam.friends())
        .await
        .map_err(|e| AppError::Internal(format!("friends task failed: {e}")))?
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
    let addr: SocketAddr = id
        .parse()
        .map_err(|_| AppError::Internal(format!("bad server id {id}")))?;
    let reply = state
        .a2s
        .info(addr)
        .await
        .map_err(|e| AppError::Internal(format!("{addr}: {e}")))?;
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
    let addr: SocketAddr = id
        .parse()
        .map_err(|_| AppError::Internal(format!("bad server id {id}")))?;
    let client = state.a2s.clone();
    let cache = Arc::clone(&state.cache);
    let cached = cached_row(&cache, &id).await;
    let (reported, max_players) = cached
        .as_ref()
        .map_or((0, 0), |r| (r.players, r.max_players));
    let was_synthetic = cached
        .as_ref()
        .is_some_and(|r| r.verdict.as_deref() == Some("synthetic"));

    let (mut info, mut rules, mut players) =
        tokio::join!(client.info(addr), client.rules(addr), client.players(addr));
    // A server that answered nothing gets the second, patient look every other check
    // gives one (the pass and the visible rows, D-160). Without it one timed-out click
    // published "offline" and hid a server verified a minute before, and a hidden row
    // is checked again only when something else looks at it (D-272).
    if info.is_err() && players.is_err() {
        let patient = client.clone().with_timeout(PATIENT_TIMEOUT);
        let rules_failed = rules.is_err();
        let (i, r, p) = tokio::join!(
            patient.info(addr),
            async {
                if rules_failed {
                    Some(patient.rules(addr).await)
                } else {
                    None
                }
            },
            patient.players(addr)
        );
        info = i;
        players = p;
        if let Some(r) = r {
            rules = r;
        }
    }
    // With INFO lost, a cached count older than a minute is no count, as in
    // `verify_one` (D-245): judged against a fresh PLAYER, an hour-old 60 read as
    // "advertises 60; 20 actually connected" and hid the row the user had open (D-268).
    let stale = cached.as_ref().is_none_or(|r| {
        ServerRow::now_unix().saturating_sub(r.last_seen) > verify::INFO_FRESH_SECS
    });
    let judged_against = if stale && info.is_err() { -1 } else { reported };
    // With R11, like every other check: `judge` alone published "verified" over a
    // standing "synthetic" verdict each time the row was opened (D-236).
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
        player_rtt_ms: None,
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

/// Everything the join dialog needs: required mods (live RULES), local state, Steam
/// titles/sizes for anything missing, and preconditions.
#[tauri::command]
pub async fn join_plan(state: State<'_, AppState>, id: String) -> AppResult<JoinPlan> {
    let row = cached_row(&state.cache, &id)
        .await
        .ok_or_else(|| AppError::Internal(format!("unknown server {id}")))?;
    let addr: SocketAddr = id
        .parse()
        .map_err(|_| AppError::Internal(format!("bad server id {id}")))?;
    // INFO beside RULES: the cached row's game port, password flag and version are
    // only as fresh as the last listing, and verification never updates them, so a
    // server that moved its game port or added a password was planned — and joined —
    // from stale facts (D-265).
    let (rules, info) = tokio::join!(state.a2s.rules(addr), state.a2s.info(addr));
    let info = info.ok().map(|r| r.value);
    let diag = tauri::async_runtime::spawn_blocking(diagnostics::collect)
        .await
        .map_err(|e| AppError::Internal(format!("diagnostics task failed: {e}")))??;
    // The list this plan read becomes the cached one: a launch whose own RULES read
    // drops falls back to the cache, which could be a day-old scan rather than the
    // list the player was shown seconds earlier (D-265).
    if let Ok(r) = &rules {
        if r.value.dayz.is_some() {
            let list = vec![(id.clone(), r.value.required_mods())];
            let c = Arc::clone(&state.cache);
            let now = ServerRow::now_unix();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                if let Ok(mut c) = c.lock() {
                    if let Err(e) = c.replace_server_mods_many(&list, now) {
                        crate::log_warn!("cache", "the plan's mod list was not stored: {e}");
                    }
                }
            })
            .await;
        }
    }
    let server_version = info
        .as_ref()
        .map_or_else(|| row.version.clone(), |i| i.version.clone());
    let password_required = info.as_ref().map_or(row.password, |i| i.password);
    let game_port = info
        .as_ref()
        .and_then(|i| i.game_port)
        .unwrap_or(row.game_port);

    let mut warnings = Vec::new();
    let required: Vec<(u64, String)> = match &rules {
        // Published mods only, each once (D-221, D-265).
        Ok(r) => r.value.required_mods(),
        // A server that will not answer RULES is not a vanilla server. Launching
        // without its mods is a kick on arrival, so fall back to the list the mod scan
        // recorded and say how old it is rather than inventing an empty one (D-209).
        Err(e) => match cached_mods(&state.cache, &id).await {
            Some((scanned_at, mods)) if !mods.is_empty() => {
                let age = ServerRow::now_unix().saturating_sub(scanned_at);
                warnings.push(format!(
                    "The server did not answer the mod query ({e}); using the list from the last scan, {}.",
                    humanise_age(age)
                ));
                mods
            }
            Some(_) => Vec::new(),
            None => {
                warnings.push(format!(
                    "Could not read the server's mod list ({e}) and it has never been scanned; launching without mods."
                ));
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
                Ok(Ok(details)) => {
                    let by_id: HashMap<u64, ItemDetails> =
                        details.into_iter().map(|d| (d.id, d)).collect();
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
    let version_mismatch = local_version
        .as_deref()
        .is_some_and(|v| v != server_version);
    if version_mismatch {
        warnings.push(format!(
            "Server runs {} but your DayZ is {}; the server will reject the connection until Steam updates the game.",
            server_version,
            local_version.clone().unwrap_or_default()
        ));
    }
    if !diag.steam.running {
        warnings.push("Steam is not running; DayZ cannot start without it.".into());
    }
    // Re-checked here, not taken from start-up (D-159): the launcher often starts before
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
    // for themselves, so the dialog showed a disabled button and no reason (D-160).
    let game_found = diag.dayz.is_some();
    let battleye_present = diag.dayz.as_ref().is_some_and(|g| g.has_battleye_exe);
    if !game_found {
        warnings.push(
            "DayZ was not found in any Steam library; install it, or check that its drive is connected."
                .into(),
        );
    } else if !battleye_present {
        warnings.push(
            "DayZ_BE.exe is missing from the game folder; verify the game files in Steam.".into(),
        );
    }
    // Whatever else the machine check turned up (no library folders, an unreadable
    // Workshop folder, nobody signed in): the user could only see these in
    // Diagnostics before, while the join in front of them quietly failed.
    warnings.extend(diag.warnings.iter().cloned());

    let settings = state.settings.get();
    let profile_name = if settings.profile_name.trim().is_empty() {
        worker_persona(&state).unwrap_or_default()
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

/// Builds junctions and the argument line, then starts `DayZ_BE.exe`. Emits
/// `launch:started` (Launched) now and `launch:exited` ({pid, code}) later.
#[tauri::command]
pub async fn launch_game(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    password: Option<String>,
    profile: Option<String>,
) -> AppResult<Launched> {
    let row = cached_row(&state.cache, &id)
        .await
        .ok_or_else(|| AppError::Internal(format!("unknown server {id}")))?;
    let addr: SocketAddr = id
        .parse()
        .map_err(|_| AppError::Internal(format!("bad server id {id}")))?;
    // INFO beside RULES for the game port, as in the plan (D-265).
    let (rules, info) = tokio::join!(state.a2s.rules(addr), state.a2s.info(addr));
    let game_port = info
        .ok()
        .and_then(|r| r.value.game_port)
        .unwrap_or(row.game_port);
    let required: Vec<(u64, String)> = match &rules {
        // Published mods only, each once (D-221, D-265).
        Ok(r) => r.value.required_mods(),
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
                    return Err(AppError::Internal(format!(
                        "Could not read {}'s mod list ({e}), and nothing is cached for it. \
                         Starting without mods would be rejected by the server, so nothing was started — try again in a moment.",
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
    let persona = worker_persona(&state);

    let row_for_spec = row.clone();
    let (game_dir, links, spec) = tauri::async_runtime::spawn_blocking(move || {
        let row = row_for_spec;
        let steam = registry::detect();
        if !steam.running {
            return Err(AppError::Internal("Steam is not running".into()));
        }
        let path = steam
            .path
            .ok_or_else(|| AppError::Internal("Steam is not installed".into()))?;
        let libs = locate::libraries(&path)?;
        let game = locate::find_dayz(&libs)?
            .ok_or_else(|| AppError::Internal("DayZ is not installed".into()))?;
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
                AppError::Internal(format!(
                    "mod {name} ({id}) is not installed; sync mods first"
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
            // S-41; D-008 corrected by D-265).
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
    .map_err(|e| AppError::Internal(format!("launch task failed: {e}")))??;

    let args = launch::build_args(&spec);
    // Spawn FIRST, then step aside (D-119, corrected in D-151): a child started by a
    // BELOW_NORMAL parent inherits that class, so lowering the launcher before the spawn
    // handed DayZ itself a below-normal priority — the opposite of the intent.
    // A second DayZ would fight the first for the game's own single-instance lock,
    // and its immediate exit used to restore the launcher's priority while the real
    // game was still playing (D-119, D-160).
    if let Some(pid) = crate::steam::registry::process::find_named("DayZ_x64.exe")
        .or_else(|| crate::steam::registry::process::find_named("DayZ_BE.exe"))
    {
        crate::log_warn!("launch", "DayZ is already running as pid {pid}");
        return Err(AppError::Internal(
            "DayZ is already running; close it before joining another server.".into(),
        ));
    }
    let (mut child, launched) = match launch::spawn(&game_dir, &args) {
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
        let row_for_history = row.clone();
        let mods = links.len();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            if let Ok(c) = c.lock() {
                // Q22: a join that is not recorded is exactly the reported symptom,
                // so the failure has to leave a trace (D-160).
                if let Err(e) = c.history_add(&row_for_history, mods) {
                    crate::log_error!("cache", "history_add failed: {e}");
                }
            }
        })
        .await;
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
    .map_err(|e| AppError::Internal(format!("process check failed: {e}")))
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
        let c = c
            .lock()
            .map_err(|_| AppError::Internal("cache lock poisoned".into()))?;
        let list = c
            .favourites()
            .map_err(|e| AppError::Internal(format!("cache: {e}")))?;
        Ok(list
            .into_iter()
            .map(|(id, added_at)| Favourite { id, added_at })
            .collect())
    })
    .await
    .map_err(|e| AppError::Internal(format!("cache task failed: {e}")))?
}

#[tauri::command]
pub async fn favourite_set(state: State<'_, AppState>, id: String, on: bool) -> AppResult<()> {
    let c = Arc::clone(&state.cache);
    tauri::async_runtime::spawn_blocking(move || {
        let c = c
            .lock()
            .map_err(|_| AppError::Internal("cache lock poisoned".into()))?;
        c.favourite_set(&id, on)
            .map_err(|e| AppError::Internal(format!("cache: {e}")))
    })
    .await
    .map_err(|e| AppError::Internal(format!("cache task failed: {e}")))?
}

#[tauri::command]
pub async fn history_list(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> AppResult<Vec<HistoryEntry>> {
    let c = Arc::clone(&state.cache);
    let limit = limit.unwrap_or(50).min(500);
    tauri::async_runtime::spawn_blocking(move || {
        let c = c
            .lock()
            .map_err(|_| AppError::Internal("cache lock poisoned".into()))?;
        c.history(limit)
            .map_err(|e| AppError::Internal(format!("cache: {e}")))
    })
    .await
    .map_err(|e| AppError::Internal(format!("cache task failed: {e}")))?
}

/// Removes every join from the history (the Recent view's confirmed "Clear list",
/// D-130). Returns how many rows went.
#[tauri::command]
pub async fn history_clear(state: State<'_, AppState>) -> AppResult<usize> {
    let c = Arc::clone(&state.cache);
    tauri::async_runtime::spawn_blocking(move || {
        let c = c
            .lock()
            .map_err(|_| AppError::Internal("cache lock poisoned".into()))?;
        c.history_clear()
            .map_err(|e| AppError::Internal(format!("cache: {e}")))
    })
    .await
    .map_err(|e| AppError::Internal(format!("cache task failed: {e}")))?
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
        let c = c
            .lock()
            .map_err(|_| AppError::Internal("cache lock poisoned".into()))?;
        c.population(&id, since)
            .map_err(|e| AppError::Internal(format!("cache: {e}")))
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
            let dayz = reply.value.app_id == 0 || reply.value.app_id == crate::steam::DAYZ_APP_ID;
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
) -> AppResult<ServerRow> {
    let text = address.trim().trim_start_matches("steam://connect/");
    let (host, port) = match text.rsplit_once(':') {
        Some((h, p)) => (
            h.trim_matches(|c| c == '[' || c == ']'),
            p.parse::<u16>()
                .map_err(|_| AppError::Internal(format!("bad port in {address}")))?,
        ),
        None => (text, 2302),
    };
    if host.is_empty() {
        return Err(AppError::Internal(
            "enter an address like 51.81.8.81:2402".into(),
        ));
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
    crate::log_info!("join", "direct connect to {address}");
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
    let id = row.id.clone();
    // The write was discarded and not even logged, while the command still returned
    // the row and emitted `servers:batch` - so on an unwritable cache the server
    // appeared in the grid and then `join_plan` and `launch_game`, which both start
    // from `cached_row(..).ok_or("unknown server")`, refused it with nothing in the
    // log to explain why. Every other cache write on this path reports (D-220).
    // The upsert keeps a stored verdict; the probe's row has none, and a check built
    // from it alone published "verified" over a standing "synthetic" whenever R11 had
    // no sample yet, as the details pane once did (D-236, D-268).
    let was_synthetic = tauri::async_runtime::spawn_blocking(move || match c.lock() {
        Ok(mut c) => {
            c.upsert(&stored)
                .map_err(|e| format!("could not store the server: {e}"))?;
            Ok(c.get(&id)
                .ok()
                .flatten()
                .is_some_and(|r| r.verdict.as_deref() == Some("synthetic")))
        }
        Err(_) => Err("the server cache is unavailable".to_string()),
    })
    .await
    .map_err(|e| AppError::Internal(format!("cache task failed: {e}")))?
    .map_err(AppError::Internal)?;
    let _ = app.emit("servers:batch", &vec![row.clone()]);
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
    pub total: usize,
    pub imported: usize,
    pub already: usize,
    /// Added from the XML data only because the server did not answer A2S now.
    pub unreachable: usize,
    pub path: String,
}

fn import_failed(n: usize, e: &str) -> AppError {
    crate::log_error!("cache", "favourite import of {n} row(s) failed: {e}");
    AppError::Internal(format!(
        "read {n} favourite(s) but could not save them: {e}"
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
    let entries =
        crate::steam::official::read_favourites(&path).map_err(|e| AppError::io(&path, e))?;
    let mut result = ImportResult {
        total: entries.len(),
        path: path.to_string_lossy().into_owned(),
        ..Default::default()
    };
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
    for e in entries {
        let id = ServerRow::id_for(&e.query_ip, e.query_port);
        if existing.contains(&id) {
            result.already += 1;
            continue;
        }
        let Ok(ip) = e.query_ip.parse::<std::net::IpAddr>() else {
            continue;
        };
        let client = state.a2s.clone();
        probes.spawn(async move {
            let probed = probe_server(&client, ip, &[e.query_port], None).await;
            (e, id, probed)
        });
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
        if let Some(t) = Target::from_row(&row) {
            targets.push(t);
        }
        new_rows.push((row, from_xml));
        result.imported += 1;
    }
    if !new_rows.is_empty() {
        let c = Arc::clone(&state.cache);
        let n = new_rows.len();
        // The count was already tallied above, so swallowing these writes reported a
        // successful import that saved nothing (D-160). Fail loudly instead.
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
                        shown.push(cached);
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
        let _ = app.emit("servers:batch", &shown);
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
