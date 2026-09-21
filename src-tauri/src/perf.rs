//! Self-measurement for Diagnostics (D-078): start-up time to the first painted list,
//! and the private memory / CPU time of the host process plus its WebView2 helper
//! processes, i.e. the same numbers the release budgets in docs/05 §6 are measured
//! with. Pure Win32 (ToolHelp + psapi), no extra crates.

use std::sync::OnceLock;

use serde::Serialize;
use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::ProcessStatus::{
    K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

static FIRST_PAINT_MS: OnceLock<u64> = OnceLock::new();

/// Records the moment the UI first showed server rows; only the first call counts.
pub fn mark_first_paint() {
    let _ = FIRST_PAINT_MS.set(crate::uptime_ms() as u64);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerfSample {
    pub uptime_ms: u64,
    /// Milliseconds from process start to the first frame with rows, if reported.
    pub first_paint_ms: Option<u64>,
    pub host_private_bytes: u64,
    /// Kernel + user CPU time of the host process.
    pub host_cpu_ms: u64,
    pub webview_private_bytes: u64,
    pub webview_processes: u32,
    pub total_private_bytes: u64,
}

pub fn sample() -> PerfSample {
    // SAFETY: pseudo-handle from GetCurrentProcess needs no closing.
    let me = unsafe { GetCurrentProcess() };
    let host_private_bytes = private_bytes(me);
    let host_cpu_ms = cpu_ms(me);

    let (webview_private_bytes, webview_processes) = webview_descendants();
    let uptime_ms = crate::uptime_ms() as u64;
    PerfSample {
        uptime_ms,
        first_paint_ms: FIRST_PAINT_MS.get().copied(),
        host_private_bytes,
        host_cpu_ms,
        webview_private_bytes,
        webview_processes,
        total_private_bytes: host_private_bytes + webview_private_bytes,
    }
}

/// `PrivateUsage` (commit charge) of a process, 0 when the query fails.
fn private_bytes(h: HANDLE) -> u64 {
    let mut c: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
    c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    // SAFETY: the EX struct starts with the base struct; the API accepts the larger
    // buffer when `cb` says so.
    let ok = unsafe {
        K32GetProcessMemoryInfo(h, &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS, c.cb)
    };
    if ok == 0 {
        0
    } else {
        c.PrivateUsage as u64
    }
}

fn cpu_ms(h: HANDLE) -> u64 {
    let mut t: [FILETIME; 4] = unsafe { std::mem::zeroed() };
    // SAFETY: four valid out-pointers into the array.
    let ok = unsafe { GetProcessTimes(h, &mut t[0], &mut t[1], &mut t[2], &mut t[3]) };
    if ok == 0 {
        return 0;
    }
    let as_u64 = |f: &FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    (as_u64(&t[2]) + as_u64(&t[3])) / 10_000 // 100 ns units → ms
}

/// Private bytes and count of every `msedgewebview2.exe` descended from any
/// `dayz-launcher.exe`. WebView2 shares one browser process per user data folder, so
/// a second launcher instance attaches to the first one's processes; walking from
/// every launcher pid keeps the count right whichever instance asks.
fn webview_descendants() -> (u64, u32) {
    let mine = unsafe { GetCurrentProcessId() };
    // (pid, parent pid, is a WebView2 process, is a launcher process)
    let mut procs: Vec<(u32, u32, bool, bool)> = Vec::new();
    // SAFETY: standard ToolHelp enumeration; the snapshot handle is closed on every path.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE || snap.is_null() {
            return (0, 0);
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut e) != 0 {
            loop {
                let len = e
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(e.szExeFile.len());
                let name = String::from_utf16_lossy(&e.szExeFile[..len]);
                procs.push((
                    e.th32ProcessID,
                    e.th32ParentProcessID,
                    name.eq_ignore_ascii_case("msedgewebview2.exe"),
                    name.eq_ignore_ascii_case("dayz-launcher.exe"),
                ));
                if Process32NextW(snap, &mut e) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    // Breadth-first over the parent links from every launcher process.
    let mut wanted: Vec<u32> = procs
        .iter()
        .filter(|p| p.3 || p.0 == mine)
        .map(|p| p.0)
        .collect();
    let mut i = 0;
    let (mut bytes, mut count) = (0u64, 0u32);
    while i < wanted.len() {
        let parent = wanted[i];
        i += 1;
        for &(pid, ppid, is_webview, _) in &procs {
            if ppid != parent || wanted.contains(&pid) {
                continue;
            }
            wanted.push(pid);
            if !is_webview {
                continue;
            }
            // SAFETY: handle is closed right after the query.
            unsafe {
                let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                if !h.is_null() {
                    bytes += private_bytes(h);
                    count += 1;
                    CloseHandle(h);
                }
            }
        }
    }
    (bytes, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_numbers_are_sane() {
        let s = sample();
        assert!(
            s.host_private_bytes > 1 << 20,
            "own private bytes {}",
            s.host_private_bytes
        );
        assert!(s.host_private_bytes < 8 << 30);
        assert_eq!(
            s.total_private_bytes,
            s.host_private_bytes + s.webview_private_bytes
        );
        assert!(s.first_paint_ms.is_none());
        mark_first_paint();
        mark_first_paint();
        assert!(sample().first_paint_ms.is_some());
    }
}
