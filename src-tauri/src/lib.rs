//! DZSA CrayZ Launcher core. Module layout follows docs/05-architecture-and-optimisation.md §2.
//! Windows-only desktop target; no mobile entry point.

pub mod a2s;
pub mod browser;
mod commands;
pub mod error;
pub mod geoip;
mod hardware;
pub mod http;
mod icon;
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
/// The async runtime, four workers (D-297). Held for the life of the process: tauri
/// only borrows its handle.
static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

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

/// The app, for the window procedure that answers a launch's hand-over (row 27).
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// The main window to the front: the single-instance plugin's hand-over (D-079), and the
/// message a launch sends before its elevation decision (row 27).
fn bring_to_front(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Answers `proc::show_message` on the single-instance plugin's own window; everything
/// else goes on to the plugin as before.
unsafe extern "system" fn answer_show(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    if message == proc::show_message() {
        // Closing, it cannot come to the front: the launch waits for it to go and starts
        // itself (D-326), where it used to end, trusting a window that was on its way out.
        if CLOSING.load(std::sync::atomic::Ordering::SeqCst) {
            return proc::CLOSING;
        }
        if let Some(app) = APP.get() {
            bring_to_front(app);
        }
        return proc::SHOWN;
    }
    // SAFETY: the arguments are the ones this procedure was called with.
    unsafe { windows_sys::Win32::UI::Shell::DefSubclassProc(hwnd, message, wparam, lparam) }
}

/// Lets a later launch bring this instance to the front before it decides anything
/// (`proc::hand_to_running_instance`, row 27): the plugin's window answers its message,
/// from a launch without elevation too when this instance runs elevated.
fn answer_handovers(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
    let Some(hwnd) = proc::own_instance_window() else {
        log_warn!("app", "the single-instance window was not found; a second launch asks for its elevation first");
        return;
    };
    // SAFETY: this process's own window, subclassed on the thread that made it: setup
    // runs on the main thread, as the plugin's setup did before it.
    let hooked =
        unsafe { windows_sys::Win32::UI::Shell::SetWindowSubclass(hwnd, Some(answer_show), 1, 0) }
            != 0;
    if !hooked {
        log_warn!(
            "app",
            "the single-instance window could not be told to answer launches"
        );
        return;
    }
    if proc::current_is_elevated() && !proc::accept_show_from_lower_integrity() {
        log_warn!(
            "app",
            "a launch without elevation cannot reach this elevated instance"
        );
    }
}

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

/// Where a restored window goes when the saved state left it unusable (row 27). The
/// window-state plugin restores the saved size always, and the place only when a corner
/// of the window lies on a monitor, with no fitting (tauri-plugin-window-state 2.4.1
/// `restore_state`): after a monitor went, or with the scale changed, the window came back
/// larger than the screen with its buttons past the edge, or with its title bar, the only
/// handle to drag it by, above the top. It is left alone while it fits and both ends of its
/// title bar are on a screen, a window across two screens included; otherwise it shrinks
/// to the work area that holds most of it and moves the least it has to onto it, centred
/// on the first area given (the primary) when no area holds any of it. `None` for "leave
/// it".
fn fit_to_screen(win: Rect, areas: &[Rect]) -> Option<Rect> {
    let overlap = |a: &Rect| {
        let w = (win.x + win.w).min(a.x + a.w) - win.x.max(a.x);
        let h = (win.y + win.h).min(a.y + a.h) - win.y.max(a.y);
        i64::from(w.max(0)) * i64::from(h.max(0))
    };
    let on_screen = |x: i32, y: i32| {
        areas
            .iter()
            .any(|a| x >= a.x && x < a.x + a.w && y >= a.y && y < a.y + a.h)
    };
    let held = areas
        .iter()
        .max_by_key(|a| overlap(a))
        .filter(|a| overlap(a) > 0);
    let area = held.or(areas.first())?;
    let too_big = win.w > area.w || win.h > area.h;
    let bar = win.y + 8;
    if !too_big && on_screen(win.x + 8, bar) && on_screen(win.x + win.w - 8, bar) {
        return None;
    }
    let (w, h) = (win.w.min(area.w), win.h.min(area.h));
    let (x, y) = if held.is_some() {
        (
            win.x.clamp(area.x, area.x + area.w - w),
            win.y.clamp(area.y, area.y + area.h - h),
        )
    } else {
        (area.x + (area.w - w) / 2, area.y + (area.h - h) / 2)
    };
    let fitted = Rect { x, y, w, h };
    (fitted != win).then_some(fitted)
}

/// Applies `fit_to_screen` to the main window once the saved state is back.
fn fit_restored_window(window: &tauri::WebviewWindow) {
    if window.is_maximized().unwrap_or(false) || window.is_minimized().unwrap_or(false) {
        return;
    }
    let (Ok(pos), Ok(outer), Ok(inner), Ok(monitors)) = (
        window.outer_position(),
        window.outer_size(),
        window.inner_size(),
        window.available_monitors(),
    ) else {
        return;
    };
    let rect = |m: &tauri::Monitor| {
        let a = m.work_area();
        Rect {
            x: a.position.x,
            y: a.position.y,
            w: a.size.width as i32,
            h: a.size.height as i32,
        }
    };
    let mut areas: Vec<Rect> = monitors.iter().map(rect).collect();
    if let Ok(Some(primary)) = window.primary_monitor() {
        let p = rect(&primary);
        areas.retain(|a| *a != p);
        areas.insert(0, p);
    }
    let win = Rect {
        x: pos.x,
        y: pos.y,
        w: outer.width as i32,
        h: outer.height as i32,
    };
    let Some(fitted) = fit_to_screen(win, &areas) else {
        return;
    };
    // `set_size` takes the inner size; the frame, if any, stays what it was.
    let inner_w = (inner.width as i32 - (win.w - fitted.w)).max(1) as u32;
    let inner_h = (inner.height as i32 - (win.h - fitted.h)).max(1) as u32;
    let _ = window.set_size(tauri::PhysicalSize::new(inner_w, inner_h));
    let _ = window.set_position(tauri::PhysicalPosition::new(fitted.x, fitted.y));
    log_info!(
        "app",
        "the saved window place did not fit the screens: {win:?} -> {fitted:?}"
    );
}

/// Set once the window may close: the page has kept what it had to, or had its time.
static CLOSING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Closes the main window for good (row 27): the page answered `app:closing`
/// (`commands::close_ready`), or 1.5 s went by without it.
pub(crate) fn finish_close(app: &tauri::AppHandle) {
    CLOSING.store(true, std::sync::atomic::Ordering::SeqCst);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.close();
    }
}

/// Whether the main window, minimised, will come back maximised, as last seen at a close
/// or at the exit (row 27).
static KEEP_MAXIMIZED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Notes whether the main window is minimised from maximised. tao reports a minimised
/// window as not maximised, and the window-state plugin saves that (tauri-plugin-window-state
/// 2.4.1 `update_state`): a launcher closed from the taskbar while minimised from maximised,
/// or left so at sign-out, opened next time un-maximised at the maximised corner. Windows'
/// own placement still knows (`WPF_RESTORETOMAXIMIZED`).
fn note_maximized(app: &tauri::AppHandle) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    let restores_maximized = w.is_minimized().unwrap_or(false)
        && w.hwnd().is_ok_and(|hwnd| {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                GetWindowPlacement, WINDOWPLACEMENT, WPF_RESTORETOMAXIMIZED,
            };
            // SAFETY: a live window of this process; the struct's length is set as the
            // call requires.
            unsafe {
                let mut wp: WINDOWPLACEMENT = std::mem::zeroed();
                wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
                GetWindowPlacement(hwnd.0 as _, &mut wp) != 0
                    && wp.flags & WPF_RESTORETOMAXIMIZED != 0
            }
        });
    KEEP_MAXIMIZED.store(restores_maximized, std::sync::atomic::Ordering::SeqCst);
}

/// After the window-state plugin has written its file: the main window saved as maximised
/// when `note_maximized` saw it minimised from maximised.
fn keep_maximized(app: &tauri::AppHandle) {
    if !KEEP_MAXIMIZED.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    use tauri_plugin_window_state::AppHandleExt;
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    let path = dir.join(app.filename());
    if let Some(text) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| mark_maximized(&t, "main"))
    {
        let _ = std::fs::write(&path, text);
    }
}

/// The window-state file's text with `label` saved as maximised; `None` when it has no
/// such window, says so already, or is not the plugin's JSON.
fn mark_maximized(json: &str, label: &str) -> Option<String> {
    let mut all: serde_json::Value = serde_json::from_str(json).ok()?;
    let window = all.get_mut(label)?.as_object_mut()?;
    if window.get("maximized") == Some(&serde_json::Value::Bool(true)) {
        return None;
    }
    window.insert("maximized".into(), serde_json::Value::Bool(true));
    serde_json::to_string_pretty(&all).ok()
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
            // It does start, so the caption says so: "could not start" stood above a
            // message saying it would run (row 14, F14, approved).
            message_box(
                "DZSA CrayZ Launcher is running without its cache",
                &format!(
                    "The launcher could not use its cache file:\n\n{why}\n\nIt is usually a \
                     lock held by antivirus, a backup or a file-sync client, and it clears \
                     by itself. The launcher will run normally this time, but favourites, \
                     join history and population will not be saved.\n\nNothing was deleted."
                ),
            );
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

/// Opens `cache.db` for writing, or decides what its failure means (D-302).
///
/// D-162 moved a database aside when it would not open, so a corrupt file could not
/// stop the app starting with no window and no message, and D-187 made the database
/// and its -wal and -shm move together or not at all: a write-ahead log is part of the
/// database, never rubbish to sweep up. That still moved the database on *any* error,
/// and a lock on the -wal or -shm alone — an antivirus scan, a backup, a sync client —
/// fails the open with SQLITE_CANTOPEN. The intact database moved, its held -wal could
/// not follow and was renamed where SQLite never looks for it, and the next start made
/// a new database beside a -wal that SQLite then reset: favourites and joins that lived
/// only there were gone (row 14, H2; reproduced with 50 of each). Only SQLite's own
/// verdict that the file is damaged moves it now; anything else runs in memory for the
/// session and leaves every byte where it was.
///
/// A file SQLite *can* open is not necessarily one it can write: a handle that denies
/// writers on cache.db opens it read-only, and one on the -wal or -shm lets it open and
/// then fails every write, and either way the session looked normal and kept nothing
/// (row 14, H3). A write in a transaction that is rolled back finds out; the file is
/// opened again for two seconds in case the lock clears, then the session runs in
/// memory.
///
/// What it did goes with the cache, for the page to say (`cache_status`): a session in
/// memory keeps nothing and shows nothing saved before, and a damaged file moved aside
/// starts the favourites and Recent empty (row 14, F14 and H2, approved).
fn open_cache(db_path: &std::path::Path) -> (Cache, commands::CacheOpened) {
    const REOPENS: u32 = 8;
    let in_memory = |why: String, moved_to: Option<std::path::PathBuf>| {
        (
            in_memory_cache(&why),
            commands::CacheOpened {
                in_memory: true,
                moved_to,
            },
        )
    };
    let mut tries = 0;
    let err = loop {
        match Cache::open(db_path) {
            Ok(c) => match c.probe_write() {
                Ok(()) => return (c, commands::CacheOpened::default()),
                // Another writer: SQLite has already waited its five seconds, and a
                // busy database is a writable one once it is done.
                Err(e) if cache_busy(&e) => {
                    log_warn!(
                        "cache",
                        "{} is busy at start ({e}); carrying on",
                        db_path.display()
                    );
                    return (c, commands::CacheOpened::default());
                }
                Err(e) if tries < REOPENS => {
                    if tries == 0 {
                        log_warn!(
                            "cache",
                            "{} cannot be written ({e}); opening it again",
                            db_path.display()
                        );
                    }
                    tries += 1;
                    drop(c);
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
                Err(e) => {
                    log_error!(
                        "cache",
                        "{} still cannot be written ({e}); it is most likely held by another program, such as antivirus or a backup. Nothing was changed",
                        db_path.display()
                    );
                    return in_memory(format!("{e}"), None);
                }
            },
            Err(e) => break e,
        }
    };
    log_error!("cache", "open failed at {}: {err}", db_path.display());
    let damaged = matches!(
        err.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase)
    );
    if !damaged {
        // Almost always a lock, and a locked database is intact: the right answer is
        // to leave every byte alone and say so.
        log_error!(
            "cache",
            "{} is not damaged, most likely held by another program, such as antivirus or a backup. Nothing was changed",
            db_path.display()
        );
        return in_memory(format!("{err}"), None);
    }
    match move_aside(db_path) {
        Ok(aside) => match Cache::open(db_path) {
            Ok(c) => {
                log_warn!(
                    "cache",
                    "damaged, moved to {} with its log; started on an empty cache. Favourites, history and population are in the moved file",
                    aside.display()
                );
                (
                    c,
                    commands::CacheOpened {
                        in_memory: false,
                        moved_to: Some(aside),
                    },
                )
            }
            Err(e2) => {
                log_error!(
                    "cache",
                    "damaged, moved to {}; a new cache could not be opened either: {e2}",
                    aside.display()
                );
                in_memory(format!("{e2}"), Some(aside))
            }
        },
        Err(move_err) => {
            log_error!(
                "cache",
                "damaged, but {} could not be moved with its log ({move_err}). Nothing was changed",
                db_path.display()
            );
            in_memory(format!("{err}"), None)
        }
    }
}

fn cache_busy(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    )
}

/// `path` with `suffix` added to the whole name, the way SQLite names a database's
/// -wal and -shm.
fn with_suffix(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    s.into()
}

/// Moves a damaged `cache.db` aside with its -wal and -shm, under the names SQLite pairs
/// them by (`cache.db.broken-<stamp>-wal`), side files first so a -wal is never left for
/// the new database to adopt or reset. They were renamed `cache.db.db-wal-<stamp>`,
/// which SQLite never pairs with anything (row 14, H2). A side file that exists and
/// cannot move puts back whatever moved, and the error says why.
fn move_aside(db_path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    let aside = with_suffix(
        db_path,
        &format!(".broken-{}", browser::ServerRow::now_unix()),
    );
    let mut moved: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();
    let undo = |moved: &[(std::path::PathBuf, std::path::PathBuf)]| {
        for (from, to) in moved.iter().rev() {
            let _ = std::fs::rename(to, from);
        }
    };
    for suffix in ["-wal", "-shm"] {
        let from = with_suffix(db_path, suffix);
        match std::fs::symlink_metadata(&from) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                undo(&moved);
                return Err(e);
            }
            Ok(_) => {}
        }
        let to = with_suffix(&aside, suffix);
        if let Err(e) = std::fs::rename(&from, &to) {
            undo(&moved);
            return Err(e);
        }
        moved.push((from, to));
    }
    if let Err(e) = std::fs::rename(db_path, &aside) {
        undo(&moved);
        return Err(e);
    }
    Ok(aside)
}

/// A dialog for the failures that happen before there is a window to put a message in.
///
/// `panic = "abort"` and `windows_subsystem = "windows"` between them mean a panic in
/// `setup` — which is how Tauri reports a failed setup — ends the process with no
/// window, no console and, if the data directory is the thing that failed, no log line
/// either. The icon flashes and nothing else ever happens. One message box is the
/// difference between "it doesn't work" and a sentence the user can act on (D-194).
fn fatal_dialog(message: &str) {
    message_box("DZSA CrayZ Launcher could not start", message);
}

/// A Windows message box with an error icon, for when there is no window yet.
fn message_box(caption: &str, message: &str) {
    use std::os::windows::ffi::OsStrExt;
    let wide = |s: &str| -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let text = wide(message);
    let caption = wide(caption);
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

/// Stops the Steam worker and checkpoints the cache, for every way out. Twice is
/// harmless: the second shutdown finds the thread gone, the checkpoint finds nothing.
fn close_down(handle: &tauri::AppHandle) {
    if let Some(state) = handle.try_state::<AppState>() {
        state.steam.shutdown();
        if let Ok(c) = state.cache.lock() {
            c.checkpoint_truncate();
        }
    }
}

/// Runs `close_down` when Tauri clears the app's resource table on the way out. The
/// updater's "install and restart" leaves through `cleanup_before_exit` and then
/// `process::exit`, never reaching `RunEvent::Exit`, so every update installed with a
/// live Steam session took the exit D-190 diagnosed as the 0xC0000409 crash, and
/// skipped the final checkpoint (tauri-plugin-updater 2.12.0 `on_before_exit`, tauri
/// 2.11.6 `cleanup_before_exit`; D-275).
struct ExitGuard(tauri::AppHandle);

impl tauri::Resource for ExitGuard {}

impl Drop for ExitGuard {
    fn drop(&mut self) {
        // The window's size and place as well: the window-state plugin writes them only on
        // RunEvent::Exit, which this way out never reaches, so "Install and restart" lost
        // the session's window (row 15, H6; tauri-plugin-window-state 2.4.1 `on_event`).
        // The resource table is cleared before the windows are hidden, so they still
        // report where they are; after a normal exit this is a second, identical save.
        use tauri_plugin_window_state::AppHandleExt;
        note_maximized(&self.0);
        let _ = self.0.save_window_state(WINDOW_STATE);
        keep_maximized(&self.0);
        close_down(&self.0);
    }
}

/// What the window-state plugin keeps (D-098): visibility, decorations and fullscreen are
/// left alone (frameless window).
const WINDOW_STATE: tauri_plugin_window_state::StateFlags =
    tauri_plugin_window_state::StateFlags::SIZE
        .union(tauri_plugin_window_state::StateFlags::POSITION)
        .union(tauri_plugin_window_state::StateFlags::MAXIMIZED);

/// The first start's window in logical pixels: 1440×900, or 90 % of the work area where
/// that is smaller, never below the minimum. The runtime centres it in the same work
/// area (tauri-runtime-wry 2.11.4 `calculate_window_center_position`).
fn first_size(work: tauri::PhysicalSize<u32>, scale: f64, min: (f64, f64)) -> (f64, f64) {
    let fit = |px: u32, cap: f64, min: f64| (f64::from(px) / scale * 0.9).floor().min(cap).max(min);
    (
        fit(work.width, 1440.0, min.0),
        fit(work.height, 900.0, min.1),
    )
}

/// The updater leaves every setup it downloads in `%TEMP%\<product>-<version>-updater-*\`:
/// it keeps the folder and exits before the file's own clean-up runs (tauri-plugin-updater
/// 2.12.0 `make_temp_dir`, `write_to_temp`), 7.5 MB per update. They go at the next start;
/// only that shape is touched, the setup file by its exact name and then the folder if
/// nothing else is in it. A setup still running holds its file, which stays for the next
/// start (D-279).
fn sweep_updater_leftovers(product: &str) {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    let prefix = format!("{product}-");
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(version) = name
            .to_str()
            .and_then(|n| n.strip_prefix(&prefix))
            .and_then(|rest| rest.split_once("-updater-"))
            .map(|(v, _)| v.to_owned())
        else {
            continue;
        };
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let dir = entry.path();
        if std::fs::remove_file(dir.join(format!("{product}-{version}-installer.exe"))).is_ok() {
            removed += 1;
        }
        let _ = std::fs::remove_dir(&dir);
    }
    if removed > 0 {
        log_info!(
            "update",
            "removed {removed} downloaded setup(s) left in the temp folder"
        );
    }
}

/// The log's line for a panic: what, where and on which thread.
fn panic_line(what: &str, location: Option<&str>, thread: Option<&str>) -> String {
    format!(
        "panic: {what} at {} on thread {}",
        location.unwrap_or("an unknown place"),
        thread.unwrap_or("without a name")
    )
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
        // Where it happened, and past a muted App: the dialog sends the player to this
        // line, a release build has no console, and the line had neither (row 25).
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let thread = std::thread::current().name().map(str::to_string);
        crate::log::write_unmuted(
            crate::log::Level::Error,
            "app",
            panic_line(&what, location.as_deref(), thread.as_deref()),
        );
        fatal_dialog(&format!(
            "The launcher stopped unexpectedly.\n\n{what}\n\nIf this keeps happening, \
             the log is at %LOCALAPPDATA%\\com.dayzlauncher.desktop\\logs\\launcher.log."
        ));
        previous(info);
    }));
    // A launch while the launcher is open goes to it before anything else, its elevation
    // decision included: that asked for administrator rights first whenever Steam ran
    // elevated, only to hand over afterwards (row 27).
    if proc::hand_to_running_instance() {
        return;
    }
    let steam_pid = steam::registry::detect().pid;
    let state = proc::elevation_state(steam_pid);
    if state == proc::ElevationState::SteamHigher && proc::relaunch_elevated() {
        return;
    }
    let _ = ELEVATION.set(state);
    proc::set_priority(proc::Priority::High);
    // Four workers, not tauri's one per logical CPU: 32 on the reference machine held
    // 1.5 MB against 0.2 MB for four, and the host's async work is waiting — A2S
    // datagrams, HTTP, blocking tasks, which have a pool of their own (row 13, D-297).
    if let Ok(rt) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
    {
        tauri::async_runtime::set(RUNTIME.get_or_init(|| rt).handle().clone());
    }
    tauri::Builder::default()
        // Must be the first plugin (its README): a second launch hands its arguments
        // to the running instance, which just comes to the front (D-079).
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            bring_to_front(app);
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
                .with_state_flags(WINDOW_STATE)
                .build(),
        )
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?;
            // Before anything else that can fail, so a failure is in the log (D-158).
            log::init(&data_dir);
            // The player's Recording choice before the first line: loaded after the start-up
            // lines, it let "start v…", the DayZ-running line and any cache warnings into
            // the file at every start with Recording off, which the Logs page says never
            // happens (row 15, H7). The settings file's own read errors are still written,
            // since recording stays on until the file has been read (D-169).
            let settings = SettingsStore::load(&app.path().app_config_dir()?.join("settings.json"));
            {
                let s = settings.get();
                log::set_enabled(s.logging);
                log::set_muted(s.log_muted);
            }
            // The facts a bug report needs, in the line every log starts with (row 25).
            log_info!(
                "app",
                "start v{} · Windows {} · WebView2 {} · elevation {:?}",
                env!("CARGO_PKG_VERSION"),
                commands::windows_build().as_deref().unwrap_or("?"),
                tauri::webview_version().ok().as_deref().unwrap_or("?"),
                elevation()
            );
            // Started while DayZ plays (relaunched by an update, or by hand): this instance
            // never launched the game, so D-119's step-down never ran and it sat at High
            // beside the game for the rest of the session (D-279).
            if let Some(pid) = steam::registry::process::find_named("DayZ_x64.exe") {
                log_info!("app", "DayZ is running (pid {pid}); starting below normal priority");
                proc::step_down_while_running(pid);
            }
            let product = app.package_info().name.clone();
            let _ = std::thread::Builder::new()
                .name("temp-sweep".into())
                .spawn(move || sweep_updater_leftovers(&product));
            let db_path = data_dir.join("cache.db");
            let (cache, cache_opened) = open_cache(&db_path);
            let cache = Arc::new(Mutex::new(cache));

            // Q22 and Q25 are both "the user's own rows are gone and nothing says when".
            // One line per start, before anything can write, is the before-and-after the
            // investigation has never had — and it costs four counting queries, each under
            // a millisecond (0.7–0.9 ms measured at 105 000 servers and 135 000 population
            // samples, row 19; D-193, D-287).
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
                launching: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                rows_out: std::sync::Mutex::new(None),
                settings,
                cache_opened,
            });
            app.resources_table().add(ExitGuard(app.handle().clone()));

            // Forwards Steam-thread events to the WebView, persists batches, and
            // verifies populated servers (docs/11 R2–R5) once a refresh completes.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut populated: Vec<Target> = Vec::new();
                // What Steam listed as populated during the refresh under way. A vouch
                // the refresh did not renew goes by this set when it completes (D-271).
                let mut listed: std::collections::HashSet<String> = std::collections::HashSet::new();
                while let Some(ev) = rx.recv().await {
                    match ev {
                        SteamEvent::Status(s) => {
                            // Status changes are rare and always interesting (D-158).
                            let _ = handle.emit("steam:status", &s);
                        }
                        SteamEvent::Batch(rows) => {
                            populated.extend(rows.iter().filter(|r| r.steam_empty == Some(false)).filter_map(Target::from_row));
                            listed.extend(rows.iter().filter(|r| r.steam_empty == Some(false)).map(|r| r.id.clone()));
                            commands::send_rows(&handle, "batch", &rows);
                            let c = Arc::clone(&cache);
                            let _ = tauri::async_runtime::spawn_blocking(move || {
                                if let Ok(mut c) = c.lock() {
                                    let stored = c.upsert(&rows);
                                    browser::cache::note_write(&stored);
                                    if let Err(e) = stored {
                                        // Release builds have no console, so this used to
                                        // vanish entirely (D-162): a full disk lost the
                                        // whole cached list without a trace. A refusal
                                        // of the file itself is `note_write`'s, once
                                        // an episode (row 25).
                                        if !browser::cache::is_file_failure(&e) {
                                            log_error!("cache", "upsert of {} row(s) failed: {e}", rows.len());
                                        }
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
                            // A rejection names its reason: "busy", "no-session" and
                            // "no-answer" all logged as an empty refresh with early=true, and
                            // the log could not tell them from one another (row 14, H12).
                            log_info!(
                                "steam",
                                "refresh done ({}): {} listed, {} answered, {} failed, {} inflated, capped={} early={}{} in {} ms across {} partition(s)",
                                d.source,
                                d.total,
                                d.responded,
                                d.failed,
                                d.inflated,
                                d.capped,
                                d.stopped_early,
                                if d.rejected {
                                    format!(" rejected={}", d.reason.unwrap_or("?"))
                                } else {
                                    String::new()
                                },
                                d.elapsed_ms,
                                d.partitions.len()
                            );
                            // Each partition of a full Refresh, so the Logs page shows which
                            // ones Steam answered and which it refused (row 19).
                            if d.partitions.len() > 1 {
                                for p in &d.partitions {
                                    let mut filters: Vec<String> =
                                        p.filters.iter().map(|(k, v)| format!("{k}={v}")).collect();
                                    filters.sort();
                                    log_info!(
                                        "steam",
                                        "partition {}: {} listed, {} answered, {} in {} ms",
                                        filters.join(" "),
                                        p.total,
                                        p.responded,
                                        p.response,
                                        p.elapsed_ms
                                    );
                                }
                            }
                            commands::send_rows(&handle, "done", &d);
                            // A LAN scan is not a list refresh: it must not push the
                            // automatic refresh's throttle or age out cached rows (D-096),
                            // and a rejected one fetched nothing at all (D-161): treating it as
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
                            // A "busy" rejection belongs to a refresh still collecting;
                            // every other Done closes the set, used or not.
                            let listed_now = if d.reason == Some("busy") {
                                std::collections::HashSet::new()
                            } else {
                                std::mem::take(&mut listed)
                            };
                            // The empty-list lanes prune what a listing of the empty
                            // partitions omitted — so every one of them has to have
                            // answered. A throttled refresh stopped early, or a map
                            // partition that timed out, omitted rows it never listed,
                            // and deleted every fake and DZSA-only row on those maps,
                            // verification history and all (D-271).
                            let listed_empty =
                                steam::sdk::empty_listing_complete(&d.partitions, d.stopped_early);
                            // And map by map: a full Refresh has always stopped early on
                            // Steam's throttle, so the lane above never ran and the cache
                            // grew by every Refresh's new farm ids (row 19).
                            let listed_maps = steam::sdk::answered_empty_maps(&d.partitions);
                            let c = Arc::clone(&cache);
                            let pruned = tauri::async_runtime::spawn_blocking(move || {
                                let mut pruned = Vec::new();
                                if let Ok(c) = c.lock() {
                                    if full_list {
                                        let _ = c.set_meta("last_refresh", &browser::ServerRow::now_unix().to_string());
                                        if complete {
                                            match c.unvouch_unlisted(&listed_now) {
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
                                        // them (D-245), nor does one that did not
                                        // finish listing them (above).
                                        match c.prune(
                                            CACHE_MAX_AGE_SECS,
                                            UNVERIFIED_MAX_AGE_SECS,
                                            listed_empty.then_some(refresh_started),
                                            Some((refresh_started, listed_maps.as_slice())),
                                        ) {
                                            Ok(ids) => pruned = ids,
                                            Err(e) => log_warn!("cache", "prune failed: {e}"),
                                        }
                                        match c.population_prune(POPULATION_MAX_AGE_SECS) {
                                            Ok(n) if n > 0 => log_info!(
                                                "cache",
                                                "{n} population sample(s) older than 7 days removed"
                                            ),
                                            Ok(_) => {}
                                            Err(e) => log_warn!("cache", "population prune failed: {e}"),
                                        }
                                        // A refresh writes thousands of rows and never
                                        // checkpointed, so the WAL reached 4.5 MB and every
                                        // later start paid recovery over it (D-164).
                                        c.checkpoint();
                                    } else {
                                        // No list refresh comes this way, but checks of
                                        // the rows on screen still add population
                                        // samples; without this they grew for the whole session
                                        // (D-288). The DZSA fallback never comes this
                                        // way: `servers_dzsa` sends its own `servers:done`.
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
                                commands::send_rows(&handle, "pruned", chunk);
                            }
                            // A rejected `Done` is an answer, not a result: the refresh
                            // it refers to either never reached Steam (D-161) or is still
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
                            let mut targets = if d.rejected {
                                if d.reason != Some("busy") {
                                    populated.clear();
                                }
                                Vec::new()
                            } else {
                                std::mem::take(&mut populated)
                            };
                            // Each server once, at its newest listing. Targets a skipped
                            // pass put back meet this refresh's own rows here, and both
                            // went out: two queries per address and a total twice its
                            // verdicts (D-220's aim, reached here; row 18).
                            targets.sort_by(|a, b| a.id.cmp(&b.id).then(b.reported_at.cmp(&a.reported_at)));
                            targets.dedup_by(|a, b| a.id == b.id);
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
                                    commands::send_rows(&handle, "verify-done", &commands::VerifySummary {
                                            skipped: true,
                                            ..Default::default()
                                        },
                                    );
                                    continue;
                                };
                                let (h, c, client) = (handle.clone(), Arc::clone(&cache), a2s.clone());
                                let covered: std::collections::HashSet<String> =
                                    targets.iter().map(|t| t.id.clone()).collect();
                                tauri::async_runtime::spawn(async move {
                                    commands::run_verification(
                                        h.clone(),
                                        Arc::clone(&c),
                                        client.clone(),
                                        targets,
                                        true,
                                    )
                                    .await;
                                    // The pass counts what Steam listed; a server the
                                    // capped partitions never returned keeps a count
                                    // hours old that still sorts and adds up as fresh —
                                    // half of the populated rows on 2026-09-25. They
                                    // are read again now, without a summary (D-247).
                                    let c2 = Arc::clone(&c);
                                    let stale = tauri::async_runtime::spawn_blocking(move || {
                                        c2.lock()
                                            .ok()
                                            .and_then(|c| c.counted_before(refresh_started).ok())
                                            .unwrap_or_default()
                                    })
                                    .await
                                    .unwrap_or_default();
                                    let stale: Vec<Target> = stale
                                        .iter()
                                        .filter(|r| !covered.contains(&r.id))
                                        .filter_map(Target::from_row)
                                        .collect();
                                    if !stale.is_empty() {
                                        log_info!(
                                            "verify",
                                            "{} counted server(s) the listing did not reach: read again",
                                            stale.len()
                                        );
                                        commands::run_verification(
                                            h.clone(),
                                            Arc::clone(&c),
                                            client.clone().with_concurrency(commands::VERIFY_CONCURRENCY),
                                            stale,
                                            false,
                                        )
                                        .await;
                                    }
                                    // Released before the scan, which takes its own flag:
                                    // holding it across both kept the door shut for ~69 s
                                    // at the measured v0.1.26 timings, and a second Refresh
                                    // inside that window was simply lost (D-204).
                                    drop(guard);
                                    if let Some(scanning) = scanning {
                                        commands::run_mod_scan(h, c, client, scanning).await;
                                    }
                                });
                            } else if full_list {
                                // Nothing to verify still has to close the pass (D-151):
                                // the UI sets "verifying" when a refresh starts and only
                                // this event clears it. Only for a Steam list refresh
                                // though (D-159): a LAN scan never sets the flag, and an
                                // empty summary there replaced a good one on screen with
                                // "0 verified · 0 fake · 0 offline".
                                commands::send_rows(&handle, "verify-done", &commands::VerifySummary::default(),
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
                            } else if d.superseded {
                                // Not a failure: a newer download took over (D-277).
                                log_info!(
                                    "mods",
                                    "download replaced by a newer one after {} ms",
                                    d.elapsed_ms
                                );
                            } else {
                                log_warn!(
                                    "mods",
                                    "download of {} item(s) failed after {} ms: {}{}",
                                    d.items.len(),
                                    d.elapsed_ms,
                                    d.failed_id.map(|id| format!("{id}: ")).unwrap_or_default(),
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
            let mut main = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
                .ok_or("tauri.conf.json has no window labelled main")?;
            // The first start's size fits the screen: at the config's 1280×800 the
            // details pane floated over five columns, and on a 1366×768 screen the
            // window was taller than the work area, so centring put the title bar
            // above it (row 16). A saved size and place still win: the window-state
            // plugin restores them once the window is ready (S-115).
            if let Ok(Some(m)) = app.primary_monitor() {
                let min = (main.min_width.unwrap_or(960.0), main.min_height.unwrap_or(600.0));
                (main.width, main.height) = first_size(m.work_area().size, m.scale_factor(), min);
            }
            let window = tauri::WebviewWindowBuilder::from_config(app.handle(), &main)?.build()?;
            // The taskbar's icon from the exe's own icon group, at the right size: tao
            // only sets the small one, which the taskbar scaled up soft (Q31, D-264).
            if let Ok(hwnd) = window.hwnd() {
                icon::apply(hwnd.0 as _);
            }
            // The plugin restored the saved place inside `build` (tauri 2.11.6 runs the
            // window-ready hooks there): now onto a screen if it left it off one (row 27).
            fit_restored_window(&window);
            answer_handovers(app.handle());
            proc::watch_for_game();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::rows_subscribe,
            commands::cache_status,
            commands::name_as_sent,
            commands::app_info,
            commands::diagnostics,
            commands::local_game_version,
            commands::steam_status,
            commands::steam_start,
            commands::perf_args,
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
            commands::open_link,
            commands::close_ready,
            commands::log_ui,
            commands::launch_game,
            commands::game_running,
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
            // A move to a screen with another scale wants the icons at its sizes (D-264).
            if let tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::ScaleFactorChanged { .. },
                ..
            } = &event
            {
                if let Some(Ok(hwnd)) = handle.get_webview_window(label).map(|w| w.hwnd()) {
                    icon::apply(hwnd.0 as _);
                }
            }
            // Every way of closing the window reaches the page's close-time work first
            // (row 27): only the title bar's button waited for it, so Alt+F4 and the
            // taskbar's Close lost a preference still in its 150 ms batch and a Settings
            // edit inside its 300 ms debounce. The page answers `close_ready`; after
            // 1.5 s the window closes whatever it does, so a page that stopped answering
            // cannot keep it open.
            if let tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } = &event
            {
                if label == "main" {
                    note_maximized(handle);
                    if !CLOSING.swap(true, std::sync::atomic::Ordering::SeqCst) {
                        api.prevent_close();
                        let _ = handle.emit_to("main", "app:closing", ());
                        let app = handle.clone();
                        let _ = std::thread::Builder::new()
                            .name("close-wait".into())
                            .spawn(move || {
                                std::thread::sleep(std::time::Duration::from_millis(1500));
                                finish_close(&app);
                            });
                    }
                }
            }
            // Tauri exits through `process::exit`, so `Drop` never runs: the Steamworks
            // threads were still live when the process went away, `steamclient` asserted
            // "Illegal termination of worker thread 'SocketThread'" and the app
            // fast-failed with 0xC0000409 instead of exiting 0 — abandoning anything
            // still in the write-ahead log on the way out (D-190).
            if matches!(event, tauri::RunEvent::Exit) {
                // The plugins' Exit hooks ran first, the window-state file written: a
                // sign-out comes here with the window still up and no close before it.
                note_maximized(handle);
                keep_maximized(handle);
                close_down(handle);
                log_info!("app", "exited cleanly");
            }
        });
}

#[cfg(test)]
mod tests {
    /// Row 25: the panic line says where and on which thread.
    #[test]
    fn a_panic_line_says_where() {
        assert_eq!(
            super::panic_line(
                "boom",
                Some("src/lib.rs:10:5"),
                Some("tokio-runtime-worker")
            ),
            "panic: boom at src/lib.rs:10:5 on thread tokio-runtime-worker"
        );
        assert_eq!(
            super::panic_line("boom", None, None),
            "panic: boom at an unknown place on thread without a name"
        );
    }

    use super::{fit_to_screen, mark_maximized, Rect};

    /// Row 27: a restored window is left where it fits, and brought onto a screen where
    /// it does not.
    #[test]
    fn a_restored_window_is_fitted_to_the_screens() {
        let r = |x, y, w, h| Rect { x, y, w, h };
        let laptop = [r(0, 0, 1920, 1032)];
        // Fits: left alone.
        assert_eq!(fit_to_screen(r(100, 100, 1440, 900), &laptop), None);
        // A 2000×1200 window from a larger screen: shrunk to the work area.
        assert_eq!(
            fit_to_screen(r(96, 51, 2000, 1200), &laptop),
            Some(r(0, 0, 1920, 1032))
        );
        // Its title bar above the top: down onto the screen, size kept.
        assert_eq!(
            fit_to_screen(r(100, -400, 1440, 900), &laptop),
            Some(r(100, 0, 1440, 900))
        );
        // Its buttons past the right edge.
        assert_eq!(
            fit_to_screen(r(1000, 100, 1440, 900), &laptop),
            Some(r(480, 100, 1440, 900))
        );
        // On no screen at all: centred on the first, the primary.
        assert_eq!(
            fit_to_screen(r(5000, 5000, 1440, 900), &laptop),
            Some(r(240, 66, 1440, 900))
        );
        // Two screens: one wholly on the second stays; one across both stays too.
        let two = [r(0, 0, 1920, 1032), r(1920, 0, 2560, 1400)];
        assert_eq!(fit_to_screen(r(2200, 100, 1600, 1000), &two), None);
        assert_eq!(fit_to_screen(r(1500, 100, 1440, 900), &two), None);
        // Larger than the screen it mostly sits on: fitted to that one, not the primary.
        assert_eq!(
            fit_to_screen(r(2000, 100, 2600, 1300), &two),
            Some(r(1920, 100, 2560, 1300))
        );
        assert_eq!(fit_to_screen(r(0, 0, 100, 100), &[]), None);
    }

    /// Row 27: the window-state file keeps "maximised" for a window closed while
    /// minimised from maximised; nothing else in it changes.
    #[test]
    fn a_window_minimised_from_maximised_is_saved_maximised() {
        let saved = r#"{"main":{"width":1440,"height":900,"x":-8,"y":-8,"prev_x":200,"prev_y":100,"maximized":false,"visible":true,"decorated":false,"fullscreen":false}}"#;
        let fixed: serde_json::Value =
            serde_json::from_str(&mark_maximized(saved, "main").unwrap()).unwrap();
        assert_eq!(fixed["main"]["maximized"], serde_json::Value::Bool(true));
        assert_eq!(fixed["main"]["prev_x"], 200);
        assert_eq!(fixed["main"]["width"], 1440);
        assert_eq!(
            mark_maximized(&fixed.to_string(), "main"),
            None,
            "already maximised"
        );
        assert_eq!(mark_maximized(saved, "other"), None);
        assert_eq!(mark_maximized("not json", "main"), None);
    }

    /// Row 27: the hand-over finds the single-instance plugin's window by the identifier,
    /// which must be tauri.conf.json's.
    #[test]
    fn the_hand_over_uses_the_apps_identifier() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], super::proc::IDENTIFIER);
    }

    use super::{first_size, move_aside, sweep_updater_leftovers, with_suffix};

    /// Row 16: the first window fits the work area it is centred in.
    #[test]
    fn the_first_window_fits_the_screen() {
        let size = tauri::PhysicalSize::new;
        let min = (960.0, 600.0);
        assert_eq!(first_size(size(1920, 1032), 1.0, min), (1440.0, 900.0));
        // 1366×768 with the taskbar: 800 px was taller than the 728 left.
        assert_eq!(first_size(size(1366, 728), 1.0, min), (1229.0, 655.0));
        // 1920×1080 at 150 %: 1280×688 logical.
        assert_eq!(first_size(size(1920, 1032), 1.5, min), (1152.0, 619.0));
        assert_eq!(first_size(size(1024, 600), 1.0, min), (960.0, 600.0));
    }

    /// D-279: the updater's own leftovers go; anything else in the temp folder stays.
    #[test]
    fn only_the_updaters_own_leftovers_are_swept() {
        let product = format!("DzlSweepTest {}", std::process::id());
        let tmp = std::env::temp_dir();
        let ours = tmp.join(format!("{product}-0.1.63-updater-Ab3dEf"));
        let busy = tmp.join(format!("{product}-0.1.64-updater-Xy9zQw"));
        let alike = tmp.join(format!("{product}-notes"));
        for d in [&ours, &busy, &alike] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(ours.join(format!("{product}-0.1.63-installer.exe")), b"MZ").unwrap();
        // Something the updater did not write: the file and so its folder stay.
        std::fs::write(busy.join("other.txt"), b"x").unwrap();
        std::fs::write(alike.join(format!("{product}-0.1.63-installer.exe")), b"MZ").unwrap();

        sweep_updater_leftovers(&product);

        assert!(!ours.exists(), "the leftover setup and its folder go");
        assert!(
            busy.join("other.txt").exists(),
            "a file the updater did not write stays"
        );
        assert!(
            alike
                .join(format!("{product}-0.1.63-installer.exe"))
                .exists(),
            "a folder not shaped like the updater's is not touched"
        );
        for d in [&busy, &alike] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// A damaged cache moves with its -wal and -shm, under the names SQLite pairs them
    /// by; a side file held by another program keeps all three where they were (D-302).
    #[test]
    fn a_damaged_cache_moves_with_its_log_or_not_at_all() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = std::env::temp_dir().join(format!("dzl-aside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("cache.db");
        let put = || {
            std::fs::write(&db, b"not a database").unwrap();
            std::fs::write(with_suffix(&db, "-wal"), b"log").unwrap();
            std::fs::write(with_suffix(&db, "-shm"), b"index").unwrap();
        };

        put();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(with_suffix(&db, "-shm"))
            .unwrap();
        assert!(move_aside(&db).is_err(), "a held -shm refuses the move");
        drop(held);
        for suffix in ["", "-wal", "-shm"] {
            assert!(
                with_suffix(&db, suffix).exists(),
                "cache.db{suffix} was put back"
            );
        }

        let aside = move_aside(&db).unwrap();
        assert!(!db.exists());
        assert_eq!(std::fs::read(&aside).unwrap(), b"not a database");
        assert_eq!(std::fs::read(with_suffix(&aside, "-wal")).unwrap(), b"log");
        assert_eq!(
            std::fs::read(with_suffix(&aside, "-shm")).unwrap(),
            b"index"
        );
        assert!(aside
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cache.db.broken-"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
