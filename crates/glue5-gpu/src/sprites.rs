//! The sprites host code draws over the game (glue5-runtime's `sprites`,
//! the mods' images, rectangles and text): drawn into the window's image
//! after the game's frame is scaled to it, at its resolution, before the
//! application's UI. egui lays them out (meshes, text) and draws them with
//! a renderer of their own; images are textures of ours, uploaded the
//! first time they are drawn and freed when their image is dropped.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use egui::epaint::{ClippedPrimitive, Mesh, TextShape};
use egui::emath::Rot2;
use egui::{Align2, Color32, FontId, LayerId, Pos2, Rect, Shape, TexturesDelta, pos2, vec2};
use glue5_runtime::sprites::{self, Align, Image, Sprite};

#[derive(Default)]
pub(crate) struct Sprites {
    /// The list drawn (its generation), the images dropped then, the size.
    seen: Option<(u64, u64, [u32; 2])>,
    /// Lays them out, and keeps the font's texture.
    ctx: egui::Context,
    renderer: Option<(wgpu::TextureFormat, egui_wgpu::Renderer)>,
    /// The images' textures, by image id.
    textures: HashMap<u64, Texture>,
    /// The list laid out (its images stay while it is drawn), as egui's
    /// primitives.
    list: Option<Arc<Vec<Sprite>>>,
    primitives: Vec<ClippedPrimitive>,
    /// Textures egui no longer uses, freed once drawn.
    free: Vec<egui::TextureId>,
}

struct Texture {
    image: Weak<Image>,
    view: wgpu::TextureView,
    id: egui::TextureId,
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
            // The images again, in this renderer; egui's own textures (the
            // font) come whole from a context of its own.
            for t in self.textures.values_mut() {
                t.id = renderer.register_native_texture(device, &t.view, wgpu::FilterMode::Linear);
            }
            self.renderer = Some((format, renderer));
            self.ctx = egui::Context::default();
            self.seen = None;
        }
        if self.seen.is_none_or(|(g, _, s)| g != generation || s != size) {
            self.list = sprites::latest();
            let mut textures = TexturesDelta::default();
            self.primitives = match self.list.clone() {
                Some(list) => self.layout(device, queue, &list, size, &mut textures),
                None => Vec::new(),
            };
            let renderer = &mut self.renderer.as_mut().unwrap().1;
            for (id, deltas) in &textures.set {
                for delta in deltas {
                    renderer.update_texture(device, queue, *id, delta);
                }
            }
            self.free.extend(textures.free.drain());
            textures.clear();
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
        if self.primitives.is_empty() {
            return false;
        }
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: size, pixels_per_point: 1.0 };
        self.renderer.as_mut().unwrap().1.update_buffers(device, queue, encoder, &self.primitives, &screen);
        true
    }

    /// Draws what [`Sprites::prepare`] laid out into `pass`.
    pub fn render(&mut self, pass: &mut wgpu::RenderPass<'static>, size: [u32; 2]) {
        let renderer = &mut self.renderer.as_mut().unwrap().1;
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: size, pixels_per_point: 1.0 };
        renderer.render(pass, &self.primitives, &screen);
        for id in self.free.drain(..) {
            renderer.free_texture(&id);
        }
    }

    /// The list as egui's primitives, in pixels; images drawn the first
    /// time are uploaded.
    fn layout(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        list: &[Sprite],
        size: [u32; 2],
        textures: &mut TexturesDelta,
    ) -> Vec<ClippedPrimitive> {
        let (w, h) = (size[0] as f32, size[1] as f32);
        let mut raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(w, h))),
            ..Default::default()
        };
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(1.0);
        let ctx = self.ctx.clone();
        ctx.begin_pass(raw);
        let painter = ctx.layer_painter(LayerId::background());
        for s in list {
            let center = pos2(s.center[0] * w, s.center[1] * h);
            let angle = s.rotation.to_radians();
            let color = Color32::from_rgba_unmultiplied(s.color[0], s.color[1], s.color[2], s.color[3]);
            match &s.shape {
                sprites::Shape::Image(image) => {
                    let Some(id) = self.texture(device, queue, image) else { continue };
                    let (iw, ih) = image.size();
                    let width = s.size[0] * w;
                    let height = if s.size[1] > 0.0 { s.size[1] * h } else { width * ih as f32 / iw as f32 };
                    let mut mesh = Mesh::with_texture(id);
                    let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
                    mesh.add_rect_with_uv(Rect::from_center_size(center, vec2(width, height)), uv, color);
                    mesh.rotate(Rot2::from_angle(angle), center);
                    painter.add(Shape::mesh(mesh));
                }
                sprites::Shape::Rect => {
                    let mut mesh = Mesh::default();
                    mesh.add_colored_rect(Rect::from_center_size(center, vec2(s.size[0] * w, s.size[1] * h)), color);
                    mesh.rotate(Rot2::from_angle(angle), center);
                    painter.add(Shape::mesh(mesh));
                }
                sprites::Shape::Text(text, align) => {
                    let anchor = match align {
                        Align::Left => Align2::LEFT_TOP,
                        Align::Center => Align2::CENTER_TOP,
                        Align::Right => Align2::RIGHT_TOP,
                    };
                    let px = (s.size[1] * h).clamp(1.0, h.max(1.0));
                    let galley = painter.layout_no_wrap(text.clone(), FontId::proportional(px), color);
                    let at = center - anchor.pos_in_rect(&galley.rect).to_vec2();
                    painter.add(TextShape::new(at, galley, color).with_angle_and_anchor(angle, anchor));
                }
            }
        }
        let output = ctx.end_pass();
        textures.append(output.textures_delta);
        ctx.tessellate(output.shapes, 1.0)
    }

    /// `image`'s texture, uploaded now if it is not yet.
    fn texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, image: &Arc<Image>) -> Option<egui::TextureId> {
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
