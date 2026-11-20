//! OS-specific window tweaks (kept out of the rest of the app).

#[cfg(windows)]
mod windows;

/// Switches the Windows 11 backdrop material at runtime. No-op elsewhere.
pub fn set_backdrop(window: &winit::window::Window, backdrop: mao_config::Backdrop) {
    #[cfg(windows)]
    {
        use winit::platform::windows::{BackdropType, WindowExtWindows};
        window.set_system_backdrop(match backdrop {
            mao_config::Backdrop::Acrylic => BackdropType::TransientWindow,
            mao_config::Backdrop::Mica => BackdropType::MainWindow,
            mao_config::Backdrop::None => BackdropType::None,
        });
    }
    #[cfg(not(windows))]
    let _ = (window, backdrop);
}

/// Lets the system backdrop (Acrylic/Mica) cover the whole window instead of
/// only its (hidden) frame. No-op elsewhere.
pub fn extend_backdrop(window: &winit::window::Window) {
    #[cfg(windows)]
    windows::extend_frame_into_client_area(window);
    #[cfg(not(windows))]
    let _ = window;
}
