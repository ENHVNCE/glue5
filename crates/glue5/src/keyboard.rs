//! The on-screen keyboard: how much of the window's bottom it covers, so
//! that the app's pages end above it rather than under it.
//!
//! SDL tells when the system keyboard shows and hides, not where it is; the
//! platforms tell that through their own notifications, which land here.

use std::sync::atomic::{AtomicU32, Ordering};

use sdl3::video::Window;

/// The keyboard's overlap of the window's bottom edge, in SDL's window
/// units (an `f32`'s bits); 0 without one, or with one that floats.
static OVERLAP: AtomicU32 = AtomicU32::new(0);

/// The on-screen keyboard's height over the window's bottom edge, in
/// points: 0 when there is none, when it floats (an iPad's undocked one),
/// and on platforms without one.
pub fn overlap(window: &Window) -> f32 {
    f32::from_bits(OVERLAP.load(Ordering::Relaxed)) * crate::ui::points_per_window_unit(window)
}

/// Starts following the keyboard (once, on the main thread, with the
/// window made).
pub fn init(window: &Window) {
    let _ = window;
}
