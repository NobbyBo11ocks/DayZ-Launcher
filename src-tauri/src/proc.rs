//! Process-level policy (D-119): CPU priority class and elevation matching.
//!
//! * Priority: the launcher runs at high priority so the UI and the network
//!   fan-out stay responsive, and drops below normal while DayZ runs so the game
//!   keeps the cores.
//! * Elevation: a game must run at the same integrity as the Steam client, or its
//!   `steam_api` cannot reach Steam ("Unable to locate a running instance of
//!   Steam", S-74). DayZ inherits the launcher's level, so the launcher matches
//!   Steam: if Steam is elevated and we are not, we restart ourselves elevated.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken, SetPriorityClass,
    WaitForSingleObject, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS, INFINITE,
    NORMAL_PRIORITY_CLASS, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, ChangeWindowMessageFilterEx, FindWindowExW, GetWindowThreadProcessId,
    RegisterWindowMessageW, SendMessageTimeoutW, MSGFLT_ALLOW, SMTO_ABORTIFHUNG, SW_SHOWNORMAL,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// Browsing: keep the launcher snappy.
    High,
    /// DayZ is running: stay out of its way.
    BelowNormal,
}

/// Whether the last `set_priority` asked for below normal, for the game watch.
static BELOW_NORMAL: AtomicBool = AtomicBool::new(false);

/// Held while the class changes, so `at_normal_priority` cannot put back a class the game
/// watch changed meanwhile.
static PRIORITY: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn set_class(class: u32) {
    // SAFETY: the pseudo-handle of the current process is always valid.
    unsafe {
        let _ = SetPriorityClass(GetCurrentProcess(), class);
    }
}

/// Sets this process's priority class; failures are ignored (best effort).
pub fn set_priority(p: Priority) {
    let class = match p {
        Priority::High => HIGH_PRIORITY_CLASS,
        Priority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
    };
    let _held = PRIORITY.lock().unwrap_or_else(|e| e.into_inner());
    BELOW_NORMAL.store(p == Priority::BelowNormal, Ordering::SeqCst);
    set_class(class);
}

/// Runs `start`, which starts a program for the player through Windows' shell, at normal
/// priority while the launcher is below normal for a game. A program started then is
/// born below normal and stays so after the game (CreateProcessW, S-127), and the shell
/// takes no priority class to name: a browser opened from a News link while DayZ ran
/// stayed below normal until it was closed (row 28).
pub fn at_normal_priority<T>(start: impl FnOnce() -> T) -> T {
    let _held = PRIORITY.lock().unwrap_or_else(|e| e.into_inner());
    let below = BELOW_NORMAL.load(Ordering::SeqCst);
    if below {
        set_class(NORMAL_PRIORITY_CLASS);
    }
    let started = start();
    if below {
        set_class(BELOW_NORMAL_PRIORITY_CLASS);
    }
    started
}

/// Below normal while DayZ runs however it was started, high again once it has gone,
/// looked at every 30 s (row 27). The step-down came only with a game this instance
/// launched, or one running when it started (D-279): a DayZ started from Steam's Play
/// button or another launcher left it at high priority beside the game, where its
/// checks, the Workshop walk and any Refresh ran above the game's threads, against
/// D-119. The launches' own waits still answer at once; this catches the rest.
pub fn watch_for_game() {
    let _ = std::thread::Builder::new()
        .name("game-watch".into())
        .spawn(|| loop {
            std::thread::sleep(Duration::from_secs(30));
            let find = crate::steam::registry::process::find_named;
            let running = find("DayZ_x64.exe").or_else(|| find(crate::launch::process::BE_EXE));
            let below = BELOW_NORMAL.load(Ordering::SeqCst);
            match running {
                Some(pid) if !below => {
                    set_priority(Priority::BelowNormal);
                    crate::log_info!("app", "DayZ is running (pid {pid}); below normal priority");
                }
                None if below => {
                    set_priority(Priority::High);
                    crate::log_info!("app", "DayZ is not running; back to high priority");
                }
                _ => {}
            }
        });
}

fn token_elevated(process: HANDLE) -> Option<bool> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: `process` is an open handle with query rights; the token handle is
    // closed below; the out buffer matches the size passed.
    unsafe {
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut info = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut info as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        (ok != 0).then_some(info.TokenIsElevated != 0)
    }
}

/// Below normal until the DayZ process `pid` exits, then high again: for an instance
/// started while a game this instance did not launch is running (D-279). A game this
/// instance launches gets the same from `launch_game`, which waits on its own child.
pub fn step_down_while_running(pid: u32) {
    set_priority(Priority::BelowNormal);
    let _ = std::thread::Builder::new()
        .name("dayz-watch".into())
        .spawn(move || {
            // SAFETY: the handle is checked before use and closed after the wait.
            let waited = unsafe {
                let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
                if h.is_null() {
                    false
                } else {
                    WaitForSingleObject(h, INFINITE);
                    CloseHandle(h);
                    true
                }
            };
            if !waited {
                // Not ours to wait on (another session, say): the list shows when it goes.
                while crate::steam::registry::process::find_named("DayZ_x64.exe").is_some() {
                    std::thread::sleep(std::time::Duration::from_secs(15));
                }
            }
            set_priority(Priority::High);
            crate::log_info!("app", "DayZ (pid {pid}) exited; back to high priority");
        });
}

/// Whether this process runs with an elevated (administrator) token.
pub fn current_is_elevated() -> bool {
    // SAFETY: pseudo-handle of the current process.
    unsafe { token_elevated(GetCurrentProcess()) }.unwrap_or(false)
}

/// Whether the process with `pid` runs elevated; `None` when it cannot be queried.
pub fn pid_is_elevated(pid: u32) -> Option<bool> {
    // SAFETY: the handle is closed after the query.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let r = token_elevated(h);
        CloseHandle(h);
        r
    }
}

/// Whether the desktop shell (Explorer) runs elevated, as it does on the built-in
/// Administrator account or with UAC off: every program the player starts then has an
/// administrator token already. `None` when there is no shell window or it cannot be
/// queried.
pub fn shell_is_elevated() -> Option<bool> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};
    // SAFETY: plain queries; a null window is checked before it is used.
    let pid = unsafe {
        let w = GetShellWindow();
        if w.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(w, &mut pid);
        pid
    };
    if pid == 0 {
        return None;
    }
    pid_is_elevated(pid)
}

/// Starts this executable again through the "runas" verb (UAC prompt) with the
/// same arguments. Returns `true` when the new instance was started, so the caller
/// exits; `false` (declined prompt or failure) keeps the current instance.
pub fn relaunch_elevated() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let params = std::env::args()
        .skip(1)
        .map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let wide = |s: &OsStr| -> Vec<u16> { s.encode_wide().chain(std::iter::once(0)).collect() };
    let verb = wide(OsStr::new("runas"));
    let file = wide(exe.as_os_str());
    let params_w = wide(OsStr::new(&params));
    // SAFETY: all strings are NUL-terminated UTF-16 and outlive the call.
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            if params.is_empty() {
                std::ptr::null()
            } else {
                params_w.as_ptr()
            },
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute returns a value above 32 on success.
    (r as usize) > 32
}

/// How the launcher's elevation compares with Steam's: decided at start-up, and again
/// by the join plan against the live Steam process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationState {
    /// Same level as Steam (or Steam is not running).
    Matched,
    /// We run elevated, Steam does not: the game will not find Steam.
    LauncherHigher,
    /// Steam runs elevated, we do not, and the elevated restart was declined or failed.
    SteamHigher,
}

/// Compares our token with Steam's. `steam_pid` 0 means Steam is not running.
pub fn elevation_state(steam_pid: u32) -> ElevationState {
    if steam_pid == 0 {
        return ElevationState::Matched;
    }
    match (current_is_elevated(), pid_is_elevated(steam_pid)) {
        (true, Some(false)) => ElevationState::LauncherHigher,
        (false, Some(true)) => ElevationState::SteamHigher,
        _ => ElevationState::Matched,
    }
}

/// `identifier` in tauri.conf.json, which names the single-instance plugin's window.
pub const IDENTIFIER: &str = "com.dayzlauncher.desktop";

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// The message that asks a running instance to come to the front: registered, so it is
/// the same number in every process, and it carries no data to read.
pub fn show_message() -> u32 {
    static MESSAGE: AtomicU32 = AtomicU32::new(0);
    let known = MESSAGE.load(Ordering::Relaxed);
    if known != 0 {
        return known;
    }
    let name = wide(&format!("{IDENTIFIER}-show"));
    // SAFETY: a NUL-terminated string that outlives the call.
    let message = unsafe { RegisterWindowMessageW(name.as_ptr()) };
    MESSAGE.store(message, Ordering::Relaxed);
    message
}

/// Every top-level window of the single-instance plugin, with the process that owns it
/// (tauri-plugin-single-instance 2.4.5 names it "{identifier}-sic" / "{identifier}-siw").
fn instance_windows() -> Vec<(HWND, u32)> {
    let class = wide(&format!("{IDENTIFIER}-sic"));
    let title = wide(&format!("{IDENTIFIER}-siw"));
    let mut found = Vec::new();
    let mut after: HWND = std::ptr::null_mut();
    // SAFETY: plain window queries with NUL-terminated strings that outlive them; a
    // window that goes meanwhile only ends the walk early.
    unsafe {
        loop {
            after = FindWindowExW(std::ptr::null_mut(), after, class.as_ptr(), title.as_ptr());
            if after.is_null() || found.len() > 8 {
                return found;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(after, &mut pid);
            if pid != 0 {
                found.push((after, pid));
            }
        }
    }
}

/// Hands this launch to a launcher that is already running, and says whether it did
/// (row 27). The plugin looks only while the app is being built, after the elevation
/// decision: with Steam elevated, every shortcut click while the launcher was open asked
/// for administrator rights first, and a launch that went without them met an elevated
/// instance whose mutex it could not open and whose window dropped its message, and ran
/// as a second launcher beside it (D-079). The running one's window is found whatever
/// its elevation and asked to come to the front, with a message that carries nothing.
pub fn hand_to_running_instance() -> bool {
    let message = show_message();
    if message == 0 {
        return false;
    }
    // SAFETY: reads this process's own id.
    let me = unsafe { GetCurrentProcessId() };
    for (hwnd, pid) in instance_windows() {
        if pid == me {
            continue;
        }
        let mut answer: usize = 0;
        // SAFETY: `hwnd` came from the window walk; a window gone since fails the send.
        let sent = unsafe {
            // Windows lets the process the player just started take the foreground, not
            // the one already running: that right goes over with the request.
            AllowSetForegroundWindow(pid);
            SendMessageTimeoutW(hwnd, message, 0, 0, SMTO_ABORTIFHUNG, 3000, &mut answer) != 0
        };
        match handover(sent, answer as isize) {
            Handover::Done => return true,
            Handover::AfterItExits => {
                wait_for_exit(pid, Duration::from_secs(8));
                return false;
            }
            Handover::Start => {}
        }
    }
    false
}

/// What the running instance answers to `show_message`: it came to the front.
pub const SHOWN: isize = 1;
/// What it answers while it is closing: the launch waits for it to go, then starts.
pub const CLOSING: isize = 2;

#[derive(Debug, PartialEq, Eq)]
enum Handover {
    /// The running instance came to the front: this launch ends.
    Done,
    /// It is closing: wait for it, then start (D-326). A launch while the window closed
    /// was answered by an instance about to exit, and neither stayed.
    AfterItExits,
    /// Not handed over: the message did not get through (an older instance behind UIPI,
    /// or a hung one), or an older instance did not know it and answered 0. Start as
    /// usual, where the plugin's own check hands over to an older instance of the same
    /// elevation.
    Start,
}

fn handover(sent: bool, answer: isize) -> Handover {
    match (sent, answer) {
        (true, SHOWN) => Handover::Done,
        (true, CLOSING) => Handover::AfterItExits,
        _ => Handover::Start,
    }
}

/// Waits up to `limit` for the process `pid` to end; a process that cannot be opened is
/// given a short pause instead.
fn wait_for_exit(pid: u32, limit: Duration) {
    // SAFETY: the handle is checked before use and closed after the wait.
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if h.is_null() {
            std::thread::sleep(Duration::from_secs(2));
            return;
        }
        WaitForSingleObject(h, limit.as_millis().min(u128::from(u32::MAX)) as u32);
        CloseHandle(h);
    }
}

/// This instance's own plugin window, which answers `show_message` (row 27).
pub fn own_instance_window() -> Option<HWND> {
    // SAFETY: reads this process's own id.
    let me = unsafe { GetCurrentProcessId() };
    instance_windows()
        .into_iter()
        .find(|(_, pid)| *pid == me)
        .map(|(hwnd, _)| hwnd)
}

/// Lets a process of lower integrity send `show_message` to this instance's window: an
/// elevated instance's window otherwise drops what a launch without elevation posts
/// (UIPI; ChangeWindowMessageFilterEx, S-126). That one message only, and it carries no
/// data.
pub fn accept_show_from_lower_integrity() -> bool {
    own_instance_window().is_some_and(|hwnd| {
        // SAFETY: this process's own window; no filter struct is asked for.
        unsafe {
            ChangeWindowMessageFilterEx(hwnd, show_message(), MSGFLT_ALLOW, std::ptr::null_mut())
                != 0
        }
    })
}

/// Opens `target` through the desktop's own shell (Explorer), so whatever it starts runs
/// as the signed-in player, not with this process's administrator token (row 27): find
/// the desktop's folder view, take its Shell.Application and call
/// `IShellDispatch2::ShellExecute` there (S-125). On a thread of its own in a
/// single-threaded COM apartment, and given ten seconds: every call goes to Explorer,
/// which can be busy or restarting.
pub fn open_as_desktop_user(target: &str) -> Result<(), String> {
    let target = target.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("shell-open".into())
        .spawn(move || {
            let _ = tx.send(shell_execute_in_explorer(&target));
        })
        .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(10))
        .map_err(|_| "Explorer did not answer within 10 s".to_string())?
}

/// The desktop's Shell.Application, reached through its folder view (S-125). Call with
/// COM initialised on this thread, single-threaded.
unsafe fn desktop_shell() -> windows::core::Result<windows::Win32::UI::Shell::IShellDispatch2> {
    use windows::core::Interface;
    use windows::Win32::System::Com::{CoCreateInstance, IDispatch, IServiceProvider, CLSCTX_ALL};
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Shell::{
        IShellBrowser, IShellFolderViewDual, IShellView, IShellWindows, SID_STopLevelBrowser,
        ShellWindows, CSIDL_DESKTOP, SVGIO_BACKGROUND, SWC_DESKTOP, SWFO_NEEDDISPATCH,
    };
    // SAFETY: the caller has initialised COM; every pointer is an interface the calls
    // themselves returned.
    unsafe {
        let shell_windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
        let mut hwnd = 0i32;
        let desktop = shell_windows.FindWindowSW(
            &VARIANT::from(CSIDL_DESKTOP as i32),
            &VARIANT::default(),
            SWC_DESKTOP,
            &mut hwnd,
            SWFO_NEEDDISPATCH,
        )?;
        let browser: IShellBrowser = desktop
            .cast::<IServiceProvider>()?
            .QueryService(&SID_STopLevelBrowser)?;
        let view: IShellView = browser.QueryActiveShellView()?;
        let background: IDispatch = view.GetItemObject(SVGIO_BACKGROUND)?;
        background
            .cast::<IShellFolderViewDual>()?
            .Application()?
            .cast()
    }
}

/// Runs `f` with COM initialised on this thread, single-threaded, and uninitialises it
/// after `f`'s interfaces are gone.
fn with_com<T>(f: impl FnOnce() -> windows::core::Result<T>) -> Result<T, String> {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    // SAFETY: paired with the CoUninitialize below on the same thread; `f` returns before it.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| format!("COM: {e}"))?;
    }
    let r = f().map_err(|e| e.to_string());
    // SAFETY: the matching call for the initialisation above.
    unsafe { CoUninitialize() };
    r
}

fn shell_execute_in_explorer(target: &str) -> Result<(), String> {
    use windows::core::BSTR;
    use windows::Win32::System::Variant::VARIANT;
    with_com(|| {
        // SAFETY: COM is initialised on this thread by `with_com`.
        let shell = unsafe { desktop_shell() }?;
        // Its arguments come in another order than ShellExecute's (S-125).
        // SAFETY: an interface `desktop_shell` returned; the arguments outlive the call.
        unsafe {
            shell.ShellExecute(
                &BSTR::from(target),
                &VARIANT::from(""),
                &VARIANT::from(""),
                &VARIANT::from(""),
                &VARIANT::from(SW_SHOWNORMAL),
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{handover, Handover, CLOSING, SHOWN};

    /// D-326: the running instance's answer decides the launch. A closing one is waited
    /// for, not trusted; one that did not get the message, or did not know it, leaves the
    /// launch to start (and to the plugin's own check).
    #[test]
    fn the_answer_decides_the_launch() {
        assert_eq!(handover(true, SHOWN), Handover::Done);
        assert_eq!(handover(true, CLOSING), Handover::AfterItExits);
        assert_eq!(
            handover(true, 0),
            Handover::Start,
            "an older instance that does not know the message"
        );
        assert_eq!(
            handover(false, 0),
            Handover::Start,
            "blocked, hung, or gone"
        );
        assert_eq!(
            handover(false, SHOWN),
            Handover::Start,
            "an answer that was never sent counts for nothing"
        );
    }

    /// Row 27: the desktop's Shell.Application is reachable, without opening anything.
    /// Ignored in CI, whose runner has no desktop shell: `cargo test -- --ignored
    /// the_desktop_shell` on a desktop.
    #[test]
    #[ignore]
    fn the_desktop_shell_is_reachable() {
        std::thread::spawn(|| super::with_com(|| unsafe { super::desktop_shell() }.map(|_| ())))
            .join()
            .unwrap()
            .unwrap();
    }
}
