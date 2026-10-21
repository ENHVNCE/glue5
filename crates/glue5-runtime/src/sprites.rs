//! What host code draws over the game's frames (mods' images, rectangles
//! and text), handed to the renderer a frame at a time. Positions are
//! fractions of the screen; the renderer draws the latest list into the
//! window's image after the game's frame is scaled to it (upscaled and
//! generated frames too), under the application's own UI.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::Locked;

/// An image to draw: RGBA, 8 bits a channel, sRGB values with the alpha
/// premultiplied. Its pixels go to the GPU the first time it is drawn and
/// are dropped here then.
pub struct Image {
    id: u64,
    width: u32,
    height: u32,
    pixels: Mutex<Option<Vec<u8>>>,
}

impl Image {
    /// `pixels`: `width` × `height` × 4 bytes.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Arc<Image> {
        assert_eq!(pixels.len(), width as usize * height as usize * 4, "an image's pixels");
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        Arc::new(Image { id, width, height, pixels: Mutex::new(Some(pixels)) })
    }

    /// Unique for the process's life.
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// What it takes on the GPU (and here until then).
    pub fn bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }

    /// The pixels, for the upload (none after the first call).
    pub fn take_pixels(&self) -> Option<Vec<u8>> {
        self.pixels.locked().take()
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        // The renderer frees its texture.
        DROPPED.fetch_add(1, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone)]
pub enum Shape {
    /// Centred on `center`, `size` wide and high; a height of 0 keeps the
    /// image's proportions.
    Image(Arc<Image>),
    /// Centred on `center`, `size` wide and high.
    Rect,
    /// From `center` (its top; which end is there, `align` says); its
    /// height is `size[1]`.
    Text(String, Align),
}

#[derive(Clone)]
pub struct Sprite {
    pub shape: Shape,
    /// Fractions of the screen: 0, 0 is its top left, 1, 1 its bottom right.
    pub center: [f32; 2],
    /// Fractions of the screen's width and height.
    pub size: [f32; 2],
    /// Degrees, clockwise, around `center`.
    pub rotation: f32,
    /// RGBA, not premultiplied: an image's pixels are multiplied by it.
    pub color: [u8; 4],
}

/// The latest list, in drawing order (none: nothing to draw).
static LATEST: Mutex<Option<Arc<Vec<Sprite>>>> = Mutex::new(None);
/// Changes with the list.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Images dropped so far.
static DROPPED: AtomicU64 = AtomicU64::new(0);
/// The window's image, in pixels (width << 32 | height), as last drawn.
static SCREEN: AtomicU64 = AtomicU64::new(0);

/// Replaces what is drawn, from the next frame shown (an empty list draws
/// nothing).
pub fn publish(sprites: Vec<Sprite>) {
    let mut latest = LATEST.locked();
    if sprites.is_empty() && latest.is_none() {
        return;
    }
    *latest = (!sprites.is_empty()).then(|| Arc::new(sprites));
    GENERATION.fetch_add(1, Ordering::Release);
}

/// The list to draw.
pub fn latest() -> Option<Arc<Vec<Sprite>>> {
    LATEST.locked().clone()
}

/// Changes whenever [`latest`] would give another list.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// Changes whenever an image is dropped (its texture can go).
pub fn dropped() -> u64 {
    DROPPED.load(Ordering::Acquire)
}

/// The renderer's: the size of the window's image the sprites go on.
pub fn set_screen(width: u32, height: u32) {
    SCREEN.store(u64::from(width) << 32 | u64::from(height), Ordering::Relaxed);
}

/// The size of the window's image, in pixels (0, 0 before a frame is shown).
pub fn screen() -> (u32, u32) {
    let s = SCREEN.load(Ordering::Relaxed);
    ((s >> 32) as u32, s as u32)
}
