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

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, SetPriorityClass,
    BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// Browsing: keep the launcher snappy.
    High,
    /// DayZ is running: stay out of its way.
    BelowNormal,
}

/// Sets this process's priority class; failures are ignored (best effort).
pub fn set_priority(p: Priority) {
    let class = match p {
        Priority::High => HIGH_PRIORITY_CLASS,
        Priority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
    };
    // SAFETY: the pseudo-handle of the current process is always valid.
    unsafe {
        let _ = SetPriorityClass(GetCurrentProcess(), class);
    }
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

/// What elevation matching decided at start-up, for Diagnostics and the join plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
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
