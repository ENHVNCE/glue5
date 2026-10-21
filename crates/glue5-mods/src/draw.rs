//! `draw` in Luau: what ScriptHookV's createTexture/drawTexture gave, and
//! more. A mod loads images (PNG) from its folder, read and decoded on a
//! thread of their own, and draws them, shapes and text over the game, in
//! fractions of the screen.
//!
//! The natives draw rectangles, the game's sprites and text in the game's
//! own frame (DRAW_RECT, DRAW_SPRITE, the text commands); these are drawn
//! by the renderer over the frame once it is scaled to the window
//! (glue5-gpu's sprites.rs), so at the window's resolution, above whatever
//! the game drew, below the app's own UI.
//!
//! Calls are the hot path (thousands a frame for a busy HUD): their
//! arguments are positional numbers, a colour one packed number, and the
//! rest a style made once (`draw.style{...}`, userdata) and passed again.
//! Measured on an M1 Pro, release, 100,000 calls of a rectangle: an
//! options table written in the call costs 425 ns a call, the same table
//! made once 332 ns (its keys are looked up by string), a packed colour
//! and a style 104 ns, a packed colour alone 65 ns.
//!
//! What a mod draws in a turn shows from the end of that turn until the end
//! of its next whole one (a turn cut short by the mod's time running out
//! goes on in the next, its drawings with it: no half a frame on screen).
//! At each engine frame, when a mod drew anew, the mods' drawings become
//! one list, by z, handed to the renderer (engine.rs). Records, the list
//! and the texts are reused from frame to frame. A mod's images are its
//! run's: they go with its VM.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

use glue5_runtime::sprites::{self, Align, Group, Image, NONE, Rgba, Shape, Sprite, Stroke};
use mlua::{AnyUserData, Function, Lua, Table, UserData, UserDataFields, UserDataMethods, Value};

/// An image's side at most, in pixels: what every GPU takes.
pub const SIDE: u32 = 2048;
/// An image file's size at most.
const FILE: u64 = 16 << 20;
/// Images a run of a mod may hold, those loading included.
pub const IMAGES: usize = 64;
/// What they may take, decoded (4 bytes a pixel, as on the GPU).
pub const IMAGE_BYTES: usize = 32 << 20;
/// Drawings a mod may make in a turn, and paths' points.
pub const DRAWINGS: usize = 16384;
pub const POINTS: usize = 65536;
/// Groups a mod may open in a turn.
pub const GROUPS: usize = 4096;
/// A text's length at most, in bytes (past it, it is cut).
const TEXT: usize = 1024;
/// Lines' width when none is given (a fraction of the screen's height:
/// two pixels at 1080).
const WIDTH: f64 = 0.002;

const MB: usize = 1 << 20;

/// What a mod's VM keeps for `draw` (in its app data).
#[derive(Default)]
pub(crate) struct Canvas {
    /// The mod's folder (none: the console, which has none).
    module: RefCell<Option<String>>,
    budget: Rc<Budget>,
    /// What this turn draws, and what the last whole turn drew (shown).
    rec: RefCell<Record>,
    ready: RefCell<Record>,
    /// The groups open (`draw.push`, `draw.clip`), innermost last.
    open: RefCell<Vec<u32>>,
    /// `ready` changed since it was published.
    committed: Cell<bool>,
    /// Texts by the address of their Luau string.
    texts: RefCell<HashMap<usize, Text>>,
}

#[derive(Default)]
struct Record {
    /// With their z.
    items: Vec<(i32, Sprite)>,
    points: Vec<[f32; 2]>,
    groups: Vec<Group>,
}

struct Text {
    text: Arc<str>,
    /// Drawn since the last whole turn.
    used: bool,
}

/// The images a run of a mod holds, and their bytes.
#[derive(Default)]
struct Budget {
    images: Cell<usize>,
    bytes: Cell<usize>,
}

/// An image, as a mod holds it; `release` or the mod's end frees it.
struct LuaImage {
    image: RefCell<Option<Arc<Image>>>,
    size: (u32, u32),
    budget: Rc<Budget>,
}

impl LuaImage {
    fn release(&self) {
        if let Some(image) = self.image.borrow_mut().take() {
            self.budget.images.set(self.budget.images.get() - 1);
            self.budget.bytes.set(self.budget.bytes.get() - image.bytes());
        }
    }
}

impl Drop for LuaImage {
    fn drop(&mut self) {
        self.release();
    }
}

impl UserData for LuaImage {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("width", |_, i| Ok(i.size.0));
        fields.add_field_method_get("height", |_, i| Ok(i.size.1));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("release", |_, i, ()| {
            i.release();
            Ok(())
        });
    }
}

/// What the image thread made of a file.
type Loaded = Arc<Mutex<Option<Result<Arc<Image>, String>>>>;

/// An image being loaded: it counts among the mod's images until it is
/// in (or failed, or the mod stopped waiting for it).
struct Load {
    path: String,
    result: Loaded,
    budget: Rc<Budget>,
    pending: Cell<bool>,
}

impl Load {
    fn settle(&self) {
        if self.pending.replace(false) {
            self.budget.images.set(self.budget.images.get() - 1);
        }
    }
}

impl Drop for Load {
    fn drop(&mut self) {
        self.settle();
    }
}

impl UserData for Load {}

/// A file to read and decode, with the room left in its mod's budget.
struct Job {
    file: PathBuf,
    room: usize,
    result: Loaded,
}

/// The thread that reads and decodes the mods' images, away from the
/// game's.
fn images_thread() -> Result<&'static Sender<Job>, String> {
    static JOBS: OnceLock<Sender<Job>> = OnceLock::new();
    if let Some(jobs) = JOBS.get() {
        return Ok(jobs);
    }
    let (tx, rx) = std::sync::mpsc::channel::<Job>();
    glue5_runtime::qos::spawn("mod images", glue5_runtime::qos::Qos::Utility, move || {
        for job in rx {
            let loaded = std::panic::catch_unwind(|| load_file(&job.file, job.room))
                .unwrap_or_else(|_| Err("the image could not be decoded".into()));
            *crate::lock(&job.result) = Some(loaded);
        }
    })
    .map_err(|e| format!("no thread for images: {e}"))?;
    Ok(JOBS.get_or_init(|| tx))
}

fn load_file(file: &std::path::Path, room: usize) -> Result<Arc<Image>, String> {
    let len = std::fs::metadata(file).map_err(|e| e.to_string())?.len();
    if len > FILE {
        return Err(format!("{} MB: image files of {} MB at most", len as usize / MB, FILE as usize / MB));
    }
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    let (width, height, pixels) = decode_png(&bytes, room)?;
    Ok(Image::new(width, height, pixels))
}

/// A PNG as premultiplied RGBA, refused before it is decoded when larger
/// than [`SIDE`] or than `room` bytes.
fn decode_png(bytes: &[u8], room: usize) -> Result<(u32, u32, Vec<u8>), String> {
    let error = |e: png::DecodingError| format!("not a PNG image the game can read ({e})");
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(bytes), png::Limits { bytes: 2 * IMAGE_BYTES });
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let info = decoder.read_header_info().map_err(error)?;
    let (width, height) = (info.width, info.height);
    if width > SIDE || height > SIDE {
        return Err(format!("{width}×{height}: images of {SIDE}×{SIDE} at most"));
    }
    let need = Image::bytes_for(width, height);
    if need > room {
        return Err(format!(
            "{width}×{height} takes {:.1} MB, more than is left of the {} MB a mod's images may take",
            need as f64 / MB as f64,
            IMAGE_BYTES / MB
        ));
    }
    let mut reader = decoder.read_info().map_err(error)?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or("an image too large")?];
    let frame = reader.next_frame(&mut buffer).map_err(error)?;
    let (color, _) = reader.output_color_type();
    let channels = color.samples();
    let mut rgba = Vec::with_capacity(need);
    for row in buffer[..frame.buffer_size()].chunks_exact(frame.line_size) {
        for p in row[..width as usize * channels].chunks_exact(channels) {
            let (r, g, b, a) = match *p {
                [v] => (v, v, v, 255),
                [v, a] => (v, v, v, a),
                [r, g, b] => (r, g, b, 255),
                [r, g, b, a] => (r, g, b, a),
                _ => unreachable!("8-bit samples"),
            };
            // Premultiplied, as the renderer blends (sRGB values, as egui).
            let m = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
            rgba.extend_from_slice(&[m(r), m(g), m(b), a]);
        }
    }
    Ok((width, height, rgba))
}

/// Adds `draw` to a mod's VM (its images come from its folder, once
/// [`set_folder`] says which).
pub fn install(lua: &Lua) -> mlua::Result<()> {
    let canvas = Rc::new(Canvas::default());
    let draw = lua.create_table()?;

    let c = canvas.clone();
    let start = lua.create_function(move |_, path: String| {
        let module = c.module.borrow().clone().ok_or_else(|| {
            mlua::Error::runtime("draw.loadImage: images come from a mod's folder (the console has none)")
        })?;
        let file = crate::files::resolve(&format!("{module}/{path}"))
            .map_err(|_| mlua::Error::runtime(format!("draw.loadImage(\"{path}\"): a file of the mod, as \"images/logo.png\"")))?;
        if !path.to_ascii_lowercase().ends_with(".png") {
            return Err(mlua::Error::runtime(format!("draw.loadImage(\"{path}\"): PNG images only")));
        }
        let budget = &c.budget;
        if budget.images.get() >= IMAGES {
            return Err(mlua::Error::runtime(format!(
                "draw.loadImage(\"{path}\"): a mod may hold {IMAGES} images (release the ones it no longer draws)"
            )));
        }
        let result: Loaded = Arc::default();
        let room = IMAGE_BYTES - budget.bytes.get();
        images_thread()
            .and_then(|jobs| jobs.send(Job { file, room, result: result.clone() }).map_err(|_| "the image thread stopped".into()))
            .map_err(|e| mlua::Error::runtime(format!("draw.loadImage(\"{path}\"): {e}")))?;
        budget.images.set(budget.images.get() + 1);
        Ok(Load { path, result, budget: budget.clone(), pending: Cell::new(true) })
    })?;
    let poll = lua.create_function(|lua, load: AnyUserData| {
        let load = load.borrow::<Load>()?;
        if !load.pending.get() {
            return Err(mlua::Error::runtime("draw.loadImage: loaded already"));
        }
        let loaded = crate::lock(&load.result).take();
        let path = &load.path;
        match loaded {
            None => Ok(Value::Nil),
            Some(Err(e)) => {
                load.settle();
                Err(mlua::Error::runtime(format!("draw.loadImage(\"{path}\"): {e}")))
            }
            Some(Ok(image)) => {
                let budget = &load.budget;
                // Others may have come in meanwhile.
                if budget.bytes.get() + image.bytes() > IMAGE_BYTES {
                    load.settle();
                    return Err(mlua::Error::runtime(format!(
                        "draw.loadImage(\"{path}\"): the mod's images would take more than {} MB",
                        IMAGE_BYTES / MB
                    )));
                }
                // It stays among the mod's images, as this one.
                load.pending.set(false);
                budget.bytes.set(budget.bytes.get() + image.bytes());
                let size = image.size();
                let image = LuaImage { image: RefCell::new(Some(image)), size, budget: budget.clone() };
                Ok(Value::UserData(lua.create_userdata(image)?))
            }
        }
    })?;
    // Waits as `wait()` does, a frame at a time, while the image thread
    // works.
    let load: Function = lua
        .load(
            "local start, poll, yield = ...\n\
             return function(path)\n\
             local load = start(path)\n\
             while true do\n\
             local image = poll(load)\n\
             if image then return image end\n\
             yield(0)\n\
             end end",
        )
        .set_name("=draw.loadImage")
        .call((start, poll, lua.globals().get::<Table>("coroutine")?.get::<Function>("yield")?))?;
    draw.set("loadImage", load)?;

    let c = canvas.clone();
    draw.set(
        "image",
        lua.create_function(
            move |_, (image, x, y, w, h, color, style): (AnyUserData, f64, f64, f64, Option<f64>, Option<f64>, Option<AnyUserData>)| {
                let image = image
                    .borrow::<LuaImage>()
                    .map_err(|_| mlua::Error::runtime("draw.image: an image from draw.loadImage"))?
                    .image
                    .borrow()
                    .clone()
                    .ok_or_else(|| mlua::Error::runtime("draw.image: the image was released"))?;
                let s = Style::of(style)?;
                let shape = Shape::Image {
                    image,
                    center: [fraction(x)?, fraction(y)?],
                    size: [fraction(w)?.max(0.0), fraction(h.unwrap_or(0.0))?.max(0.0)],
                    rotation: s.rotation,
                    tint: rgba(color)?,
                    uv: s.uv,
                    nine: s.nine,
                };
                c.add(&s, shape)
            },
        )?,
    )?;
    let c = canvas.clone();
    draw.set(
        "rect",
        lua.create_function(
            move |_, (x, y, w, h, color, style): (f64, f64, f64, f64, Option<f64>, Option<AnyUserData>)| {
                let s = Style::of(style)?;
                let fill = match (color, s.stroke) {
                    // An outline alone.
                    (None, Some(_)) => [0; 4],
                    _ => rgba(color)?,
                };
                let shape = Shape::Rect {
                    center: [fraction(x)?, fraction(y)?],
                    size: [fraction(w)?.max(0.0), fraction(h)?.max(0.0)],
                    rotation: s.rotation,
                    fill,
                    gradient: s.gradient.map(|g| (g, s.horizontal)),
                    rounding: s.rounding,
                    stroke: s.outline(),
                };
                c.add(&s, shape)
            },
        )?,
    )?;
    let c = canvas.clone();
    draw.set(
        "text",
        lua.create_function(
            move |_, (text, x, y, size, color, style): (mlua::LuaString, f64, f64, Option<f64>, Option<f64>, Option<AnyUserData>)| {
                let s = Style::of(style)?;
                let shape = Shape::Text {
                    text: c.text(&text),
                    at: [fraction(x)?, fraction(y)?],
                    size: length(size.unwrap_or(0.03))?.clamp(0.001, 1.0),
                    rotation: s.rotation,
                    align: s.align,
                    color: rgba(color)?,
                };
                c.add(&s, shape)
            },
        )?,
    )?;
    let c = canvas.clone();
    draw.set(
        "line",
        lua.create_function(
            move |_, (x1, y1, x2, y2, width, color, style): (f64, f64, f64, f64, Option<f64>, Option<f64>, Option<AnyUserData>)| {
                let s = Style::of(style)?;
                let stroke = Stroke { color: rgba(color)?, width: length(width.unwrap_or(WIDTH))? };
                c.add(&s, Shape::Line { from: [fraction(x1)?, fraction(y1)?], to: [fraction(x2)?, fraction(y2)?], stroke })
            },
        )?,
    )?;
    let c = canvas.clone();
    draw.set(
        "path",
        lua.create_function(move |_, (points, width, color, style): (Table, Option<f64>, Option<f64>, Option<AnyUserData>)| {
            let s = Style::of(style)?;
            let n = points.raw_len();
            if n % 2 != 0 || n < 4 {
                return Err(mlua::Error::runtime("draw.path: points are {x1, y1, x2, y2, ...}, two at least"));
            }
            let stroke = match (color, s.fill) {
                // A filled outline alone.
                (None, Some(_)) => None,
                _ => Some(Stroke { color: rgba(color)?, width: length(width.unwrap_or(WIDTH))? }),
            };
            let range = c.points(n / 2, |i| Ok([fraction(points.raw_get(2 * i + 1)?)?, fraction(points.raw_get(2 * i + 2)?)?]))?;
            c.add(&s, Shape::Path { points: range, closed: s.closed || s.fill.is_some(), fill: s.fill, stroke })
        })?,
    )?;
    let c = canvas.clone();
    draw.set(
        "circle",
        lua.create_function(move |_, (x, y, r, color, style): (f64, f64, f64, Option<f64>, Option<AnyUserData>)| {
            let s = Style::of(style)?;
            let fill = match (color, s.stroke) {
                (None, Some(_)) => None,
                _ => Some(rgba(color)?),
            };
            c.add(&s, Shape::Circle { center: [fraction(x)?, fraction(y)?], radius: length(r)?.max(0.0), fill, stroke: s.outline() })
        })?,
    )?;
    let c = canvas.clone();
    draw.set(
        "arc",
        lua.create_function(
            move |_, (x, y, r, from, to, width, color, style): (f64, f64, f64, f64, f64, Option<f64>, Option<f64>, Option<AnyUserData>)| {
                let s = Style::of(style)?;
                let stroke = Stroke { color: rgba(color)?, width: length(width.unwrap_or(WIDTH))? };
                let (from, to) = (finite(from)? as f32, finite(to)? as f32);
                c.add(&s, Shape::Arc { center: [fraction(x)?, fraction(y)?], radius: length(r)?.max(0.0), from, to, stroke })
            },
        )?,
    )?;
    let c = canvas.clone();
    draw.set(
        "push",
        lua.create_function(move |_, (x, y, style): (f64, f64, Option<AnyUserData>)| {
            let s = Style::of(style)?;
            c.push([fraction(x)?, fraction(y)?], s.scale, s.rotation, None)
        })?,
    )?;
    let c = canvas.clone();
    draw.set(
        "clip",
        lua.create_function(move |_, (x, y, w, h): (f64, f64, f64, f64)| {
            let clip = [fraction(x)?, fraction(y)?, fraction(w)?.max(0.0), fraction(h)?.max(0.0)];
            c.push([0.0, 0.0], 1.0, 0.0, Some(clip))
        })?,
    )?;
    let c = canvas.clone();
    draw.set(
        "pop",
        lua.create_function(move |_, ()| match c.open.borrow_mut().pop() {
            Some(_) => Ok(()),
            None => Err(mlua::Error::runtime("draw.pop: no group open (draw.push, draw.clip)")),
        })?,
    )?;
    draw.set(
        "rgba",
        lua.create_function(|_, (r, g, b, a): (f64, f64, f64, Option<f64>)| {
            let byte = |v: f64| finite(v).map(|v| v.clamp(0.0, 255.0).round() as u32);
            Ok(f64::from(byte(r)? << 24 | byte(g)? << 16 | byte(b)? << 8 | byte(a.unwrap_or(255.0))?))
        })?,
    )?;
    draw.set("style", lua.create_function(|_, t: Option<Table>| Style::from_table(t.as_ref()))?)?;
    draw.set(
        "screenSize",
        lua.create_function(|_, ()| {
            let (w, h) = sprites::screen();
            Ok((w, h))
        })?,
    )?;
    lua.set_app_data(canvas);
    lua.globals().set("draw", draw)
}

/// The mod's folder, where `draw.loadImage` reads.
pub fn set_folder(lua: &Lua, module: &str) {
    if let Some(canvas) = lua.app_data_ref::<Rc<Canvas>>() {
        *canvas.module.borrow_mut() = Some(module.to_string());
    }
}

/// A mod's drawings, for [`publish`].
pub(crate) fn canvas(lua: &Lua) -> Option<Rc<Canvas>> {
    lua.app_data_ref::<Rc<Canvas>>().map(|c| c.clone())
}

/// A mod's turn ended; `whole`: it ran to its waits (not stopped short by
/// its time running out). Then what it drew becomes what it shows, until
/// its next whole turn; else it goes on drawing in its next turn, and the
/// last whole turn's drawings stay (no half a frame on screen).
pub fn end_turn(lua: &Lua, whole: bool) {
    let Some(c) = lua.app_data_ref::<Rc<Canvas>>() else { return };
    if !whole {
        return;
    }
    let (mut rec, mut ready) = (c.rec.borrow_mut(), c.ready.borrow_mut());
    std::mem::swap(&mut *rec, &mut *ready);
    rec.items.clear();
    rec.points.clear();
    rec.groups.clear();
    // Groups left open close with the turn.
    c.open.borrow_mut().clear();
    c.committed.set(true);
    c.texts.borrow_mut().retain(|_, t| std::mem::take(&mut t.used));
}

/// What publishing keeps between frames (on the engine's thread), so that
/// it allocates nothing once warm.
#[derive(Default)]
struct Publisher {
    /// The canvases of this frame and those published last (by address).
    current: Vec<Rc<Canvas>>,
    published: Vec<usize>,
    /// (z, canvas, item), sorted; where each canvas's points and groups
    /// start.
    order: Vec<(i32, u32, u32)>,
    base: Vec<(u32, u32)>,
}

thread_local! {
    static PUBLISHER: RefCell<Publisher> = RefCell::default();
}

/// The running mods' drawings for the renderer, when one of them drew
/// anew or the mods changed: in one list, by z, then in the order drawn.
pub fn publish(canvases: impl IntoIterator<Item = Rc<Canvas>>) {
    PUBLISHER.with_borrow_mut(|p| {
        p.current.clear();
        p.current.extend(canvases);
        let mut changed = p.current.len() != p.published.len();
        for (c, &at) in p.current.iter().zip(&p.published) {
            changed |= Rc::as_ptr(c) as usize != at;
        }
        for c in &p.current {
            changed |= c.committed.take();
        }
        if !changed {
            p.current.clear();
            return;
        }
        p.published.clear();
        p.published.extend(p.current.iter().map(|c| Rc::as_ptr(c) as usize));
        p.order.clear();
        p.base.clear();
        let mut list = sprites::spare();
        for (ci, c) in p.current.iter().enumerate() {
            let ready = c.ready.borrow();
            p.order.extend(ready.items.iter().enumerate().map(|(i, (z, _))| (*z, ci as u32, i as u32)));
            // Each mod's points and groups after the ones before.
            let (points, groups) = (list.points.len() as u32, list.groups.len() as u32);
            p.base.push((points, groups));
            list.points.extend_from_slice(&ready.points);
            list.groups.extend(ready.groups.iter().map(|g| Group { parent: shift(g.parent, groups), ..*g }));
        }
        // Unstable is enough: (canvas, item) make each key unique, in order.
        p.order.sort_unstable();
        for &(_, ci, i) in &p.order {
            let ready = p.current[ci as usize].ready.borrow();
            let item = &ready.items[i as usize].1;
            let (points, groups) = p.base[ci as usize];
            let mut shape = item.shape.clone();
            if let Shape::Path { points: range, .. } = &mut shape {
                *range = range.start + points..range.end + points;
            }
            list.sprites.push(Sprite { shape, additive: item.additive, group: shift(item.group, groups) });
        }
        p.current.clear();
        sprites::publish(list);
    });
}

/// A group index moved by `base` (none stays none).
fn shift(group: u32, base: u32) -> u32 {
    if group == NONE { NONE } else { group + base }
}

/// The innermost open group.
fn c_top(open: &RefCell<Vec<u32>>) -> u32 {
    open.borrow().last().copied().unwrap_or(NONE)
}

impl Canvas {
    /// Opens a group in the one open.
    fn push(&self, origin: [f32; 2], scale: f32, rotation: f32, clip: Option<[f32; 4]>) -> mlua::Result<()> {
        let mut rec = self.rec.borrow_mut();
        if rec.groups.len() >= GROUPS {
            return Err(mlua::Error::runtime(format!("draw: {GROUPS} groups a turn at most")));
        }
        let parent = c_top(&self.open);
        rec.groups.push(Group { parent, origin, scale, rotation, clip });
        self.open.borrow_mut().push(rec.groups.len() as u32 - 1);
        Ok(())
    }

    fn add(&self, s: &Style, shape: Shape) -> mlua::Result<()> {
        let mut rec = self.rec.borrow_mut();
        if rec.items.len() >= DRAWINGS {
            return Err(mlua::Error::runtime(format!("draw: {DRAWINGS} drawings a turn at most")));
        }
        let group = c_top(&self.open);
        rec.items.push((s.z, Sprite { shape, additive: s.additive, group }));
        Ok(())
    }

    /// `n` points from `point(i)`, kept for a path.
    /// (`point` reads raw table slots: no Luau runs meanwhile.)
    fn points(&self, n: usize, mut point: impl FnMut(usize) -> mlua::Result<[f32; 2]>) -> mlua::Result<std::ops::Range<u32>> {
        let mut rec = self.rec.borrow_mut();
        if rec.points.len() + n > POINTS {
            return Err(mlua::Error::runtime(format!("draw.path: {POINTS} points a turn at most")));
        }
        let start = rec.points.len();
        for i in 0..n {
            match point(i) {
                Ok(p) => rec.points.push(p),
                Err(e) => {
                    rec.points.truncate(start);
                    return Err(e);
                }
            }
        }
        Ok(start as u32..rec.points.len() as u32)
    }

    /// A text as the renderer keeps it: the same string as in an earlier
    /// call is the same text, not a copy.
    fn text(&self, s: &mlua::LuaString) -> Arc<str> {
        let bytes = s.as_bytes();
        let bytes = &bytes[..bytes.len().min(TEXT)];
        let mut texts = self.texts.borrow_mut();
        let key = s.to_pointer() as usize;
        if let Some(t) = texts.get_mut(&key)
            && t.text.as_bytes() == bytes
        {
            t.used = true;
            return t.text.clone();
        }
        let text: Arc<str> = String::from_utf8_lossy(bytes).into();
        texts.insert(key, Text { text: text.clone(), used: true });
        text
    }
}

/// What a drawing has besides where it is: made once (`draw.style{...}`)
/// and passed with each call, its fields changed as needed.
#[derive(Clone, Copy)]
struct Style {
    z: i32,
    rotation: f32,
    rounding: f32,
    stroke: Option<Rgba>,
    width: f32,
    fill: Option<Rgba>,
    closed: bool,
    gradient: Option<Rgba>,
    horizontal: bool,
    align: Align,
    scale: f32,
    uv: [f32; 4],
    nine: Option<[f32; 4]>,
    additive: bool,
}

const PLAIN: Style = Style {
    z: 0,
    rotation: 0.0,
    rounding: 0.0,
    stroke: None,
    width: WIDTH as f32,
    fill: None,
    closed: false,
    gradient: None,
    horizontal: false,
    align: Align::Left,
    scale: 1.0,
    uv: [0.0, 0.0, 1.0, 1.0],
    nine: None,
    additive: false,
};

impl Style {
    fn of(style: Option<AnyUserData>) -> mlua::Result<Style> {
        match style {
            None => Ok(PLAIN),
            Some(u) => u.borrow::<Style>().map(|s| *s).map_err(|_| mlua::Error::runtime("draw: a style from draw.style")),
        }
    }

    fn outline(&self) -> Option<Stroke> {
        self.stroke.map(|color| Stroke { color, width: self.width })
    }

    fn from_table(t: Option<&Table>) -> mlua::Result<Style> {
        let mut s = PLAIN;
        if let Some(t) = t {
            for pair in t.pairs::<String, Value>() {
                let (k, v) = pair?;
                s.set(&k, v)?;
            }
        }
        Ok(s)
    }

    fn set(&mut self, key: &str, v: Value) -> mlua::Result<()> {
        let bad = |what: &str| mlua::Error::runtime(format!("draw.style: {key} is {what}"));
        let num = |v: &Value| match *v {
            Value::Integer(i) => Some(i as f64),
            Value::Number(n) if n.is_finite() => Some(n),
            _ => None,
        };
        let color = |v: &Value| match v {
            Value::Nil => Ok(None),
            v => num(v).ok_or_else(|| bad("a colour (draw.rgba)")).and_then(|c| rgba(Some(c)).map(Some)),
        };
        let flag = |v: &Value| match v {
            Value::Nil => Ok(false),
            Value::Boolean(b) => Ok(*b),
            _ => Err(bad("true or false")),
        };
        match key {
            "z" => self.z = num(&v).ok_or_else(|| bad("a number"))?.clamp(-1e6, 1e6).round() as i32,
            "rotation" => self.rotation = (num(&v).ok_or_else(|| bad("a number"))? % 360.0) as f32,
            "scale" => self.scale = num(&v).ok_or_else(|| bad("a number"))?.clamp(0.0, 1e3) as f32,
            "rounding" => self.rounding = length(num(&v).ok_or_else(|| bad("a number"))?)?.max(0.0),
            "width" => self.width = length(num(&v).ok_or_else(|| bad("a number"))?)?.max(0.0),
            "stroke" => self.stroke = color(&v)?,
            "fill" => self.fill = color(&v)?,
            "gradient" => self.gradient = color(&v)?,
            "closed" => self.closed = flag(&v)?,
            "uv" | "nineSlice" => {
                let four = match &v {
                    Value::Nil => None,
                    Value::Table(t) if t.raw_len() == 4 => {
                        let mut q = [0.0f32; 4];
                        for (i, x) in q.iter_mut().enumerate() {
                            *x = num(&t.raw_get(i + 1)?).ok_or_else(|| bad("four numbers"))? as f32;
                        }
                        Some(q)
                    }
                    _ => return Err(bad("four numbers")),
                };
                if key == "uv" {
                    self.uv = four.map_or(PLAIN.uv, |q| q.map(|x| x.clamp(-16.0, 16.0)));
                } else {
                    self.nine = four.map(|q| q.map(|x| x.clamp(0.0, SIDE as f32)));
                }
            }
            "blend" => {
                self.additive = match &v {
                    Value::Nil => false,
                    Value::String(s) if s == "alpha" => false,
                    Value::String(s) if s == "add" => true,
                    _ => return Err(bad("\"alpha\" or \"add\"")),
                }
            }
            "horizontal" => self.horizontal = flag(&v)?,
            "align" => {
                self.align = match &v {
                    Value::Nil => Align::Left,
                    Value::String(s) if s == "left" => Align::Left,
                    Value::String(s) if s == "center" => Align::Center,
                    Value::String(s) if s == "right" => Align::Right,
                    _ => return Err(bad("\"left\", \"center\" or \"right\"")),
                }
            }
            _ => return Err(mlua::Error::runtime(format!("draw.style: no {key}"))),
        }
        Ok(())
    }
}

/// A style's fields, as `draw.style{...}` and assignments name them.
const STYLE_FIELDS: &[&str] = &["z", "rotation", "scale", "rounding", "width", "stroke", "fill", "gradient", "closed", "horizontal", "align", "uv", "nineSlice", "blend"];

impl UserData for Style {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        for &key in STYLE_FIELDS {
            fields.add_field_method_set(key, move |_, s, v: Value| s.set(key, v));
        }
        fields.add_field_method_get("z", |_, s| Ok(s.z));
        fields.add_field_method_get("rotation", |_, s| Ok(s.rotation));
        fields.add_field_method_get("scale", |_, s| Ok(s.scale));
        fields.add_field_method_get("rounding", |_, s| Ok(s.rounding));
        fields.add_field_method_get("width", |_, s| Ok(s.width));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // A sprite sheet's frame, without a table a frame.
        methods.add_method_mut("setUv", |_, s, (u0, v0, u1, v1): (f64, f64, f64, f64)| {
            s.uv = [fraction(u0)?, fraction(v0)?, fraction(u1)?, fraction(v1)?];
            Ok(())
        });
    }
}

/// A packed colour (0xRRGGBBAA; none: white).
fn rgba(c: Option<f64>) -> mlua::Result<Rgba> {
    match c {
        None => Ok([255; 4]),
        Some(c) if c.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&c) => Ok((c as u32).to_be_bytes()),
        Some(_) => Err(mlua::Error::runtime("draw: a colour is 0xRRGGBBAA (draw.rgba)")),
    }
}

fn finite(v: f64) -> mlua::Result<f64> {
    if v.is_finite() { Ok(v) } else { Err(mlua::Error::runtime("draw: not a number")) }
}

/// A position or size in fractions of the screen: within a few screens.
fn fraction(v: f64) -> mlua::Result<f32> {
    Ok(finite(v)?.clamp(-16.0, 16.0) as f32)
}

/// A length in fractions of the screen's height.
fn length(v: f64) -> mlua::Result<f32> {
    fraction(v)
}

/// The definitions of `draw`, for the type checker.
pub const DEFINITIONS: &str = "
-- An image a mod loaded (draw.loadImage); its size in pixels.
export type Image = {
\twidth: number,
\theight: number,
\t-- Frees it now (else when nothing holds it, or when the mod stops); it is no longer drawn.
\trelease: (self: Image) -> (),
}
-- What a drawing has besides where it is: made once with draw.style{...}, passed with each call,
-- its fields changed as needed (style.rotation = angle).
export type Style = {
\t-- Higher in front (0 by default); equal ones in the order drawn.
\tz: number,
\t-- Degrees, clockwise, around a rectangle's or image's centre, a text's start.
\trotation: number,
\t-- A rectangle's corners' radius (a fraction of the screen's height).
\trounding: number,
\t-- An outline's colour (a rectangle's, a circle's), and its width (0.002 by default).
\tstroke: Color?,
\twidth: number,
\t-- A path's fill (a convex outline); it closes the path.
\tfill: Color?,
\tclosed: boolean,
\t-- A rectangle from its colour at the top to this one at the bottom (at the right when horizontal).
\tgradient: Color?,
\thorizontal: boolean,
\t-- \"add\": added to what is under it (light, glows); \"alpha\" by default.
\tblend: \"alpha\" | \"add\",
\t-- The part of an image drawn: {u0, v0, u1, v1}, fractions of its size (a sprite sheet's frame;
\t-- style:setUv(u0, v0, u1, v1) changes it without a table).
\tuv: { number }?,
\t-- An image cut nine ways: {left, top, right, bottom} borders in its pixels. The corners keep
\t-- their size (as on a 1080-pixel-high screen), the edges and the middle stretch: panels of any size.
\tnineSlice: { number }?,
\tsetUv: (self: Style, u0: number, v0: number, u1: number, v1: number) -> (),
\t-- A group's scale (draw.push).
\tscale: number,
\t-- Which end of a text is at x.
\talign: \"left\" | \"center\" | \"right\",
}
export type StyleOptions = {
\tz: number?, rotation: number?, scale: number?, rounding: number?, stroke: Color?, width: number?, fill: Color?, closed: boolean?,
\tgradient: Color?, horizontal: boolean?, align: (\"left\" | \"center\" | \"right\")?, uv: { number }?, nineSlice: { number }?, blend: (\"alpha\" | \"add\")?,
}

-- Images, shapes and text over the game. What a turn of the mod draws shows until its next turn:
-- draw every frame (a loop with wait()) to keep things on screen. Positions and sizes are fractions of
-- the screen (0, 0 its top left, 1, 1 its bottom right), rectangles and images centred as DRAW_RECT's;
-- radii, widths, rounding and text sizes fractions of its height. Drawn at the window's resolution, over
-- everything the game draws, under the app's own controls. Colours are packed (0xRRGGBBAA, white
-- when none); the rest goes in a style, made once.
declare draw: {
\t-- An image from the mod's folder (\"images/logo.png\"): PNG, 2048×2048 at most. Waits, as wait() does,
\t-- while it is read and decoded away from the game. A mod holds 64 images, 32 MB of them
\t-- (4 bytes a pixel), at most; they go when it stops.
\tloadImage: (path: string) -> Image,
\t-- `image` centred at x, y, width by height (none: the image's proportions), its colours multiplied by `color`.
\timage: (image: Image, x: number, y: number, width: number, height: number?, color: Color?, style: Style?) -> (),
\t-- A rectangle centred at x, y; with no colour and a style's stroke, its outline alone.
\trect: (x: number, y: number, width: number, height: number, color: Color?, style: Style?) -> (),
\t-- `text` from x, y (its top), `size` high (0.03 by default).
\ttext: (text: string, x: number, y: number, size: number?, color: Color?, style: Style?) -> (),
\tline: (x1: number, y1: number, x2: number, y2: number, width: number?, color: Color?, style: Style?) -> (),
\t-- A line through {x1, y1, x2, y2, ...}; closed, or filled (convex), as the style says.
\tpath: (points: { number }, width: number?, color: Color?, style: Style?) -> (),
\t-- A disc; with no colour and a style's stroke, its outline alone.
\tcircle: (x: number, y: number, radius: number, color: Color?, style: Style?) -> (),
\t-- An arc from `from` to `to` degrees, clockwise from the top (a gauge).
\tarc: (x: number, y: number, radius: number, from: number, to: number, width: number?, color: Color?, style: Style?) -> (),
\t-- Opens a group at x, y: what is drawn until draw.pop() is placed from there (its 0, 0 there),
\t-- scaled and turned by the style's scale and rotation. Groups nest; those left open close with the turn.
\tpush: (x: number, y: number, style: Style?) -> (),
\t-- Opens a group that shows only what falls in this rectangle (centred at x, y), until draw.pop().
\tclip: (x: number, y: number, width: number, height: number) -> (),
\tpop: () -> (),
\t-- A packed colour, 0 to 255 each (alpha 255 when none).
\trgba: (r: number, g: number, b: number, a: number?) -> Color,
\tstyle: (options: StyleOptions?) -> Style,
\t-- The screen's size in pixels (the window's, where these are drawn), for proportions.
\tscreenSize: () -> (number, number),
}
";

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Once};

    use glue5_runtime::sprites::{self, Shape};
    use mlua::{Function, Lua, MultiValue, StdLib};

    use super::*;

    fn png(width: u32, height: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(data).unwrap();
        out
    }

    /// A mods folder with mod `drawtest` and its images.
    fn folder() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let dir = std::env::temp_dir().join(format!("glue5-draw-test-{}", std::process::id()));
            let images = dir.join("drawtest/images");
            std::fs::create_dir_all(&images).unwrap();
            std::fs::write(dir.join("drawtest/main.luau"), "").unwrap();
            std::fs::write(images.join("a.png"), png(16, 8, png::ColorType::Rgba, &[200; 16 * 8 * 4])).unwrap();
            std::fs::write(images.join("big.png"), png(2049, 1, png::ColorType::Grayscale, &[0; 2049])).unwrap();
            std::fs::write(images.join("bad.png"), "not a png").unwrap();
            crate::open(&dir);
        });
    }

    fn vm() -> Lua {
        folder();
        let lua = Lua::new_with(StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING, Default::default()).unwrap();
        install(&lua).unwrap();
        set_folder(&lua, "drawtest");
        lua
    }

    /// Runs `code` as a mod's coroutine, a frame every millisecond, until
    /// it ends; then its turn ends whole.
    fn run(lua: &Lua, code: &str) -> mlua::Result<MultiValue> {
        let f: Function = lua.load(code).into_function()?;
        let thread = lua.create_thread(f)?;
        for _ in 0..5000 {
            let values = thread.resume::<MultiValue>(());
            if values.is_err() || !thread.is_resumable() {
                end_turn(lua, true);
                return values;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("{code}: still loading");
    }

    /// What the mod shows.
    fn shown(lua: &Lua) -> Vec<(i32, Sprite)> {
        canvas(lua).unwrap().ready.borrow().items.clone()
    }

    /// The published list is the process's: one test at a time publishes.
    static PUBLISHING: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn budget(lua: &Lua) -> (usize, usize) {
        let canvas = lua.app_data_ref::<Rc<Canvas>>().unwrap();
        (canvas.budget.images.get(), canvas.budget.bytes.get())
    }

    #[test]
    fn png_premultiplied() {
        let rgba = png(2, 1, png::ColorType::Rgba, &[255, 0, 0, 128, 0, 255, 0, 255]);
        assert_eq!(decode_png(&rgba, IMAGE_BYTES).unwrap(), (2, 1, vec![128, 0, 0, 128, 0, 255, 0, 255]));
        let grey = png(1, 1, png::ColorType::Grayscale, &[200]);
        assert_eq!(decode_png(&grey, IMAGE_BYTES).unwrap().2, vec![200, 200, 200, 255]);
        let rgb = png(1, 2, png::ColorType::Rgb, &[1, 2, 3, 4, 5, 6]);
        assert_eq!(decode_png(&rgb, IMAGE_BYTES).unwrap().2, vec![1, 2, 3, 255, 4, 5, 6, 255]);
    }

    /// Too large, or larger than what is left: refused from the header.
    #[test]
    fn png_limits() {
        let wide = png(SIDE + 1, 1, png::ColorType::Grayscale, &vec![0; SIDE as usize + 1]);
        assert!(decode_png(&wide, IMAGE_BYTES).unwrap_err().contains("2048×2048 at most"));
        let small = png(64, 64, png::ColorType::Grayscale, &[0; 64 * 64]);
        assert!(decode_png(&small, Image::bytes_for(64, 64) - 1).unwrap_err().contains("more than is left"));
        assert!(decode_png(&small, Image::bytes_for(64, 64)).is_ok());
        assert!(decode_png(b"not a png", IMAGE_BYTES).unwrap_err().starts_with("not a PNG"));
    }

    /// An image counts until released or the mod's VM goes, which frees it
    /// once the renderer's list lets it go too.
    #[test]
    fn images_lifetime() {
        let lua = vm();
        let size = run(&lua, "img = draw.loadImage('images/a.png') draw.image(img, 0.5, 0.5, 0.1) return img.width, img.height").unwrap();
        assert_eq!(size.into_iter().map(|v| v.as_i64().unwrap()).collect::<Vec<_>>(), [16, 8]);
        assert_eq!(budget(&lua), (1, Image::bytes_for(16, 8)));
        let drawn = shown(&lua);
        let Shape::Image { image, size, tint, .. } = &drawn[0].1.shape else { panic!("not an image") };
        assert_eq!((*size, *tint), ([0.1, 0.0], [255; 4]));
        let image = Arc::downgrade(image);
        drop(drawn);
        run(&lua, "img:release()").unwrap();
        assert_eq!(budget(&lua), (0, 0));
        assert!(image.upgrade().is_none(), "the mod no longer holds it, nor shows it");
        let e = run(&lua, "draw.image(img, 0.5, 0.5, 0.1)").unwrap_err().to_string();
        assert!(e.contains("released"), "{e}");

        run(&lua, "img = draw.loadImage('images/a.png') draw.image(img, 0.5, 0.5, 0.1)").unwrap();
        let drawn = shown(&lua);
        let Shape::Image { image, .. } = &drawn[0].1.shape else { panic!("not an image") };
        let image = Arc::downgrade(image);
        drop(lua);
        assert!(image.upgrade().is_some(), "the list drawn holds it");
        drop(drawn);
        assert!(image.upgrade().is_none(), "the mod stopped");
    }

    #[test]
    fn load_errors() {
        let lua = vm();
        for (path, says) in [
            ("images/big.png", "2048×2048 at most"),
            ("images/bad.png", "not a PNG"),
            ("images/none.png", "images/none.png"),
            ("../other/a.png", "a file of the mod"),
            ("main.luau", "PNG images only"),
        ] {
            let e = run(&lua, &format!("draw.loadImage('{path}')")).unwrap_err().to_string();
            assert!(e.contains(says), "{path}: {e}");
        }
        assert_eq!(budget(&lua), (0, 0), "failed loads count no more");
        let console = Lua::new();
        install(&console).unwrap();
        let e = run(&console, "draw.loadImage('images/a.png')").unwrap_err().to_string();
        assert!(e.contains("the console has none"), "{e}");
    }

    #[test]
    fn limits() {
        let lua = vm();
        lua.app_data_ref::<Rc<Canvas>>().unwrap().budget.images.set(IMAGES);
        let e = run(&lua, "draw.loadImage('images/a.png')").unwrap_err().to_string();
        assert!(e.contains("may hold 64 images"), "{e}");
        lua.app_data_ref::<Rc<Canvas>>().unwrap().budget.images.set(0);
        lua.app_data_ref::<Rc<Canvas>>().unwrap().budget.bytes.set(IMAGE_BYTES - 10);
        let e = run(&lua, "draw.loadImage('images/a.png')").unwrap_err().to_string();
        assert!(e.contains("more than is left"), "{e}");
        assert_eq!(budget(&lua).0, 0);

        let e = run(&lua, &format!("for i = 1, {} do draw.rect(0.5, 0.5, 0.1, 0.1) end", DRAWINGS + 1)).unwrap_err().to_string();
        assert!(e.contains("drawings a turn at most"), "{e}");
        let e = run(&lua, &format!("local p = table.create({}, 0.5) for i = 1, 5 do draw.path(p) end", POINTS / 2)).unwrap_err().to_string();
        assert!(e.contains("points a turn at most"), "{e}");
        for bad in [
            "draw.rect(0 / 0, 0, 1, 1)",
            "draw.rect(0, 0, 1, 1, -1)",
            "draw.rect(0, 0, 1, 1, 0.5)",
            "draw.rect(0, 0, 1, 1, nil, {})",
            "draw.path({ 0, 0, 1 })",
            "draw.style{ align = 'middle' }",
            "draw.style{ colour = 1 }",
            "draw.style{ blend = 'multiply' }",
            "local s = draw.style() s.rotation = 'a'",
        ] {
            assert!(run(&lua, bad).is_err(), "{bad}");
        }
    }

    /// Positional colours and the style's fields reach the shapes.
    #[test]
    fn shapes_and_styles() {
        let lua = vm();
        run(&lua, "
            local s = draw.style{ z = 2, rounding = 0.01, stroke = 0x000000ff, width = 0.004, gradient = draw.rgba(0, 0, 255) }
            s.rotation = 450
            draw.rect(0.1, 0.2, 0.3, 0.4, 0xff0000ff, s)
            draw.circle(0.5, 0.5, 0.1, nil, draw.style{ stroke = 0xffffff80, blend = 'add' })
            draw.arc(0.5, 0.5, 0.2, -120, 120, 0.01, 0x00ff00ff)
            draw.path({ 0, 0, 1, 0, 1, 1 }, nil, nil, draw.style{ fill = 0x11223344 })
            draw.line(0, 0, 1, 1)
            draw.text('hi', 0.5, 0.5, 0.05, nil, draw.style{ align = 'right' })
        ").unwrap();
        let drawn = shown(&lua);
        assert!(matches!(drawn[0], (2, Sprite { shape: Shape::Rect { rotation: 90.0, fill: [255, 0, 0, 255], gradient: Some(([0, 0, 255, 255], false)), rounding: 0.01, stroke: Some(Stroke { color: [0, 0, 0, 255], width: 0.004 }), .. }, additive: false, .. })));
        assert!(matches!(drawn[1].1.shape, Shape::Circle { fill: None, stroke: Some(Stroke { color: [255, 255, 255, 128], .. }), .. }));
        assert!(drawn[1].1.additive);
        assert!(matches!(drawn[2].1.shape, Shape::Arc { from: -120.0, to: 120.0, stroke: Stroke { color: [0, 255, 0, 255], width: 0.01 }, .. }));
        let c = canvas(&lua).unwrap();
        assert!(matches!(&drawn[3].1.shape, Shape::Path { points, closed: true, fill: Some([0x11, 0x22, 0x33, 0x44]), stroke: None } if c.ready.borrow().points[points.start as usize..points.end as usize] == [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]));
        assert!(matches!(drawn[4].1.shape, Shape::Line { stroke: Stroke { color: [255, 255, 255, 255], .. }, .. }));
        assert!(matches!(&drawn[5].1.shape, Shape::Text { text, align: Align::Right, size: 0.05, .. } if &**text == "hi"));
    }

    /// A sprite sheet's frame and nine-slicing are the image's coordinates.
    #[test]
    fn image_parts() {
        let lua = vm();
        run(&lua, "
            local img = draw.loadImage('images/a.png')
            local frame = draw.style{ uv = { 0, 0, 0.25, 1 }, nineSlice = { 2, 2, 2, 2 } }
            draw.image(img, 0.5, 0.5, 0.1, nil, nil, frame)
            frame:setUv(0.25, 0, 0.5, 1)
            draw.image(img, 0.5, 0.5, 0.1, nil, nil, frame)
        ").unwrap();
        let drawn = shown(&lua);
        assert!(matches!(drawn[0].1.shape, Shape::Image { uv: [0.0, 0.0, 0.25, 1.0], nine: Some([2.0, 2.0, 2.0, 2.0]), .. }));
        assert!(matches!(drawn[1].1.shape, Shape::Image { uv: [0.25, 0.0, 0.5, 1.0], .. }));
        for bad in ["draw.style{ uv = { 0, 0, 1 } }", "draw.style{ nineSlice = 4 }"] {
            assert!(run(&lua, bad).is_err(), "{bad}");
        }
    }

    /// Groups nest; what is drawn in one is in it; those left open close
    /// with the turn; published, their indices follow their mod's.
    #[test]
    fn groups() {
        let (a, b) = (vm(), vm());
        run(&a, "draw.rect(0, 0, 1, 1)").unwrap();
        run(&b, "
            draw.push(0.5, 0.5, draw.style{ scale = 2, rotation = 90 })
            draw.clip(0, 0, 0.2, 0.1)
            draw.circle(0, 0, 0.1)
            draw.pop()
            draw.line(0, 0, 1, 1)
            draw.pop()
            draw.rect(0, 0, 1, 1)
            draw.push(0.1, 0.1)
        ").unwrap();
        let drawn = shown(&b);
        assert_eq!(drawn.iter().map(|(_, s)| s.group).collect::<Vec<_>>(), [1, 0, NONE]);
        let groups = canvas(&b).unwrap().ready.borrow().groups.clone();
        assert_eq!(groups[0], Group { parent: NONE, origin: [0.5, 0.5], scale: 2.0, rotation: 90.0, clip: None });
        assert_eq!(groups[1], Group { parent: 0, origin: [0.0, 0.0], scale: 1.0, rotation: 0.0, clip: Some([0.0, 0.0, 0.2, 0.1]) });
        assert!(canvas(&b).unwrap().open.borrow().is_empty(), "closed with the turn");
        assert!(run(&a, "draw.pop()").is_err());
        run(&a, "draw.push(0, 0) draw.rect(0, 0, 1, 1) draw.pop()").unwrap();
        let _one = PUBLISHING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        publish([canvas(&a).unwrap(), canvas(&b).unwrap()]);
        let list = sprites::latest().unwrap();
        // a's group first, then b's (the second inside the first; the
        // third left open, empty).
        assert_eq!(list.groups.iter().map(|g| g.parent).collect::<Vec<_>>(), [NONE, NONE, 1, NONE]);
        assert_eq!(list.sprites.iter().map(|s| s.group).collect::<Vec<_>>(), [0, 2, 1, NONE]);
    }

    /// A turn cut short leaves the last whole one on screen; the next whole
    /// one shows what both drew. The same string is the same text.
    #[test]
    fn whole_turns() {
        let lua = vm();
        run(&lua, "draw.rect(0, 0, 1, 1)").unwrap();
        lua.load("draw.text('a', 0, 0) draw.text('a', 0, 0)").exec().unwrap();
        end_turn(&lua, false);
        assert_eq!(shown(&lua).len(), 1);
        lua.load("draw.circle(0, 0, 1)").exec().unwrap();
        end_turn(&lua, true);
        let drawn = shown(&lua);
        assert_eq!(drawn.len(), 3);
        let (Shape::Text { text: a, .. }, Shape::Text { text: b, .. }) = (&drawn[0].1.shape, &drawn[1].1.shape) else { panic!() };
        assert!(Arc::ptr_eq(a, b));
        run(&lua, "").unwrap();
        assert!(shown(&lua).is_empty(), "a whole turn without drawing clears");
    }

    /// The mods' drawings in one list, by z, then as drawn, published only
    /// when something changed; paths' points follow their mod's.
    #[test]
    fn published_by_z() {
        let (a, b) = (vm(), vm());
        run(&a, "draw.rect(0.1, 0.2, 0.3, 0.4, nil, draw.style{ z = 2 }) draw.path({ 0, 0, 1, 1 })").unwrap();
        run(&b, "draw.path({ 0.5, 0.5, 0.6, 0.6 }, nil, nil, draw.style{ z = -1 }) draw.rect(0, 0, 1, 1, nil, draw.style{ z = 2 })").unwrap();
        let both = || [canvas(&a).unwrap(), canvas(&b).unwrap()];
        let _one = PUBLISHING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        publish(both());
        let list = sprites::latest().unwrap();
        let path = |s: &Sprite| match &s.shape {
            Shape::Path { points, .. } => Some(list.points[points.start as usize]),
            _ => None,
        };
        assert_eq!(list.sprites.iter().map(path).collect::<Vec<_>>(), [Some([0.5, 0.5]), Some([0.0, 0.0]), None, None]);
        assert!(matches!(list.sprites[2].shape, Shape::Rect { center: [0.1, 0.2], .. }));
        drop(list);
        let generation = sprites::generation();
        publish(both());
        assert_eq!(sprites::generation(), generation, "nothing new");
        publish([canvas(&a).unwrap()]);
        assert_eq!(sprites::generation(), generation + 1, "a mod went");
        run(&a, "").unwrap();
        publish([canvas(&a).unwrap()]);
        assert!(sprites::latest().is_none());
    }
}
