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

/// RGBA, not premultiplied.
pub type Rgba = [u8; 4];

/// A line along an outline, or one drawn by itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub color: Rgba,
    /// A fraction of the screen's height.
    pub width: f32,
}

/// What is drawn. Positions are fractions of the screen (0, 0 its top
/// left, 1, 1 its bottom right); lengths that are not along x or y (radii,
/// line widths, rounding, text sizes) fractions of its height. Rotations
/// are degrees, clockwise.
#[derive(Clone)]
pub enum Shape {
    /// `size` wide and high (a height of 0 keeps the image's proportions),
    /// its pixels multiplied by `tint`.
    Image { image: Arc<Image>, center: [f32; 2], size: [f32; 2], rotation: f32, tint: Rgba },
    /// Filled with `fill`, or from `fill` to `gradient.0` top to bottom (left
    /// to right when `gradient.1`).
    Rect {
        center: [f32; 2],
        size: [f32; 2],
        rotation: f32,
        fill: Rgba,
        gradient: Option<(Rgba, bool)>,
        rounding: f32,
        stroke: Option<Stroke>,
    },
    /// From `at` (its top; which end of it is there, `align` says), `size`
    /// high.
    Text { text: Arc<str>, at: [f32; 2], size: f32, rotation: f32, align: Align, color: Rgba },
    Line { from: [f32; 2], to: [f32; 2], stroke: Stroke },
    /// The list's `points[points]`; a closed one may be filled (convex).
    Path { points: std::ops::Range<u32>, closed: bool, fill: Option<Rgba>, stroke: Option<Stroke> },
    Circle { center: [f32; 2], radius: f32, fill: Option<Rgba>, stroke: Option<Stroke> },
    /// From `from` to `to` degrees, clockwise from the top.
    Arc { center: [f32; 2], radius: f32, from: f32, to: f32, stroke: Stroke },
}

#[derive(Clone)]
pub struct Sprite {
    pub shape: Shape,
}

/// What is drawn in a frame, in drawing order. Kept and reused: a list the
/// renderer is done with comes back from [`spare`].
#[derive(Default)]
pub struct List {
    pub sprites: Vec<Sprite>,
    /// The paths' points.
    pub points: Vec<[f32; 2]>,
}

impl List {
    pub fn clear(&mut self) {
        self.sprites.clear();
        self.points.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.sprites.is_empty()
    }
}

/// The latest list (none: nothing to draw), and the one before, for reuse.
static LATEST: Mutex<Option<Arc<List>>> = Mutex::new(None);
static SPARE: Mutex<Option<Arc<List>>> = Mutex::new(None);
/// Changes with the list.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Images dropped so far.
static DROPPED: AtomicU64 = AtomicU64::new(0);
/// The window's image, in pixels (width << 32 | height), as last drawn.
static SCREEN: AtomicU64 = AtomicU64::new(0);

/// Replaces what is drawn, from the next frame shown (an empty list draws
/// nothing).
pub fn publish(list: List) {
    let mut latest = LATEST.locked();
    if list.is_empty() && latest.is_none() {
        return;
    }
    let old = std::mem::replace(&mut *latest, (!list.is_empty()).then(|| Arc::new(list)));
    GENERATION.fetch_add(1, Ordering::Release);
    drop(latest);
    if old.is_some() {
        *SPARE.locked() = old;
    }
}

/// An empty list to fill, the one the renderer was done with if it is
/// (its memory kept).
pub fn spare() -> List {
    let spare = SPARE.locked().take();
    match spare.map(Arc::try_unwrap) {
        Some(Ok(mut list)) => {
            list.clear();
            list
        }
        _ => List::default(),
    }
}

/// The list to draw.
pub fn latest() -> Option<Arc<List>> {
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
