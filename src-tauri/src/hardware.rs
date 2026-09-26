//! This PC's processor threads, memory and video memory, and the DayZ performance
//! arguments made from them (D-267).
//!
//! DayZ 1.29 still parses three performance limits: `cpucount`, `maxmem=` and
//! `maxVRAM` (the executable's own option table, S-90). The launch adds them for the
//! machine it runs on, unless the player's extra arguments already set one — theirs
//! always wins. Read once per process: none of it changes while the launcher runs.

use std::sync::OnceLock;

use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{GetActiveProcessorCount, ALL_PROCESSOR_GROUPS};

use crate::launch::args::split_extra;

/// Memory left to Windows and everything else running beside the game. `-maxMem` is
/// the point where DayZ starts trimming its own caches; set to all of it, an 8 or
/// 16 GB machine would page other programs out first.
const RAM_RESERVE_MB: u64 = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hardware {
    /// Hardware threads across every processor group (a 16-core, SMT CPU is 32).
    pub threads: u32,
    /// Physical memory, MB.
    pub ram_mb: u64,
    /// Dedicated memory of the largest hardware GPU, MB, when one was found.
    pub vram_mb: Option<u64>,
}

/// The machine's hardware, read on first use.
pub fn detect() -> Hardware {
    static HW: OnceLock<Hardware> = OnceLock::new();
    *HW.get_or_init(|| {
        let hw = Hardware {
            threads: threads(),
            ram_mb: ram_mb(),
            vram_mb: vram_mb(),
        };
        crate::log_info!(
            "launch",
            "hardware: {} thread(s), {} MB RAM, {} VRAM",
            hw.threads,
            hw.ram_mb,
            hw.vram_mb
                .map_or_else(|| "unknown".to_string(), |v| format!("{v} MB"))
        );
        hw
    })
}

/// The performance arguments for `hw`, leaving out any the player set in `extra`.
pub fn launch_args(hw: &Hardware, extra: &str) -> Vec<String> {
    let set: Vec<String> = split_extra(extra)
        .iter()
        .map(|a| {
            a.split('=')
                .next()
                .unwrap_or("")
                .trim_start_matches('-')
                .to_ascii_lowercase()
        })
        .collect();
    let mine = |key: &str| !set.iter().any(|k| k == key);
    let mut out = Vec::with_capacity(3);
    if mine("cpucount") && hw.threads > 0 {
        out.push(format!("-cpuCount={}", hw.threads));
    }
    if mine("maxmem") && hw.ram_mb > RAM_RESERVE_MB * 2 {
        out.push(format!("-maxMem={}", hw.ram_mb - RAM_RESERVE_MB));
    }
    if let (true, Some(v)) = (mine("maxvram"), hw.vram_mb) {
        out.push(format!("-maxVRAM={v}"));
    }
    out
}

fn threads() -> u32 {
    // SAFETY: takes a constant and returns a count.
    let n = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) };
    if n > 0 {
        n
    } else {
        std::thread::available_parallelism().map_or(0, |n| n.get() as u32)
    }
}

fn ram_mb() -> u64 {
    // SAFETY: the struct is zeroed, its length set as the call requires, and only read
    // after the call reports success.
    unsafe {
        let mut ms: MEMORYSTATUSEX = std::mem::zeroed();
        ms.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut ms) == 0 {
            return 0;
        }
        ms.ullTotalPhys >> 20
    }
}

/// Through DXGI: WMI's `AdapterRAM` is 32-bit and stops at 4 GB, so an 8 GB card
/// reads as 4 there. DXGI's figure is what the driver leaves usable (8 018 MB of an
/// RTX 3070's 8 192), the same one the game sees. The software adapter (Microsoft
/// Basic Render Driver) is skipped, and the largest remaining one is the card DayZ
/// renders on in any normal setup. Under 2 GB the figure is usually an integrated
/// GPU's small reserved slice of system memory rather than what it can use, so DayZ
/// keeps its own estimate there.
fn vram_mb() -> Option<u64> {
    // SAFETY: plain DXGI enumeration; every interface is released when dropped.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
    let mut best = 0u64;
    for i in 0.. {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else {
            break;
        };
        if let Ok(desc) = unsafe { adapter.GetDesc1() } {
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 {
                best = best.max(desc.DedicatedVideoMemory as u64);
            }
        }
    }
    let mb = best >> 20;
    (mb >= 2048).then_some(mb)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PC: Hardware = Hardware {
        threads: 32,
        ram_mb: 32_674,
        vram_mb: Some(8_192),
    };

    #[test]
    fn all_three_for_an_empty_extra() {
        assert_eq!(
            launch_args(&PC, ""),
            vec!["-cpuCount=32", "-maxMem=30626", "-maxVRAM=8192"]
        );
    }

    #[test]
    fn the_players_own_values_win() {
        // Any case, any value: a key the player set is left to them.
        assert_eq!(
            launch_args(&PC, r#"-CPUCOUNT=8 -profiles="D:\P""#),
            vec!["-maxMem=30626", "-maxVRAM=8192"]
        );
        assert!(launch_args(&PC, "-cpuCount=32 -maxMem=32674 -maxVRAM=8192").is_empty());
    }

    #[test]
    fn nothing_it_could_not_read() {
        let bare = Hardware {
            threads: 0,
            ram_mb: 3_000,
            vram_mb: None,
        };
        assert!(launch_args(&bare, "").is_empty());
    }

    #[test]
    fn this_machine_reads_as_a_real_one() {
        let hw = detect();
        println!("{hw:?} -> {:?}", launch_args(&hw, ""));
        assert!(hw.threads >= 1, "{hw:?}");
        assert!(hw.ram_mb >= 1024, "{hw:?}");
    }
}
