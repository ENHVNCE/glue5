//! The sprites host code draws over the game (glue5-runtime's `sprites`,
//! the mods' images, shapes and text): drawn into the window's image after
//! the game's frame is scaled to it, at its resolution, before the
//! application's UI.
//!
//! epaint (egui's painting crate, without an egui frame) turns them into
//! triangles, anti-aliased by a pixel of feathering: shapes and text go into
//! one mesh with the font's texture (its white pixel for the shapes), an
//! image into the same mesh as the images around it with its texture, so
//! draw calls are cut only where the texture changes in the drawing order.
//! The meshes, the scratch paths and the GPU buffers are kept from frame to
//! frame and grow only; a list is laid out again only when a new one comes
//! (or the window's size changes). Images are uploaded the first time they
//! are drawn and freed when nothing holds them.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use egui::epaint::text::{FontDefinitions, LayoutJob};
use egui::epaint::tessellator::{Path, path};
use egui::epaint::{
    ClippedPrimitive, Color32, CornerRadiusF32, FontId, Fonts, Galley, Mesh, PathStroke, Primitive,
    TessellationOptions, Tessellator, TextOptions, TextShape, TextureId, Vertex,
};
use egui::emath::{Align2, Pos2, Rect, Rot2, Vec2, pos2, vec2};
use glue5_runtime::sprites::{self, Align, Image, List, Rgba, Shape, Stroke};

/// The font atlas's side at most (and its texture's).
const ATLAS: usize = 2048;
/// The font atlas's texture (as egui's).
const FONT_TEXTURE: TextureId = TextureId::Managed(0);

#[derive(Default)]
pub(crate) struct Sprites {
    /// The list laid out (its generation), the images dropped then, the size.
    seen: Option<(u64, u64, [u32; 2])>,
    renderer: Option<(wgpu::TextureFormat, egui_wgpu::Renderer)>,
    /// The images' textures, by image id.
    textures: HashMap<u64, Texture>,
    /// The list laid out (its images stay while it is drawn).
    list: Option<Arc<List>>,
    layout: Layout,
}

struct Texture {
    image: Weak<Image>,
    view: wgpu::TextureView,
    id: TextureId,
}

/// What laying out keeps between frames.
#[derive(Default)]
struct Layout {
    fonts: Option<Fonts>,
    /// The batches drawn, and meshes kept for the next lists.
    primitives: Vec<ClippedPrimitive>,
    meshes: Vec<Mesh>,
    path: Path,
    points: Vec<Pos2>,
    /// Laid-out texts, by their text (held, so its address is not reused),
    /// size, alignment; whether the last list used them.
    galleys: HashMap<(usize, u32, u8), (Arc<str>, Arc<Galley>, bool)>,
}

impl Sprites {
    /// Lays out the latest list when it changed, and records the uploads
    /// into `encoder`: whether there is something to [`Sprites::render`].
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        format: wgpu::TextureFormat,
        size: [u32; 2],
    ) -> bool {
        sprites::set_screen(size[0], size[1]);
        let generation = sprites::generation();
        if self.seen.is_none() && generation == 0 {
            // Nothing was ever drawn.
            return false;
        }
        if self.renderer.as_ref().is_none_or(|(f, _)| *f != format) {
            let mut renderer = egui_wgpu::Renderer::new(device, format, Default::default());
            // The images again, in this renderer; the font's atlas whole.
            for t in self.textures.values_mut() {
                t.id = renderer.register_native_texture(device, &t.view, wgpu::FilterMode::Linear);
            }
            self.renderer = Some((format, renderer));
            self.layout.fonts = None;
            self.layout.galleys.clear();
            self.seen = None;
        }
        let relayout = self.seen.is_none_or(|(g, _, s)| g != generation || s != size);
        if relayout {
            self.list = sprites::latest();
            let list = self.list.clone();
            let mut layout = std::mem::take(&mut self.layout);
            layout.begin();
            if let Some(list) = &list {
                layout.list(list, size, |image| self.texture(device, queue, image));
            }
            let renderer = &mut self.renderer.as_mut().unwrap().1;
            if let Some(delta) = layout.fonts.as_mut().and_then(Fonts::font_image_delta) {
                renderer.update_texture(device, queue, FONT_TEXTURE, &delta);
            }
            if !layout.primitives.is_empty() {
                let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: size, pixels_per_point: 1.0 };
                renderer.update_buffers(device, queue, encoder, &layout.primitives, &screen);
            }
            self.layout = layout;
        }
        // Images nobody holds (the list laid out holds its own): their
        // textures go.
        let dropped = sprites::dropped();
        if self.seen.is_none_or(|(_, d, _)| d != dropped) {
            let renderer = &mut self.renderer.as_mut().unwrap().1;
            self.textures.retain(|_, t| {
                let alive = t.image.strong_count() > 0;
                if !alive {
                    renderer.free_texture(&t.id);
                }
                alive
            });
        }
        self.seen = Some((generation, dropped, size));
        !self.layout.primitives.is_empty()
    }

    /// Draws what [`Sprites::prepare`] laid out into `pass`.
    pub fn render(&mut self, pass: &mut wgpu::RenderPass<'static>, size: [u32; 2]) {
        let renderer = &mut self.renderer.as_mut().unwrap().1;
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: size, pixels_per_point: 1.0 };
        renderer.render(pass, &self.layout.primitives, &screen);
    }

    /// `image`'s texture, uploaded now if it is not yet.
    fn texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, image: &Arc<Image>) -> Option<TextureId> {
        if let Some(t) = self.textures.get(&image.id()) {
            return Some(t.id);
        }
        let pixels = image.take_pixels()?;
        let (width, height) = image.size();
        let extent = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        // What egui's renderer takes: sRGB values read as they are.
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprite"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &pixels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * width), rows_per_image: Some(height) },
            extent,
        );
        let view = texture.create_view(&Default::default());
        let id = self.renderer.as_mut()?.1.register_native_texture(device, &view, wgpu::FilterMode::Linear);
        self.textures.insert(image.id(), Texture { image: Arc::downgrade(image), view, id });
        Some(id)
    }
}

/// A colour as the renderer blends it: premultiplied, and with no alpha
/// when `add`ed. The blend is "one, one minus the source's alpha", so a
/// colour without alpha adds to what is under it: additive drawings go in
/// the same batches as the others, no pipeline of their own to switch to.
fn color(c: Rgba, add: bool) -> Color32 {
    let c = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
    if add { additive(c) } else { c }
}

fn additive(c: Color32) -> Color32 {
    Color32::from_rgba_premultiplied(c.r(), c.g(), c.b(), 0)
}

fn stroke(s: &Stroke, h: f32, add: bool) -> PathStroke {
    PathStroke::new((s.width * h).max(0.0), color(s.color, add))
}

impl Layout {
    /// A new list: the batches emptied (their memory kept), the font's
    /// texts not used since the last list dropped.
    fn begin(&mut self) {
        for p in self.primitives.drain(..) {
            if let Primitive::Mesh(mut mesh) = p.primitive {
                mesh.clear();
                self.meshes.push(mesh);
            }
        }
        self.galleys.retain(|_, (_, _, used)| std::mem::take(used));
        let fonts = self.fonts.get_or_insert_with(|| {
            Fonts::new(TextOptions { max_texture_side: ATLAS, ..Default::default() }, FontDefinitions::default())
        });
        let atlas = fonts.texture_atlas().fill_ratio();
        fonts.begin_pass(TextOptions { max_texture_side: ATLAS, ..Default::default() });
        // A full atlas was made again: the texts with it.
        if fonts.texture_atlas().fill_ratio() < atlas {
            self.galleys.clear();
        }
    }

    /// The mesh to add to for `texture` (the last batch, or a new one).
    fn batch(&mut self, texture: TextureId, clip: Rect) -> &mut Mesh {
        let same = self.primitives.last().is_some_and(|p| {
            p.clip_rect == clip && matches!(&p.primitive, Primitive::Mesh(m) if m.texture_id == texture)
        });
        if !same {
            let mut mesh = self.meshes.pop().unwrap_or_default();
            mesh.texture_id = texture;
            self.primitives.push(ClippedPrimitive { clip_rect: clip, primitive: Primitive::Mesh(mesh) });
        }
        match &mut self.primitives.last_mut().unwrap().primitive {
            Primitive::Mesh(mesh) => mesh,
            Primitive::Callback(_) => unreachable!("only meshes"),
        }
    }

    /// The list as batches, in pixels; `texture` gives an image's.
    fn list(&mut self, list: &List, size: [u32; 2], mut texture: impl FnMut(&Arc<Image>) -> Option<TextureId>) {
        let (w, h) = (size[0] as f32, size[1] as f32);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(w, h));
        let at = |p: [f32; 2]| pos2(p[0] * w, p[1] * h);
        let mut fonts = self.fonts.take().expect("fonts made by begin");
        // The texts first: their glyphs may grow the atlas, whose size the
        // tessellator needs.
        for sprite in &list.sprites {
            if let Shape::Text { text, size, align, .. } = &sprite.shape {
                let px = (size * h).clamp(1.0, h.max(1.0));
                let key = (text.as_ptr() as usize, px.to_bits(), *align as u8);
                if let Some((_, _, used)) = self.galleys.get_mut(&key) {
                    *used = true;
                    continue;
                }
                let mut job = LayoutJob::simple(text.to_string(), FontId::proportional(px), Color32::WHITE, f32::INFINITY);
                job.halign = match align {
                    Align::Left => egui::Align::LEFT,
                    Align::Center => egui::Align::Center,
                    Align::Right => egui::Align::RIGHT,
                };
                let galley = fonts.with_pixels_per_point(1.0).layout_job(job);
                self.galleys.insert(key, (text.clone(), galley, true));
            }
        }
        let mut tess = Tessellator::new(
            1.0,
            TessellationOptions::default(),
            fonts.font_image_size(),
            fonts.texture_atlas().prepared_discs(),
        );
        let feathering = 1.0;
        for sprite in &list.sprites {
            let add = sprite.additive;
            match &sprite.shape {
                Shape::Image { image, center, size, rotation, tint } => {
                    let (iw, ih) = image.size();
                    let width = size[0] * w;
                    let height = if size[1] > 0.0 { size[1] * h } else { width * ih as f32 / iw as f32 };
                    let c = at(*center);
                    let half = vec2(width, height) / 2.0;
                    if !visible(screen, c, half.length()) {
                        continue;
                    }
                    let Some(id) = texture(image) else { continue };
                    let rot = Rot2::from_angle(rotation.to_radians());
                    quad(self.batch(id, screen), c, half, rot, [0.0, 0.0, 1.0, 1.0], color(*tint, add));
                }
                Shape::Rect { center, size, rotation, fill, gradient, rounding, stroke: line } => {
                    let c = at(*center);
                    let half = vec2(size[0] * w, size[1] * h) / 2.0;
                    if !visible(screen, c, half.length() + line.map_or(0.0, |s| s.width * h)) {
                        continue;
                    }
                    let rot = Rot2::from_angle(rotation.to_radians());
                    let r = (rounding * h).min(half.x.min(half.y)).max(0.0);
                    path::rounded_rectangle(&mut self.points, Rect::from_center_size(Pos2::ZERO, half * 2.0), CornerRadiusF32::same(r));
                    for p in &mut self.points {
                        *p = c + rot * p.to_vec2();
                    }
                    let (points, path) = (std::mem::take(&mut self.points), std::mem::take(&mut self.path));
                    let (mut points, mut path) = (points, path);
                    let mesh = self.batch(FONT_TEXTURE, screen);
                    path.clear();
                    path.add_line_loop(&points);
                    let first = mesh.vertices.len();
                    match gradient {
                        None => path.fill(feathering, color(*fill, add), mesh),
                        Some((to, horizontal)) => {
                            path.fill(feathering, Color32::WHITE, mesh);
                            // Each vertex's colour by where it is along the
                            // gradient, its coverage (the feathered edge) kept.
                            let (from, to) = (egui::Rgba::from(color(*fill, false)), egui::Rgba::from(color(*to, false)));
                            let axis = if *horizontal { rot * vec2(1.0, 0.0) } else { rot * vec2(0.0, 1.0) };
                            let extent = if *horizontal { half.x } else { half.y };
                            for v in &mut mesh.vertices[first..] {
                                let t = ((v.pos - c).dot(axis) / (2.0 * extent.max(1e-3)) + 0.5).clamp(0.0, 1.0);
                                let coverage = v.color.a() as f32 / 255.0;
                                let c = Color32::from(from * (1.0 - t) + to * t).gamma_multiply(coverage);
                                v.color = if add { additive(c) } else { c };
                            }
                        }
                    }
                    if let Some(s) = line {
                        path.clear();
                        path.add_line_loop(&points);
                        path.stroke_closed(feathering, &stroke(s, h, add), mesh);
                    }
                    points.clear();
                    (self.points, self.path) = (points, path);
                }
                Shape::Text { text, at: from, size, rotation, align, color: c } => {
                    let px = (size * h).clamp(1.0, h.max(1.0));
                    let key = (text.as_ptr() as usize, px.to_bits(), *align as u8);
                    let Some((_, galley, _)) = self.galleys.get(&key) else { continue };
                    let galley = galley.clone();
                    let anchor = match align {
                        Align::Left => Align2::LEFT_TOP,
                        Align::Center => Align2::CENTER_TOP,
                        Align::Right => Align2::RIGHT_TOP,
                    };
                    let p = at(*from);
                    if !visible(screen, p, galley.rect.size().length() + galley.rect.min.to_vec2().length()) {
                        continue;
                    }
                    let shape = TextShape::new(p - anchor.pos_in_rect(&galley.rect).to_vec2(), galley, color(*c, add))
                        .with_override_text_color(color(*c, add))
                        .with_angle_and_anchor(rotation.to_radians(), anchor);
                    let mesh = self.batch(FONT_TEXTURE, screen);
                    tess.tessellate_text(&shape, mesh);
                }
                Shape::Line { from, to, stroke: s } => {
                    let (a, b) = (at(*from), at(*to));
                    if !visible(screen, a + (b - a) / 2.0, (b - a).length() / 2.0 + s.width * h) {
                        continue;
                    }
                    let mut path = std::mem::take(&mut self.path);
                    path.clear();
                    path.add_line_segment([a, b]);
                    path.stroke_open(feathering, &stroke(s, h, add), self.batch(FONT_TEXTURE, screen));
                    self.path = path;
                }
                Shape::Path { points, closed, fill, stroke: line } => {
                    let range = points.start as usize..points.end as usize;
                    let Some(src) = list.points.get(range) else { continue };
                    if src.len() < 2 {
                        continue;
                    }
                    let (mut pts, mut path) = (std::mem::take(&mut self.points), std::mem::take(&mut self.path));
                    pts.clear();
                    pts.extend(src.iter().map(|p| at(*p)));
                    let bounds = Rect::from_points(&pts);
                    let margin = line.map_or(0.0, |s| s.width * h);
                    if bounds.expand(margin).intersects(screen) {
                        let line = line.map(|s| stroke(&s, h, add)).unwrap_or(PathStroke::NONE);
                        let mesh = self.batch(FONT_TEXTURE, screen);
                        path.clear();
                        match (closed, fill) {
                            (true, Some(f)) => {
                                path.add_line_loop(&pts);
                                path.fill_and_stroke(feathering, color(*f, add), &line, mesh);
                            }
                            (true, None) => {
                                path.add_line_loop(&pts);
                                path.stroke_closed(feathering, &line, mesh);
                            }
                            (false, _) => {
                                path.add_open_points(&pts);
                                path.stroke_open(feathering, &line, mesh);
                            }
                        }
                    }
                    (self.points, self.path) = (pts, path);
                }
                Shape::Circle { center, radius, fill, stroke: line } => {
                    let (c, r) = (at(*center), radius * h);
                    if r <= 0.0 || !visible(screen, c, r + line.map_or(0.0, |s| s.width * h)) {
                        continue;
                    }
                    let line = line.map(|s| stroke(&s, h, add)).unwrap_or(PathStroke::NONE);
                    let mut path = std::mem::take(&mut self.path);
                    path.clear();
                    path.add_circle(c, r);
                    let fill = fill.map_or(Color32::TRANSPARENT, |f| color(f, add));
                    path.fill_and_stroke(feathering, fill, &line, self.batch(FONT_TEXTURE, screen));
                    self.path = path;
                }
                Shape::Arc { center, radius, from, to, stroke: s } => {
                    let (c, r) = (at(*center), radius * h);
                    if r <= 0.0 || !visible(screen, c, r + s.width * h) {
                        continue;
                    }
                    let (mut pts, mut path) = (std::mem::take(&mut self.points), std::mem::take(&mut self.path));
                    pts.clear();
                    let sweep = (to - from).clamp(-360.0, 360.0);
                    // A segment every 3 pixels or so of the arc.
                    let n = ((sweep.abs().to_radians() * r / 3.0).ceil() as usize).clamp(2, 512);
                    pts.extend((0..=n).map(|i| {
                        let a = (from + sweep * i as f32 / n as f32).to_radians();
                        c + vec2(a.sin(), -a.cos()) * r
                    }));
                    path.clear();
                    path.add_open_points(&pts);
                    path.stroke_open(feathering, &stroke(s, h, add), self.batch(FONT_TEXTURE, screen));
                    (self.points, self.path) = (pts, path);
                }
            }
        }
        self.fonts = Some(fonts);
    }
}

/// Whether something within `radius` of `center` may show on `screen`.
fn visible(screen: Rect, center: Pos2, radius: f32) -> bool {
    screen.expand(radius + 1.0).contains(center)
}

/// A textured quad: `half` its half size, turned by `rot` around `center`;
/// `uv` its part of the texture (u0, v0, u1, v1).
fn quad(mesh: &mut Mesh, center: Pos2, half: Vec2, rot: Rot2, uv: [f32; 4], color: Color32) {
    let base = mesh.vertices.len() as u32;
    for (x, y, u, v) in [(-1.0, -1.0, uv[0], uv[1]), (1.0, -1.0, uv[2], uv[1]), (1.0, 1.0, uv[2], uv[3]), (-1.0, 1.0, uv[0], uv[3])] {
        let pos = center + rot * vec2(x * half.x, y * half.y);
        mesh.vertices.push(Vertex { pos, uv: pos2(u, v), color });
    }
    mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}
