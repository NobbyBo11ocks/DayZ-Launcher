//! DayZ Launcher core. Module layout follows docs/05-architecture-and-optimisation.md §2.
//! Windows-only desktop target; no mobile entry point.

pub mod a2s;
pub mod browser;
mod commands;
pub mod error;
pub mod geoip;
pub mod launch;
pub mod news;
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
        .plugin(tauri_plugin_notification::init())
        // External links (news posts, Workshop pages) open in the default browser (D-099).
        .plugin(tauri_plugin_opener::init())
        // Size, position and maximised state come back on the next start (D-098);
        // visibility, decorations and fullscreen are left alone (frameless window).
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .build(),
        )
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

            // Watches favourites with the alert flag: one INFO per minute each (D-083).
            tauri::async_runtime::spawn(commands::watch_favourites(
                app.handle().clone(),
                Arc::clone(&cache),
                a2s.clone(),
            ));

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
                            // A LAN scan is not a list refresh: it must not push the
                            // automatic refresh's throttle or age out cached rows.
                            let full_list = d.source == "steam";
                            let c = Arc::clone(&cache);
                            let _ = tauri::async_runtime::spawn_blocking(move || {
                                if let Ok(c) = c.lock() {
                                    if full_list {
                                        let _ = c.set_meta("last_refresh", &browser::ServerRow::now_unix().to_string());
                                        let _ = c.prune(CACHE_MAX_AGE_SECS);
                                        let _ = c.population_prune(POPULATION_MAX_AGE_SECS);
                                    }
                                }
                            })
                            .await;
                            let targets = std::mem::take(&mut populated);
                            if !targets.is_empty() {
                                #[cfg(debug_assertions)]
                                eprintln!("[verify] start: {} populated servers", targets.len());
                                // Verification first (players), then the mod lists of
                                // whatever is populated and modded (D-080).
                                let (h, c, client) = (handle.clone(), Arc::clone(&cache), a2s.clone());
                                tauri::async_runtime::spawn(async move {
                                    commands::run_verification(
                                        h.clone(),
                                        Arc::clone(&c),
                                        client.clone(),
                                        targets,
                                        true,
                                    )
                                    .await;
                                    commands::run_mod_scan(h, c, client, false).await;
                                });
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
            // The main window is created here, after the state exists: Tauri builds
            // the windows of tauri.conf.json before this hook (app.rs 2525 vs 2531,
            // S-73), and on a slow start the page's first IPC call raced ahead of
            // `manage` and read default settings (D-112). `create: false` in the config.
            let main = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
                .ok_or("tauri.conf.json has no window labelled main")?;
            tauri::WebviewWindowBuilder::from_config(app.handle(), &main)?.build()?;
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
            commands::servers_dzsa,
            commands::servers_verify,
            commands::server_details,
            commands::server_slots,
            commands::join_plan,
            commands::mods_sync,
            commands::mods_unsubscribe,
            commands::mods_index,
            commands::mods_scan,
            commands::junctions_remove_dangling,
            commands::friends_list,
            commands::news_fetch,
            commands::news_cached,
            commands::news_thumb,
            commands::friend_avatar,
            commands::cache_stats,
            commands::steam_avatar,
            commands::launch_game,
            commands::favourites_list,
            commands::favourite_set,
            commands::favourite_alert_set,
            commands::history_list,
            commands::population_history,
            commands::direct_connect,
            commands::import_official_favourites
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
