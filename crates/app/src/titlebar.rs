//! Paints the native title bar in the theme's colors. Uses the DWM
//! attributes added in Windows 11; on older systems the calls fail
//! harmlessly and the default title bar stays.

#[cfg(windows)]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::Color;

#[cfg(windows)]
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
#[cfg(windows)]
const DWMWA_BORDER_COLOR: u32 = 34;
#[cfg(windows)]
const DWMWA_CAPTION_COLOR: u32 = 35;
#[cfg(windows)]
const DWMWA_TEXT_COLOR: u32 = 36;

#[cfg(windows)]
#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(
        hwnd: isize,
        attribute: u32,
        value: *const core::ffi::c_void,
        size: u32,
    ) -> i32;
}

/// Colors the title bar of `window`. Returns false if the native window
/// doesn't exist yet (it is created shortly after the window is shown).
#[cfg(not(windows))]
pub fn paint(_: &slint::Window, _: Color, _: Color, _: Color, _: bool) -> bool {
    // The window manager draws the title bar; nothing to retry.
    true
}

#[cfg(windows)]
pub fn paint(
    window: &slint::Window,
    caption: Color,
    text: Color,
    border: Color,
    dark: bool,
) -> bool {
    let handle = window.window_handle();
    let Ok(handle) = handle.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd = win32.hwnd.get();
    set(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, i32::from(dark));
    set(hwnd, DWMWA_CAPTION_COLOR, colorref(caption));
    set(hwnd, DWMWA_TEXT_COLOR, colorref(text));
    set(hwnd, DWMWA_BORDER_COLOR, colorref(border));
    true
}

#[cfg(windows)]
fn set<T>(hwnd: isize, attribute: u32, value: T) {
    // SAFETY: `hwnd` is a live window handle from Slint, and `value` is a
    // plain integer whose size we pass, as DwmSetWindowAttribute expects.
    // Failure (e.g. on Windows 10) only means the attribute isn't applied.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute,
            (&value as *const T).cast(),
            std::mem::size_of::<T>() as u32,
        );
    }
}

/// COLORREF is 0x00BBGGRR.
#[cfg_attr(not(windows), allow(dead_code))]
fn colorref(color: Color) -> u32 {
    u32::from(color.red()) | u32::from(color.green()) << 8 | u32::from(color.blue()) << 16
}
