//! DayZ Launcher core. Module layout follows docs/05-architecture-and-optimisation.md §2.
//! Windows-only desktop target; no mobile entry point.

pub mod a2s;
pub mod browser;
mod commands;
pub mod error;
pub mod geoip;
pub mod launch;
pub mod perf;
pub mod settings;
pub mod steam;

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use tauri::{Emitter, Manager};

use browser::verify::Target;
use browser::Cache;
use commands::AppState;
use settings::SettingsStore;
use steam::sdk::{SteamEvent, SteamWorker};

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Milliseconds since `run()` began; used by debug traces to measure cold start.
pub fn uptime_ms() -> u128 {
    STARTED.get().map_or(0, |t| t.elapsed().as_millis())
}

/// Rows not seen for this long are dropped from the cache after a completed refresh.
const CACHE_MAX_AGE_SECS: i64 = 30 * 24 * 3600;
/// Population samples older than this are dropped (the sparkline shows 72 h).
const POPULATION_MAX_AGE_SECS: i64 = 7 * 24 * 3600;

pub fn run() {
    let _ = STARTED.set(Instant::now());
    tauri::Builder::default()
        // Must be the first plugin (its README): a second launch hands its arguments
        // to the running instance, which just comes to the front (D-079).
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let db_path = app.path().app_local_data_dir()?.join("cache.db");
            let cache = Arc::new(Mutex::new(Cache::open(&db_path)?));
            #[cfg(debug_assertions)]
            eprintln!("[setup] cache at {} ({} rows)", db_path.display(), cache.lock().map(|c| c.count().unwrap_or(0)).unwrap_or(0));
            let settings = SettingsStore::load(&app.path().app_config_dir()?.join("settings.json"));

            let last_refresh = cache
                .lock()
                .ok()
                .and_then(|c| c.get_meta("last_refresh").ok().flatten())
                .and_then(|v| v.parse::<i64>().ok());
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SteamEvent>();
            let steam = SteamWorker::spawn(tx, last_refresh, settings.get().steam_idle_timeout());
            let a2s = a2s::Client::default();
            app.manage(AppState {
                steam,
                cache: Arc::clone(&cache),
                a2s: a2s.clone(),
                settings,
            });

            // Forwards Steam-thread events to the WebView, persists batches, and
            // verifies populated servers (docs/11 R2–R5) once a refresh completes.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut populated: Vec<Target> = Vec::new();
                while let Some(ev) = rx.recv().await {
                    match ev {
                        SteamEvent::Status(s) => {
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "[steam] status: initialized={} refreshing={} error={:?} at {} ms",
                                s.initialized,
                                s.refreshing,
                                s.error,
                                uptime_ms()
                            );
                            let _ = handle.emit("steam:status", &s);
                        }
                        SteamEvent::Batch(rows) => {
                            populated.extend(rows.iter().filter(|r| r.steam_empty == Some(false)).filter_map(Target::from_row));
                            let _ = handle.emit("servers:batch", &rows);
                            let c = Arc::clone(&cache);
                            let _ = tauri::async_runtime::spawn_blocking(move || {
                                if let Ok(mut c) = c.lock() {
                                    if let Err(e) = c.upsert(&rows) {
                                        eprintln!("[cache] upsert failed: {e}");
                                    }
                                }
                            })
                            .await;
                        }
                        SteamEvent::Done(d) => {
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "[steam] refresh done: total={} responded={} failed={} inflated={} capped={} stopped_early={} in {} ms; partitions={:?}",
                                d.total,
                                d.responded,
                                d.failed,
                                d.inflated,
                                d.capped,
                                d.stopped_early,
                                d.elapsed_ms,
                                d.partitions
                                    .iter()
                                    .map(|p| format!(
                                        "{:?}: {} total / {} ok / {} failed / {} inflated / {} ms / {}",
                                        p.filters, p.total, p.responded, p.failed, p.inflated, p.elapsed_ms, p.response
                                    ))
                                    .collect::<Vec<_>>()
                            );
                            let _ = handle.emit("servers:done", &d);
                            let c = Arc::clone(&cache);
                            let _ = tauri::async_runtime::spawn_blocking(move || {
                                if let Ok(c) = c.lock() {
                                    let _ = c.set_meta("last_refresh", &browser::ServerRow::now_unix().to_string());
                                    let _ = c.prune(CACHE_MAX_AGE_SECS);
                                    let _ = c.population_prune(POPULATION_MAX_AGE_SECS);
                                }
                            })
                            .await;
                            let targets = std::mem::take(&mut populated);
                            if !targets.is_empty() {
                                #[cfg(debug_assertions)]
                                eprintln!("[verify] start: {} populated servers", targets.len());
                                tauri::async_runtime::spawn(commands::run_verification(
                                    handle.clone(),
                                    Arc::clone(&cache),
                                    a2s.clone(),
                                    targets,
                                    true,
                                ));
                            }
                        }
                        SteamEvent::SyncProgress(p) => {
                            let _ = handle.emit("mods:progress", &p);
                        }
                        SteamEvent::SyncDone(d) => {
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "[mods] sync {} {}: {} item(s) in {} ms{}",
                                d.job,
                                if d.ok { "ok" } else { "failed" },
                                d.items.len(),
                                d.elapsed_ms,
                                d.error.as_deref().map(|e| format!(" ({e})")).unwrap_or_default()
                            );
                            let _ = handle.emit("mods:done", &d);
                        }
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::perf_first_paint,
            commands::perf_sample,
            commands::diagnostics,
            commands::diagnostics_export,
            commands::local_game_version,
            commands::steam_status,
            commands::settings_get,
            commands::settings_set,
            commands::ui_prefs_set,
            commands::servers_cached,
            commands::servers_refresh,
            commands::servers_verify,
            commands::server_details,
            commands::server_slots,
            commands::join_plan,
            commands::mods_sync,
            commands::mods_unsubscribe,
            commands::launch_game,
            commands::favourites_list,
            commands::favourite_set,
            commands::history_list,
            commands::population_history,
            commands::direct_connect,
            commands::import_official_favourites
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
