//! `draw` in Luau: what ScriptHookV's createTexture/drawTexture gave, and
//! a little more. A mod loads images (PNG) from its folder, read and
//! decoded on a thread of their own, and draws them, rectangles and text
//! over the game a frame at a time, in fractions of the screen.
//!
//! The natives draw rectangles, the game's sprites and text in the game's
//! own frame (DRAW_RECT, DRAW_SPRITE, the text commands); these are drawn
//! by the renderer over the frame once it is scaled to the window (glue5-gpu's
//! sprites.rs), so at the window's resolution, above whatever the game
//! drew, below the app's own UI.
//!
//! A frame's drawings are handed to the renderer at the start of the next
//! (the engine's frame count changing, engine.rs): what a mod draws shows
//! for that frame. A mod's images are its run's: they go with its VM.

use std::cell::{Cell, RefCell};
use std::io::Cursor;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

use glue5_runtime::sprites::{self, Align, Image, Shape, Sprite};
use mlua::{AnyUserData, Function, Lua, Table, UserData, UserDataFields, UserDataMethods, Value};

/// An image's side at most, in pixels: what every GPU takes.
pub const SIDE: u32 = 2048;
/// An image file's size at most.
const FILE: u64 = 16 << 20;
/// Images a run of a mod may hold, those loading included.
pub const IMAGES: usize = 64;
/// What they may take, decoded (4 bytes a pixel, as on the GPU).
pub const IMAGE_BYTES: usize = 32 << 20;
/// Drawings a mod may make in a frame.
pub const DRAWINGS: usize = 1024;
/// A text's length at most, in bytes (past it, it is cut).
const TEXT: usize = 1024;

const MB: usize = 1 << 20;

/// What a mod's VM keeps for `draw` (in its app data).
#[derive(Default)]
struct Canvas {
    /// The mod's folder (none: the console, which has none).
    module: RefCell<Option<String>>,
    budget: Rc<Budget>,
    /// This frame's drawings, with their z.
    drawings: RefCell<Vec<(i32, Sprite)>>,
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
    let need = width as usize * height as usize * 4;
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
            move |_, (image, x, y, w, h, options): (AnyUserData, f64, f64, f64, Option<f64>, Option<Table>)| {
                let image = image
                    .borrow::<LuaImage>()
                    .map_err(|_| mlua::Error::runtime("draw.image: an image from draw.loadImage"))?
                    .image
                    .borrow()
                    .clone()
                    .ok_or_else(|| mlua::Error::runtime("draw.image: the image was released"))?;
                let style = Style::read("draw.image", options.as_ref())?;
                let size = [fraction(w, "width")?.max(0.0), fraction(h.unwrap_or(0.0), "height")?.max(0.0)];
                c.add(style.z, Sprite {
                    shape: Shape::Image(image),
                    center: [fraction(x, "x")?, fraction(y, "y")?],
                    size,
                    rotation: style.rotation,
                    color: style.color,
                })
            },
        )?,
    )?;
    let c = canvas.clone();
    draw.set(
        "rect",
        lua.create_function(move |_, (x, y, w, h, options): (f64, f64, f64, f64, Option<Table>)| {
            let style = Style::read("draw.rect", options.as_ref())?;
            c.add(style.z, Sprite {
                shape: Shape::Rect,
                center: [fraction(x, "x")?, fraction(y, "y")?],
                size: [fraction(w, "width")?.max(0.0), fraction(h, "height")?.max(0.0)],
                rotation: style.rotation,
                color: style.color,
            })
        })?,
    )?;
    let c = canvas.clone();
    draw.set(
        "text",
        lua.create_function(move |_, (text, x, y, options): (mlua::LuaString, f64, f64, Option<Table>)| {
            let style = Style::read("draw.text", options.as_ref())?;
            let (mut size, mut align) = (0.03, Align::Left);
            if let Some(o) = &options {
                if let Some(s) = number(o.get("size")?, "draw.text: size")? {
                    size = s.clamp(0.001, 1.0);
                }
                align = match o.get::<Value>("align")? {
                    Value::Nil => Align::Left,
                    Value::String(s) if s == "left" => Align::Left,
                    Value::String(s) if s == "center" => Align::Center,
                    Value::String(s) if s == "right" => Align::Right,
                    _ => return Err(mlua::Error::runtime("draw.text: align is \"left\", \"center\" or \"right\"")),
                };
            }
            let bytes = text.as_bytes();
            let text = String::from_utf8_lossy(&bytes[..bytes.len().min(TEXT)]).into_owned();
            c.add(style.z, Sprite {
                shape: Shape::Text(text, align),
                center: [fraction(x, "x")?, fraction(y, "y")?],
                size: [0.0, size as f32],
                rotation: style.rotation,
                color: style.color,
            })
        })?,
    )?;
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

/// What a mod drew since the last call, with the z of each.
pub fn take(lua: &Lua) -> Vec<(i32, Sprite)> {
    lua.app_data_ref::<Rc<Canvas>>().map(|c| std::mem::take(&mut *c.drawings.borrow_mut())).unwrap_or_default()
}

/// The mods' drawings of a frame, for the renderer: by z, then in the
/// order drawn.
pub fn publish(drawings: impl IntoIterator<Item = Vec<(i32, Sprite)>>) {
    let mut all: Vec<(i32, Sprite)> = drawings.into_iter().flatten().collect();
    all.sort_by_key(|(z, _)| *z);
    sprites::publish(all.into_iter().map(|(_, s)| s).collect());
}

impl Canvas {
    fn add(&self, z: i32, sprite: Sprite) -> mlua::Result<()> {
        let mut drawings = self.drawings.borrow_mut();
        if drawings.len() >= DRAWINGS {
            return Err(mlua::Error::runtime(format!("draw: {DRAWINGS} drawings a frame at most")));
        }
        drawings.push((z, sprite));
        Ok(())
    }
}

/// A drawing's rotation, colour and z.
struct Style {
    rotation: f32,
    color: [u8; 4],
    z: i32,
}

impl Style {
    fn read(what: &str, options: Option<&Table>) -> mlua::Result<Style> {
        let mut style = Style { rotation: 0.0, color: [255; 4], z: 0 };
        let Some(o) = options else { return Ok(style) };
        if let Some(r) = number(o.get("rotation")?, &format!("{what}: rotation"))? {
            style.rotation = (r % 360.0) as f32;
        }
        if let Some(z) = number(o.get("z")?, &format!("{what}: z"))? {
            style.z = z.clamp(-1e6, 1e6).round() as i32;
        }
        match o.get::<Value>("color")? {
            Value::Nil => {}
            Value::Table(t) => {
                for (i, c) in style.color.iter_mut().enumerate() {
                    if let Some(v) = number(t.raw_get(i + 1)?, &format!("{what}: color[{}]", i + 1))? {
                        *c = v.clamp(0.0, 255.0).round() as u8;
                    } else if i < 3 {
                        return Err(mlua::Error::runtime(format!("{what}: color is {{r, g, b, a?}}, 0 to 255")));
                    }
                }
            }
            _ => return Err(mlua::Error::runtime(format!("{what}: color is {{r, g, b, a?}}, 0 to 255"))),
        }
        Ok(style)
    }
}

/// An optional number option, finite.
fn number(v: Value, what: &str) -> mlua::Result<Option<f64>> {
    match v {
        Value::Nil => Ok(None),
        Value::Integer(i) => Ok(Some(i as f64)),
        Value::Number(n) if n.is_finite() => Ok(Some(n)),
        _ => Err(mlua::Error::runtime(format!("{what}: a number"))),
    }
}

/// A position or size in fractions of the screen: finite, and within a
/// few screens.
fn fraction(v: f64, what: &str) -> mlua::Result<f32> {
    if !v.is_finite() {
        return Err(mlua::Error::runtime(format!("draw: {what} is not a number")));
    }
    Ok(v.clamp(-16.0, 16.0) as f32)
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
export type DrawOptions = {
\t-- Degrees, clockwise, around its centre (a text's: where it is drawn from).
\trotation: number?,
\t-- {r, g, b, a?}, 0 to 255: an image's colours are multiplied by it (white: as they are).
\tcolor: { number }?,
\t-- Higher in front (0 by default); equal ones in the order drawn.
\tz: number?,
}
export type TextOptions = {
\trotation: number?,
\tcolor: { number }?,
\tz: number?,
\t-- Its height, a fraction of the screen's (0.03 by default).
\tsize: number?,
\t-- Which of its ends is at x (\"left\" by default).
\talign: (\"left\" | \"center\" | \"right\")?,
}

-- Images, rectangles and text over the game, for one frame: draw them every frame (a loop with wait())
-- to keep them on screen. Positions and sizes are fractions of the screen (0, 0 its top left, 1, 1 its
-- bottom right), centred as DRAW_RECT's and DRAW_SPRITE's; they are drawn at the window's resolution,
-- over everything the game draws, under the app's own controls.
declare draw: {
\t-- An image from the mod's folder (\"images/logo.png\"): PNG, 2048×2048 at most. Waits, as wait() does,
\t-- while it is read and decoded away from the game. A mod holds 64 images, 32 MB of them
\t-- (4 bytes a pixel), at most; they go when it stops.
\tloadImage: (path: string) -> Image,
\t-- `image` centred at x, y, width by height (none: the image's proportions).
\timage: (image: Image, x: number, y: number, width: number, height: number?, options: DrawOptions?) -> (),
\t-- A rectangle centred at x, y (white unless options.color says).
\trect: (x: number, y: number, width: number, height: number, options: DrawOptions?) -> (),
\t-- `text` from x, y (its top).
\ttext: (text: string, x: number, y: number, options: TextOptions?) -> (),
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
    /// it ends.
    fn run(lua: &Lua, code: &str) -> mlua::Result<MultiValue> {
        let f: Function = lua.load(code).into_function()?;
        let thread = lua.create_thread(f)?;
        for _ in 0..5000 {
            let values = thread.resume::<MultiValue>(())?;
            if !thread.is_resumable() {
                return Ok(values);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("{code}: still loading");
    }

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
        assert!(decode_png(&small, 64 * 64 * 4 - 1).unwrap_err().contains("more than is left"));
        assert!(decode_png(&small, 64 * 64 * 4).is_ok());
        assert!(decode_png(b"not a png", IMAGE_BYTES).unwrap_err().starts_with("not a PNG"));
    }

    /// An image counts until released or the mod's VM goes, which frees it
    /// once the renderer's list lets it go too.
    #[test]
    fn images_lifetime() {
        let lua = vm();
        let size = run(&lua, "img = draw.loadImage('images/a.png') draw.image(img, 0.5, 0.5, 0.1) return img.width, img.height").unwrap();
        assert_eq!(size.into_iter().map(|v| v.as_i64().unwrap()).collect::<Vec<_>>(), [16, 8]);
        assert_eq!(budget(&lua), (1, 16 * 8 * 4));
        let drawn = take(&lua);
        let Shape::Image(image) = &drawn[0].1.shape else { panic!("not an image") };
        let image = Arc::downgrade(image);
        assert_eq!(drawn[0].1.size, [0.1, 0.0]);
        drop(drawn);
        assert!(image.upgrade().is_some(), "the mod holds it");
        run(&lua, "img:release()").unwrap();
        assert_eq!(budget(&lua), (0, 0));
        assert!(image.upgrade().is_none());
        let e = run(&lua, "draw.image(img, 0.5, 0.5, 0.1)").unwrap_err().to_string();
        assert!(e.contains("released"), "{e}");

        run(&lua, "img = draw.loadImage('images/a.png') draw.image(img, 0.5, 0.5, 0.1)").unwrap();
        let drawn = take(&lua);
        let Shape::Image(image) = &drawn[0].1.shape else { panic!("not an image") };
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

        run(&lua, &format!("for i = 1, {DRAWINGS} do draw.rect(0.5, 0.5, 0.1, 0.1) end")).unwrap();
        let e = run(&lua, "draw.text('one more', 0, 0)").unwrap_err().to_string();
        assert!(e.contains("drawings a frame at most"), "{e}");
        assert_eq!(take(&lua).len(), DRAWINGS);
        run(&lua, "draw.text('next frame', 0, 0)").unwrap();
        for bad in [
            "draw.rect(0 / 0, 0, 1, 1)",
            "draw.rect(0, 0, 1, 1, { color = { 1, 2 } })",
            "draw.rect(0, 0, 1, 1, { rotation = 'a' })",
            "draw.text('x', 0, 0, { align = 'middle' })",
        ] {
            assert!(run(&lua, bad).is_err(), "{bad}");
        }
    }

    /// The mods' drawings in one list, by z, then as drawn; rotation,
    /// colour and alignment as given.
    #[test]
    fn published_by_z() {
        let a = vm();
        let b = vm();
        run(&a, "draw.rect(0.1, 0.2, 0.3, 0.4, { z = 2, rotation = 450, color = { 1, 2, 3 } }) draw.rect(0, 0, 1, 1)").unwrap();
        run(&b, "draw.text('hi', 0.5, 0.5, { z = -1, size = 0.05, align = 'right' }) draw.rect(0, 0, 1, 1, { z = 2 })").unwrap();
        publish([take(&a), take(&b)]);
        let list = sprites::latest().unwrap();
        let kinds: Vec<_> = list.iter().map(|s| (matches!(s.shape, Shape::Rect), s.center)).collect();
        assert_eq!(kinds, [(false, [0.5, 0.5]), (true, [0.0, 0.0]), (true, [0.1, 0.2]), (true, [0.0, 0.0])]);
        assert!(matches!(&list[0].shape, Shape::Text(t, Align::Right) if t == "hi"));
        assert_eq!(list[0].size, [0.0, 0.05]);
        assert_eq!((list[2].rotation, list[2].color), (90.0, [1, 2, 3, 255]));
        let generation = sprites::generation();
        publish([take(&a), take(&b)]);
        assert!(sprites::latest().is_none());
        publish([take(&a), take(&b)]);
        assert_eq!(sprites::generation(), generation + 1, "nothing drawn again: nothing new");
    }
}
