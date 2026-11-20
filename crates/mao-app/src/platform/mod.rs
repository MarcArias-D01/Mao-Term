//! OS-specific window tweaks (kept out of the rest of the app).

#[cfg(windows)]
mod windows;

/// Lets the system backdrop (Acrylic/Mica) cover the whole window instead of
/// only its (hidden) frame. No-op elsewhere.
pub fn extend_backdrop(window: &winit::window::Window) {
    #[cfg(windows)]
    windows::extend_frame_into_client_area(window);
    #[cfg(not(windows))]
    let _ = window;
}
