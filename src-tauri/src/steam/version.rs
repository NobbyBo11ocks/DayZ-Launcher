//! PE version resource reader via `version.dll`.
//!
//! DayZ_x64.exe reports ProductVersion `1.29.0.163709` (docs/02 §1). The build
//! number 163709 does not fit the 16-bit fields of `VS_FIXEDFILEINFO`, so the
//! authoritative value is the `StringFileInfo\<lang><codepage>\ProductVersion`
//! string, which is also what Explorer and PowerShell show. The fixed block is
//! only a fallback for binaries without a string table.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
};

const VS_FFI_SIGNATURE: u32 = 0xFEEF_04BD;
/// Tried after the translations the file declares (US English with Unicode,
/// Windows-1252 and neutral code pages).
const FALLBACK_TRANSLATIONS: [(u16, u16); 3] = [(0x0409, 0x04B0), (0x0409, 0x04E4), (0x0409, 0x0000)];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileVersion {
    /// `ProductVersion` string, e.g. `1.29.0.163709`.
    pub product: String,
    /// `FileVersion` string, e.g. `1.29`.
    pub file: String,
    /// `VS_FIXEDFILEINFO` product version (16-bit components).
    pub fixed_product: [u16; 4],
}

impl FileVersion {
    /// DayZ's A2S / launcher form: `1.29.0.163709` → `1.29.163709`.
    pub fn game_string(&self) -> String {
        game_form(&self.product)
    }
}

/// Four-part `a.b.c.d` becomes `a.b.d` (DayZ keeps the third component at 0);
/// anything else is returned trimmed and unchanged.
pub fn game_form(product: &str) -> String {
    let p = product.trim();
    let parts: Vec<&str> = p.split('.').collect();
    match parts.as_slice() {
        [a, b, _, d] => format!("{a}.{b}.{d}"),
        _ => p.to_string(),
    }
}

pub fn read(path: &Path) -> Option<FileVersion> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: the block is sized by the API's own answer and outlives every query;
    // returned pointers are read unaligned/copied and never retained.
    unsafe {
        let mut handle = 0u32;
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), &mut handle);
        if size == 0 {
            return None;
        }
        let mut block = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, block.as_mut_ptr().cast::<c_void>()) == 0 {
            return None;
        }

        let fixed = query(&block, "\\").and_then(|(ptr, len)| {
            if (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>() {
                return None;
            }
            let info = std::ptr::read_unaligned(ptr.cast::<VS_FIXEDFILEINFO>());
            (info.dwSignature == VS_FFI_SIGNATURE).then_some(info)
        });

        let mut translations: Vec<(u16, u16)> = Vec::new();
        if let Some((ptr, len)) = query(&block, "\\VarFileInfo\\Translation") {
            let pairs = len as usize / 4;
            let p = ptr.cast::<u16>();
            for i in 0..pairs {
                let lang = std::ptr::read_unaligned(p.add(i * 2));
                let cp = std::ptr::read_unaligned(p.add(i * 2 + 1));
                translations.push((lang, cp));
            }
        }
        translations.extend(FALLBACK_TRANSLATIONS);

        let string_value = |name: &str| -> Option<String> {
            translations.iter().find_map(|(lang, cp)| {
                let sub = format!("\\StringFileInfo\\{lang:04X}{cp:04X}\\{name}");
                let (ptr, len) = query(&block, &sub)?;
                let mut buf = vec![0u16; len as usize];
                std::ptr::copy_nonoverlapping(ptr.cast::<u16>(), buf.as_mut_ptr(), len as usize);
                let s = String::from_utf16_lossy(&buf);
                let s = s.trim_end_matches('\0').trim().to_string();
                (!s.is_empty()).then_some(s)
            })
        };

        let fixed_product = fixed
            .map(|f| split(f.dwProductVersionMS, f.dwProductVersionLS))
            .unwrap_or([0; 4]);
        let product = string_value("ProductVersion").unwrap_or_else(|| join(fixed_product));
        let file = string_value("FileVersion").unwrap_or_else(|| {
            fixed.map(|f| join(split(f.dwFileVersionMS, f.dwFileVersionLS))).unwrap_or_default()
        });
        if product.is_empty() && fixed.is_none() {
            return None;
        }
        Some(FileVersion {
            product,
            file,
            fixed_product,
        })
    }
}

unsafe fn query(block: &[u8], sub: &str) -> Option<(*mut c_void, u32)> {
    let w: Vec<u16> = sub.encode_utf16().chain(std::iter::once(0)).collect();
    let mut ptr: *mut c_void = std::ptr::null_mut();
    let mut len = 0u32;
    // SAFETY: caller guarantees `block` is a complete version-info block.
    let ok = unsafe { VerQueryValueW(block.as_ptr().cast::<c_void>(), w.as_ptr(), &mut ptr, &mut len) };
    (ok != 0 && !ptr.is_null() && len > 0).then_some((ptr, len))
}

fn split(ms: u32, ls: u32) -> [u16; 4] {
    [(ms >> 16) as u16, (ms & 0xFFFF) as u16, (ls >> 16) as u16, (ls & 0xFFFF) as u16]
}

fn join(v: [u16; 4]) -> String {
    format!("{}.{}.{}.{}", v[0], v[1], v[2], v[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_form_drops_third_component() {
        assert_eq!(game_form("1.29.0.163709"), "1.29.163709");
        assert_eq!(game_form(" 1.29.163709 "), "1.29.163709");
        assert_eq!(game_form("1.29"), "1.29");
    }

    #[test]
    fn reads_a_system_binary() {
        let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let v = read(Path::new(&sys).join("System32").join("kernel32.dll").as_path()).expect("kernel32 has a version resource");
        assert!(!v.product.is_empty());
        assert!(v.fixed_product[0] >= 6, "Windows major version");
    }
}
