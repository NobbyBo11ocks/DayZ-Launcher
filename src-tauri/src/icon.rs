//! The window's icons, loaded from the exe's own icon group at the sizes Windows asks
//! for (D-264).
//!
//! Tauri hands tao one image — the ICO's first entry, decoded at build time — and tao
//! sets it as the window's *small* icon only (S-85). The taskbar and Alt-Tab draw the
//! *big* icon, so they scaled that one 32 px picture to whatever they needed, and the
//! taskbar button came out soft (Q31). The exe carries the whole ICO as a resource
//! (tauri-build embeds it under id 32512, S-88), so `LoadImageW` can take the entry made
//! for each size: 32, 40 or 48 px for the big icon at 100, 125 or 150 % scaling, 16, 20
//! or 24 for the small one.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    LoadImageW, SendMessageW, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, SM_CXICON,
    SM_CXSMICON, WM_SETICON,
};

/// tauri-build's resource id for the app icon (`set_icon_with_id(.., "32512")`, S-88).
const APP_ICON_ID: usize = 32512;

/// Sets the window's big and small icons from the exe's icon group at the window's DPI.
/// Called after the window is built and again when its scale changes; the handles are
/// kept for the life of the window, as `WM_SETICON` requires, so they are never freed.
pub fn apply(hwnd: HWND) {
    // SAFETY: `hwnd` is a live window of this process; every call below takes plain
    // values and returns a handle or 0, which is checked before use.
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        if module.is_null() {
            return;
        }
        let dpi = match GetDpiForWindow(hwnd) {
            0 => 96,
            d => d,
        };
        for (which, metric) in [(ICON_BIG, SM_CXICON), (ICON_SMALL, SM_CXSMICON)] {
            let size = GetSystemMetricsForDpi(metric, dpi);
            let icon = LoadImageW(
                module,
                APP_ICON_ID as *const u16,
                IMAGE_ICON,
                size,
                size,
                LR_DEFAULTCOLOR,
            );
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, which as usize, icon as isize);
            }
        }
    }
}
