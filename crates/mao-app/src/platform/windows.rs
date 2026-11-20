//! Windows: DWM calls winit doesn't expose.

use windows_sys::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows_sys::Win32::UI::Controls::MARGINS;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub fn extend_frame_into_client_area(window: &winit::window::Window) {
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return;
    };
    // -1 on every side: "sheet of glass" over the whole client area.
    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    // SAFETY: `h.hwnd` is the live window handle owned by `window` (borrowed for
    // this call), and `margins` is a valid, initialized struct on the stack.
    #[allow(unsafe_code)]
    let hr = unsafe { DwmExtendFrameIntoClientArea(h.hwnd.get() as _, &margins) };
    if hr != 0 {
        tracing::debug!(hr, "DwmExtendFrameIntoClientArea failed");
    }
}
