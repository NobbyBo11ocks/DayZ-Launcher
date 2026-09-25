//! DZSA CrayZ Launcher core. Module layout follows docs/05-architecture-and-optimisation.md §2.
//! Windows-only desktop target; no mobile entry point.

pub mod a2s;
pub mod browser;
mod commands;
pub mod error;
pub mod geoip;
pub mod http;
pub mod launch;
pub mod log;
pub mod news;
pub mod proc;
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
/// A row no verification pass ever counted holds nothing worth a month; it is back
/// as a fresh row the next time Steam lists it (D-245).
const UNVERIFIED_MAX_AGE_SECS: i64 = 3 * 24 * 3600;
/// Ids per `servers:pruned` event: ~21 bytes each as JSON, so ~170 KB at most.
const PRUNED_EVENT_IDS: usize = 8_000;
/// Population samples older than this are dropped (the sparkline shows 72 h).
const POPULATION_MAX_AGE_SECS: i64 = 7 * 24 * 3600;

/// Elevation matching (D-119): DayZ inherits our level and must match Steam's, so
/// when Steam is elevated and we are not, restart elevated before anything else.
/// The result is kept for the join plan (the Diagnostics view went in D-168).
static ELEVATION: OnceLock<proc::ElevationState> = OnceLock::new();

pub fn elevation() -> proc::ElevationState {
    ELEVATION
        .get()
        .copied()
        .unwrap_or(proc::ElevationState::Matched)
}

/// Last resort when `cache.db` can be neither opened nor moved out of the way: a
/// cache that lives in memory for this run only. Nothing persists, and the user is
/// told once, but the launcher starts and the server list — which comes from Steam,
/// not from here — works normally (D-194).
fn in_memory_cache(why: &str) -> browser::Cache {
    match browser::Cache::open_in_memory() {
        Ok(c) => {
            log_warn!(
                "cache",
                "running without a cache file ({why}); favourites, join history and \
                 population will not be kept for this session"
            );
            fatal_dialog(&format!(
                "The launcher could not use its cache file:\n\n{why}\n\nIt is usually a \
                 lock held by antivirus, a backup or a file-sync client, and it clears \
                 by itself. The launcher will run normally this time, but favourites, \
                 join history and population will not be saved.\n\nNothing was deleted."
            ));
            c
        }
        // `Connection::open_in_memory` failing means the process is out of memory;
        // there is nothing left to fall back to.
        Err(e) => {
            log_error!("cache", "in-memory cache failed too: {e}");
            panic!("cache unavailable: {e}");
        }
    }
}

/// A dialog for the failures that happen before there is a window to put a message in.
///
/// `panic = "abort"` and `windows_subsystem = "windows"` between them mean a panic in
/// `setup` — which is how Tauri reports a failed setup — ends the process with no
/// window, no console and, if the data directory is the thing that failed, no log line
/// either. The icon flashes and nothing else ever happens. One message box is the
/// difference between "it doesn't work" and a sentence the user can act on (D-194).
fn fatal_dialog(message: &str) {
    use std::os::windows::ffi::OsStrExt;
    let wide = |s: &str| -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let text = wide(message);
    let caption = wide("DZSA CrayZ Launcher could not start");
    // SAFETY: both strings are NUL-terminated and outlive the call; a null owner window
    // is valid and makes the box application-modal.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            windows_sys::Win32::UI::WindowsAndMessaging::MB_OK
                | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
        );
    }
}

pub fn run() {
    let _ = STARTED.set(Instant::now());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "no further detail".into());
        log_error!("app", "panic: {what}");
        fatal_dialog(&format!(
            "The launcher stopped unexpectedly.\n\n{what}\n\nIf this keeps happening, \
             the log is at %LOCALAPPDATA%\\com.dayzlauncher.desktop\\logs\\launcher.log."
        ));
        previous(info);
    }));
    let steam_pid = steam::registry::detect().pid;
    let state = proc::elevation_state(steam_pid);
    if state == proc::ElevationState::SteamHigher && proc::relaunch_elevated() {
        return;
    }
    let _ = ELEVATION.set(state);
    proc::set_priority(proc::Priority::High);
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
        // External links (news posts, Workshop pages) open in the default browser (D-099).
        // DayZ update posts raise a Windows notification when the window is not
        // focused (D-099). Favourite alerts used this too until D-182.
        .plugin(tauri_plugin_notification::init())
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
            let data_dir = app.path().app_local_data_dir()?;
            // Before anything else that can fail, so a failure is in the log (D-158).
            log::init(&data_dir);
            log_info!(
                "app",
                "start v{} · elevation {:?}",
                env!("CARGO_PKG_VERSION"),
                elevation()
            );
            let db_path = data_dir.join("cache.db");
            let cache = Arc::new(Mutex::new(match Cache::open(&db_path) {
                Ok(c) => c,
                Err(e) => {
                    // D-160 moved a database aside when it would not open, so a corrupt
                    // file could not stop the app starting with no window and no message.
                    // It then deleted the -wal and -shm unconditionally, and that is a way
                    // to destroy data rather than recover it (D-187): when the file is
                    // *locked* rather than corrupt — an antivirus scan, a backup, a sync
                    // client — the rename fails while deleting the unlocked -wal succeeds,
                    // so the retry reopens the original database with every uncheckpointed
                    // commit gone. Favourites, join history and population live only here,
                    // and that is exactly the Q22 symptom.
                    //
                    // So: the three files move together or nothing moves. A write-ahead log
                    // is part of the database, never rubbish to sweep up.
                    log_error!("cache", "open failed at {}: {e}", db_path.display());
                    let stamp = browser::ServerRow::now_unix();
                    let aside = db_path.with_extension(format!("db.broken-{stamp}"));
                    match std::fs::rename(&db_path, &aside) {
                        Ok(()) => {
                            // The database moved, so its log and shared-memory file belong
                            // with it; a leftover -wal would be adopted by the new file.
                            for ext in ["db-wal", "db-shm"] {
                                let from = db_path.with_extension(ext);
                                let to = aside.with_extension(format!("{ext}-{stamp}"));
                                let _ = std::fs::rename(&from, &to);
                            }
                            match Cache::open(&db_path) {
                                Ok(c) => {
                                    log_warn!(
                                        "cache",
                                        "unreadable, moved to {} with its log; started on an empty cache. Favourites, history and population are in the moved file",
                                        aside.display()
                                    );
                                    c
                                }
                                Err(e2) => {
                                    log_error!("cache", "second open failed as well: {e2}");
                                    in_memory_cache(&format!("{e2}"))
                                }
                            }
                        }
                        Err(move_err) => {
                            // Almost always a lock, and a locked database is intact: the
                            // right answer is to leave every byte alone and say so.
                            log_error!(
                                "cache",
                                "could not open or move {} ({move_err}); the file is most likely held by another program, such as antivirus or a backup. Nothing was changed",
                                db_path.display()
                            );
                            in_memory_cache(&format!("{e}"))
                        }
                    }
                }
            }));
            let settings = SettingsStore::load(&app.path().app_config_dir()?.join("settings.json"));
            // Start-up logs before this point are kept deliberately: they are the ones
            // that explain a failure to start. From here the user's choice applies (D-169).
            {
                let s = settings.get();
                log::set_enabled(s.logging);
                log::set_muted(s.log_muted);
            }

            // Q22 and Q25 are both "the user's own rows are gone and nothing says when".
            // One line per start, before anything can write, is the before-and-after the
            // investigation has never had — and it costs three counting queries against
            // indexes on tables that hold tens of rows (D-193).
            if let Ok(c) = cache.lock() {
                match c.row_counts() {
                    Ok(n) => log_info!(
                        "cache",
                        "open: {} servers, {} favourites, {} joins, {} population samples",
                        n.servers,
                        n.favourites,
                        n.history,
                        n.population
                    ),
                    Err(e) => log_warn!("cache", "could not count rows at open: {e}"),
                }
            }

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
                verifying: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                scanning: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                dzsa: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
                            // Status changes are rare and always interesting (D-158).
                            let _ = handle.emit("steam:status", &s);
                        }
                        SteamEvent::Batch(rows) => {
                            populated.extend(rows.iter().filter(|r| r.steam_empty == Some(false)).filter_map(Target::from_row));
                            let _ = handle.emit("servers:batch", &rows);
                            let c = Arc::clone(&cache);
                            let _ = tauri::async_runtime::spawn_blocking(move || {
                                if let Ok(mut c) = c.lock() {
                                    if let Err(e) = c.upsert(&rows) {
                                        // Release builds have no console, so this used to
                                        // vanish entirely (D-160): a full disk lost the
                                        // whole cached list without a trace.
                                        log_error!("cache", "upsert of {} row(s) failed: {e}", rows.len());
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
                            log_info!(
                                "steam",
                                "refresh done ({}): {} listed, {} answered, {} failed, {} inflated, capped={} early={} in {} ms across {} partition(s)",
                                d.source,
                                d.total,
                                d.responded,
                                d.failed,
                                d.inflated,
                                d.capped,
                                d.stopped_early,
                                d.elapsed_ms,
                                d.partitions.len()
                            );
                            let _ = handle.emit("servers:done", &d);
                            // A LAN scan is not a list refresh: it must not push the
                            // automatic refresh's throttle or age out cached rows, and a
                            // rejected one fetched nothing at all (D-160): treating it as
                            // real wrote last_refresh, pruned rows the list never renewed
                            // and reported "0 of 0 shown" as a success.
                            let full_list = d.source == "steam" && !d.rejected;
                            // Only a refresh that Steam answered completely may withdraw
                            // vouches: a capped, early-stopped, timed-out or throttled one
                            // did not see every populated server, and withdrawing on it
                            // would punish the ones it never reached (D-233). The worker
                            // decides, from the partition answers (D-236).
                            let complete = full_list && d.complete;
                            let refresh_started = browser::ServerRow::now_unix()
                                - (d.elapsed_ms / 1000) as i64
                                - 10;
                            let c = Arc::clone(&cache);
                            let pruned = tauri::async_runtime::spawn_blocking(move || {
                                let mut pruned = Vec::new();
                                if let Ok(c) = c.lock() {
                                    if full_list {
                                        let _ = c.set_meta("last_refresh", &browser::ServerRow::now_unix().to_string());
                                        if complete {
                                            match c.unvouch_unseen(refresh_started) {
                                                Ok(n) if n > 0 => log_info!(
                                                    "steam",
                                                    "{n} server(s) Steam no longer lists as populated lost their vouch"
                                                ),
                                                Ok(_) => {}
                                                Err(e) => log_warn!("cache", "unvouch failed: {e}"),
                                            }
                                        }
                                        // Fakes the latest listing of the empty
                                        // partitions did not include go now; a
                                        // populated-only refresh says nothing about
                                        // them (D-245).
                                        let listed_empty = d
                                            .partitions
                                            .iter()
                                            .any(|p| p.filters.contains_key("noplayers"));
                                        match c.prune(
                                            CACHE_MAX_AGE_SECS,
                                            UNVERIFIED_MAX_AGE_SECS,
                                            listed_empty.then_some(refresh_started),
                                        ) {
                                            Ok(ids) => pruned = ids,
                                            Err(e) => log_warn!("cache", "prune failed: {e}"),
                                        }
                                        let _ = c.population_prune(POPULATION_MAX_AGE_SECS);
                                        // A refresh writes thousands of rows and never
                                        // checkpointed, so the WAL reached 4.5 MB and every
                                        // later start paid recovery over it (D-160).
                                        c.checkpoint();
                                    } else {
                                        // A LAN scan or the DZSA fallback still adds
                                        // population samples; without this they grew for
                                        // the whole session (D-160).
                                        let _ = c.population_prune(POPULATION_MAX_AGE_SECS);
                                    }
                                }
                                pruned
                            })
                            .await
                            .unwrap_or_default();
                            // The UI holds every row it was ever sent; without this a
                            // long-lived session kept, in the WebView, rows the cache
                            // had dropped a month ago (Q24, D-235).
                            // In chunks: rows only a Full refresh renews all reach 30 days
                            // together, ~21 000 ids at the D-233 cache size — 444 KiB in
                            // one event against docs/05 §4's ~200 KB ceiling (D-236).
                            for chunk in pruned.chunks(PRUNED_EVENT_IDS) {
                                let _ = handle.emit("servers:pruned", chunk);
                            }
                            // A rejected `Done` is an answer, not a result: the refresh
                            // it refers to either never reached Steam (D-160) or is still
                            // running (D-197's busy reply). Taking `populated` there stole
                            // the live refresh's partial rows and spent the one-pass guard
                            // on them, so the real completion a minute later found the
                            // guard held, skipped, and left the UI reading "verifying
                            // player counts…" until the five-minute watchdog invented an
                            // error (D-204).
                            // A rejected `Done` takes no targets — but only "busy"
                            // leaves them for someone else. That rejection comes from a
                            // refresh that is still running and still collecting into
                            // this accumulator; every other one means nothing is coming,
                            // so the partial rows have to go or the next refresh
                            // verifies them a second time and the totals stop adding up
                            // (D-208, after D-204).
                            let targets = if d.rejected {
                                if d.reason != Some("busy") {
                                    populated.clear();
                                }
                                Vec::new()
                            } else {
                                std::mem::take(&mut populated)
                            };
                            if !targets.is_empty() {
                                #[cfg(debug_assertions)]
                                eprintln!("[verify] start: {} populated servers", targets.len());
                                // Verification first (players), then the mod lists of
                                // whatever is populated and modded (D-080).
                                // One full pass at a time. Each one builds its own
                                // 128-permit pool (D-193) and they all draw on the same
                                // pacer, so overlapping passes multiply what an
                                // interactive query waits for its first datagram —
                                // measured 0.29 s for one, 2.53 s for eight (D-197).
                                let state = handle.try_state::<AppState>();
                                let scanning = state.as_ref().map(|s| Arc::clone(&s.scanning));
                                let guard = state
                                    .as_ref()
                                    .and_then(|s| commands::InFlight::claim(&s.verifying));
                                let Some(guard) = guard else {
                                    // The UI arms "verifying" on every refresh and only
                                    // `servers:verify-done` disarms it, so skipping has to
                                    // say so rather than go quiet (D-204) — but an empty
                                    // summary reads as "0 verified · 0 fake · 0 offline",
                                    // which is the invented result D-159 removed from the
                                    // LAN path. It says it was skipped, and the targets go
                                    // back so the running pass or the next refresh still
                                    // gets them (D-208).
                                    log_info!("verify", "a pass is already running; skipped");
                                    // Back into the accumulator the next refresh is
                                    // still extending, so dedupe: otherwise refresh
                                    // N+1 sends two PLAYER datagrams to every address
                                    // N already covered (against D-037) and reports a
                                    // total twice the sum of its verdicts, which is
                                    // the arithmetic D-208 existed to fix (D-220).
                                    populated = targets;
                                    populated.sort_by(|a, b| a.id.cmp(&b.id));
                                    populated.dedup_by(|a, b| a.id == b.id);
                                    let _ = handle.emit(
                                        "servers:verify-done",
                                        &commands::VerifySummary {
                                            skipped: true,
                                            ..Default::default()
                                        },
                                    );
                                    continue;
                                };
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
                                    // Released before the scan, which takes its own flag:
                                    // holding it across both kept the door shut for ~69 s
                                    // at the measured v0.1.26 timings, and a second Refresh
                                    // inside that window was simply lost (D-204).
                                    drop(guard);
                                    if let Some(scanning) = scanning {
                                        commands::run_mod_scan(h, c, client, scanning, false).await;
                                    }
                                });
                            } else if full_list {
                                // Nothing to verify still has to close the pass (D-151):
                                // the UI sets "verifying" when a refresh starts and only
                                // this event clears it. Only for a Steam list refresh
                                // though (D-159): a LAN scan never sets the flag, and an
                                // empty summary there replaced a good one on screen with
                                // "0 verified · 0 fake · 0 offline".
                                let _ = handle.emit(
                                    "servers:verify-done",
                                    &commands::VerifySummary::default(),
                                );
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
                            if d.ok {
                                log_info!(
                                    "mods",
                                    "downloaded {} item(s) in {} ms",
                                    d.items.len(),
                                    d.elapsed_ms
                                );
                            } else {
                                log_warn!(
                                    "mods",
                                    "download of {} item(s) failed after {} ms: {}",
                                    d.items.len(),
                                    d.elapsed_ms,
                                    d.error.as_deref().unwrap_or("no reason given")
                                );
                            }
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
            commands::diagnostics,
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
            commands::junctions_remove_dangling,
            commands::mods_index,
            commands::mods_scan,
            commands::mods_stale,
            commands::friends_list,
            commands::news_fetch,
            commands::news_cached,
            commands::news_thumb,
            commands::friend_avatar,
            commands::logs_recent,
            commands::logs_path,
            commands::log_ui,
            commands::launch_game,
            commands::favourites_list,
            commands::favourite_set,
            commands::history_list,
            commands::history_clear,
            commands::population_history,
            commands::direct_connect,
            commands::import_official_favourites
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|handle, event| {
            // Tauri exits through `process::exit`, so `Drop` never runs: the Steamworks
            // threads were still live when the process went away, `steamclient` asserted
            // "Illegal termination of worker thread 'SocketThread'" and the app
            // fast-failed with 0xC0000409 instead of exiting 0 — abandoning anything
            // still in the write-ahead log on the way out (D-190).
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(state) = handle.try_state::<AppState>() {
                    state.steam.shutdown();
                    if let Ok(c) = state.cache.lock() {
                        c.checkpoint_truncate();
                    }
                }
                log_info!("app", "exited cleanly");
            }
        });
}
