//! Steam client location and liveness (docs/02 §2).
//!
//! * `HKCU\Software\Valve\Steam` → `SteamPath` (forward slashes, lower case), `SteamExe`
//! * `HKLM\SOFTWARE\WOW6432Node\Valve\Steam` → `InstallPath` (fallback)
//! * `HKCU\Software\Valve\Steam\ActiveProcess` → `pid`, `ActiveUser` (0 when logged out)
//!
//! `ActiveProcess\pid` is **not** reliable: on 2026-09-21 it read 16884 while the
//! live `steam.exe` was 15176 (D-026). Liveness therefore comes from enumerating
//! processes and checking the image path against `SteamPath`.

use std::path::{Path, PathBuf};

use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
use winreg::RegKey;

#[derive(Debug, Clone)]
pub struct SteamInfo {
    pub path: Option<PathBuf>,
    pub exe: Option<PathBuf>,
    /// Which hive supplied `path`: "HKCU", "HKLM" or "none".
    pub source: &'static str,
    /// PID of the live `steam.exe` (0 when not running).
    pub pid: u32,
    /// What `ActiveProcess\pid` says; may be stale.
    pub registry_pid: u32,
    pub active_user: u32,
    pub running: bool,
}

pub fn detect() -> SteamInfo {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let mut info = SteamInfo {
        path: None,
        exe: None,
        source: "none",
        pid: 0,
        registry_pid: 0,
        active_user: 0,
        running: false,
    };

    if let Ok(key) = hkcu.open_subkey_with_flags(r"Software\Valve\Steam", KEY_READ) {
        if let Some(p) = read_path(&key, "SteamPath") {
            info.path = Some(p);
            info.source = "HKCU";
        }
        info.exe = read_path(&key, "SteamExe");
    }

    if info.path.is_none() {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        if let Ok(key) = hklm.open_subkey_with_flags(r"SOFTWARE\WOW6432Node\Valve\Steam", KEY_READ) {
            if let Some(p) = read_path(&key, "InstallPath") {
                info.exe = Some(p.join("steam.exe"));
                info.path = Some(p);
                info.source = "HKLM";
            }
        }
    }

    if let Ok(key) = hkcu.open_subkey_with_flags(r"Software\Valve\Steam\ActiveProcess", KEY_READ) {
        info.registry_pid = key.get_value::<u32, _>("pid").unwrap_or(0);
        info.active_user = key.get_value::<u32, _>("ActiveUser").unwrap_or(0);
    }

    if let Some(pid) = process::find_steam(info.path.as_deref()) {
        info.pid = pid;
        info.running = true;
    }
    info
}

fn read_path(key: &RegKey, name: &str) -> Option<PathBuf> {
    let raw: String = key.get_value(name).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(normalize(Path::new(&trimmed.replace('/', "\\"))))
}

/// Canonicalises when possible (restores the real casing of `c:/program files (x86)/steam`)
/// and strips the `\\?\` verbatim prefix so the value is fit for display and for
/// building child paths.
pub fn normalize(p: &Path) -> PathBuf {
    match std::fs::canonicalize(p) {
        Ok(c) => strip_verbatim(&c),
        Err(_) => p.to_path_buf(),
    }
}

pub fn strip_verbatim(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => p.to_path_buf(),
    }
}

mod process {
    use std::path::Path;

    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// PID of a live `steam.exe`. When `steam_path` is known the image must live
    /// under it, so an unrelated binary called steam.exe does not count.
    pub fn find_steam(steam_path: Option<&Path>) -> Option<u32> {
        let want_prefix = steam_path.map(|p| p.to_string_lossy().to_ascii_lowercase());
        for pid in pids_named("steam.exe") {
            match (&want_prefix, image_path(pid)) {
                (None, _) => return Some(pid),
                (Some(prefix), Some(img)) if img.to_ascii_lowercase().starts_with(prefix.as_str()) => return Some(pid),
                _ => continue,
            }
        }
        None
    }

    fn pids_named(exe: &str) -> Vec<u32> {
        let mut out = Vec::new();
        // SAFETY: standard ToolHelp enumeration; the snapshot handle is closed on every path.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE || snap.is_null() {
                return out;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                    let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                    if name.eq_ignore_ascii_case(exe) {
                        out.push(entry.th32ProcessID);
                    }
                    if Process32NextW(snap, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
        }
        out
    }

    fn image_path(pid: u32) -> Option<String> {
        // SAFETY: handle is owned and closed; buffer length is passed in/out as documented.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) != 0;
            CloseHandle(h);
            ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
        }
    }
}
