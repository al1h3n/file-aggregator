//! Liquid Glass under the egui interface.
//!
//! egui draws text and widgets; [liquid-rust](https://crates.io/crates/liquid-rust) draws
//! the glass shapes behind them. Every frame:
//!
//! 1. The app moves glass in the shared [`Scene`] to where its widgets are (`GlassLayer::lock`).
//! 2. egui runs our paint callback first, on its background layer:
//!    - `prepare`: if the background or window size changed, draw the background into the
//!      `content` texture; if anything changed at all, let liquid-rust refract `content`
//!      into the `glass` texture. When nothing moved, both steps are skipped.
//!    - `paint`: copy `glass` into egui's render pass (one full-screen triangle).
//! 3. egui draws the widgets on top.
//!
//! Costs at rest: one texture copy per egui repaint, and no repaints at all while the
//! window is idle. liquid-rust also keeps the blur of a background that holds still.

use std::sync::{Arc, Mutex, MutexGuard};

use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
use liquid_rust::{Appearance, ContainerId, Glass, GlassId, Material, Rect, Renderer, Scene, Spring};

use super::background::{self, Background};

/// Texture format of the content and glass textures. sRGB so liquid-rust reads linear colour.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Converts an egui rectangle (points) to a liquid-rust one (points).
pub fn rect(r: egui::Rect) -> Rect {
    Rect::new(r.min.x, r.min.y, r.width(), r.height())
}

/// Converts back, e.g. to clip text to glass that is still growing.
pub fn egui_rect(r: Rect) -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(r.x, r.y), egui::vec2(r.width, r.height))
}

/// The scene plus what changed since the GPU last drew it. Shared by the app and the paint callback.
pub struct Shared {
    scene: Scene,
    /// The glass must be drawn again (something moved or changed).
    redraw: bool,
    background: Background,
    /// The background must be drawn again (new background or a photo upload).
    background_dirty: bool,
    /// Photo waiting to be uploaded to the GPU.
    photo_upload: Option<image::RgbaImage>,
    /// A press landed on interactive glass (its glow follows the pointer until release).
    pressing: bool,
}

/// Handle the app keeps; cloning it only clones an `Arc`.
#[derive(Clone)]
pub struct GlassLayer {
    shared: Arc<Mutex<Shared>>,
}

impl GlassLayer {
    /// Creates the GPU resources. `None` when eframe isn't running on wgpu.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Option<Self> {
        let render_state = cc.wgpu_render_state.as_ref()?;
        let gpu = GlassGpu::new(&render_state.device, render_state.target_format);
        render_state.renderer.write().callback_resources.insert(gpu);

        let mut scene = Scene::new();
        // Apple's rule: glass can't sample glass. A popover over the panel refracts the
        // background, never ghosts of the panel beneath it.
        scene.glass_on_glass = false;
        let shared = Shared { scene, redraw: true, background: Background::default(), background_dirty: true, photo_upload: None, pressing: false };
        Some(Self { shared: Arc::new(Mutex::new(shared)) })
    }

    /// Exclusive access for this frame's changes.
    pub fn lock(&self) -> MutexGuard<'_, Shared> {
        // A panic while holding the lock leaves plain data behind; keep using it.
        self.shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Queues the glass to be drawn behind everything egui draws this frame.
    pub fn paint(&self, ctx: &egui::Context) {
        let callback = egui_wgpu::Callback::new_paint_callback(ctx.content_rect(), GlassCallback { shared: Arc::clone(&self.shared) });
        ctx.layer_painter(egui::LayerId::background()).add(callback);
    }
}

/// Every change goes through these methods so `redraw` is set exactly when needed.
impl Shared {
    pub fn add_container(&mut self, spacing: f32, z: i32) -> ContainerId {
        self.scene.add_container(spacing, z)
    }

    pub fn add(&mut self, glass: Glass) -> GlassId {
        self.redraw = true;
        self.scene.add(glass)
    }

    pub fn remove(&mut self, id: GlassId) {
        self.redraw = true;
        self.scene.remove(id);
    }

    /// Moves glass toward `frame`: springs when `animate`, jumps otherwise (window resize).
    /// Does nothing (and keeps the frame cached) when it is already headed there.
    pub fn frame_to(&mut self, id: GlassId, frame: egui::Rect, animate: bool) {
        let frame = rect(frame);
        if self.scene.glass(id).is_some_and(|g| g.frame == frame) {
            return;
        }
        self.redraw = true;
        if animate { self.scene.set_frame(id, frame) } else { self.scene.place(id, frame) }
    }

    pub fn set_material(&mut self, id: GlassId, material: Material) {
        if self.scene.glass(id).is_some_and(|g| g.material != material) {
            self.redraw = true;
            self.scene.set_material(id, material);
        }
    }

    /// Grows new glass out of `source` (a toast out of the toolbar).
    pub fn expand_from(&mut self, source: GlassId, glass: Glass, spring: Spring) -> GlassId {
        self.redraw = true;
        self.scene.expand_from(source, glass, spring)
    }

    /// Folds glass back into `target` and drops it.
    pub fn collapse_into(&mut self, id: GlassId, target: GlassId, spring: Spring) {
        self.redraw = true;
        self.scene.collapse_into(id, target, spring);
    }

    /// Frame on screen right now, mid-animation.
    pub fn current_frame(&self, id: GlassId) -> Option<egui::Rect> {
        self.scene.current_frame(id).map(egui_rect)
    }

    pub fn set_appearance(&mut self, appearance: Appearance) {
        if self.scene.appearance != appearance {
            self.scene.appearance = appearance;
            self.redraw = true;
        }
    }

    pub fn pointer_down(&mut self, p: egui::Pos2) {
        self.pressing = self.scene.pointer_down(liquid_rust::Vec2::new(p.x, p.y)).is_some();
        self.redraw |= self.pressing;
    }

    /// Only matters while a press on glass is held (a drag elsewhere, like selecting
    /// text, doesn't redraw the glass).
    pub fn pointer_move(&mut self, p: egui::Pos2) {
        if self.pressing {
            self.scene.pointer_move(liquid_rust::Vec2::new(p.x, p.y));
            self.redraw = true;
        }
    }

    pub fn pointer_up(&mut self) {
        if std::mem::take(&mut self.pressing) {
            self.scene.pointer_up();
            self.redraw = true;
        }
    }

    /// Advances the springs. `true` while anything moves: request another frame.
    pub fn update(&mut self, dt: f32) -> bool {
        let moving = self.scene.update(dt);
        self.redraw |= moving;
        moving
    }

    /// Switches the background; a photo comes with its decoded pixels.
    pub fn set_background(&mut self, background: Background, photo: Option<image::RgbaImage>) {
        if photo.is_some() {
            self.photo_upload = photo;
        } else if self.background == background {
            return;
        }
        self.background = background;
        self.background_dirty = true;
    }
}

struct GlassCallback {
    shared: Arc<Mutex<Shared>>,
}

impl egui_wgpu::CallbackTrait for GlassCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(gpu) = resources.get_mut::<GlassGpu>() {
            let mut shared = self.shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            gpu.prepare(device, queue, encoder, screen, &mut shared);
        }
        Vec::new()
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, resources: &egui_wgpu::CallbackResources) {
        if let Some(gpu) = resources.get::<GlassGpu>() {
            gpu.paint(pass);
        }
    }
}

/// GPU side, stored in egui-wgpu's callback resources for the life of the window.
struct GlassGpu {
    renderer: Renderer,
    background_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    sampler: wgpu::Sampler,
    /// Photo (or a 1×1 placeholder) and its size in pixels.
    photo: wgpu::TextureView,
    photo_size: (u32, u32),
    background_group: wgpu::BindGroup,
    targets: Option<Targets>,
}

/// Window-sized textures, recreated when the window size changes.
struct Targets {
    size: [u32; 2],
    content: wgpu::TextureView,
    glass: wgpu::TextureView,
    blit_group: wgpu::BindGroup,
}

/// Uniforms of `fs_background` (layout must match `background.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BackgroundUniform {
    a: [f32; 4],
    b: [f32; 4],
    photo: [f32; 4],
    info: [f32; 4],
}

fn texture(device: &wgpu::Device, label: &str, size: [u32; 2], usage: wgpu::TextureUsages) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn pipeline(device: &wgpu::Device, module: &wgpu::ShaderModule, fragment: &str, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment),
        layout: None,
        vertex: wgpu::VertexState { module, entry_point: Some("vs_full"), compilation_options: Default::default(), buffers: &[] },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl GlassGpu {
    fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("file-aggregator background"),
            source: wgpu::ShaderSource::Wgsl(include_str!("background.wgsl").into()),
        });
        let blit_entry = if target_format.is_srgb() { "fs_blit_linear" } else { "fs_blit_encode" };
        let background_pipeline = pipeline(device, &module, "fs_background", FORMAT);
        let blit_pipeline = pipeline(device, &module, blit_entry, target_format);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("background uniforms"),
            size: size_of::<BackgroundUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("photo"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let photo = texture(device, "photo placeholder", [1, 1], wgpu::TextureUsages::TEXTURE_BINDING);
        let background_group = Self::background_group(device, &background_pipeline, &uniforms, &sampler, &photo);
        Self {
            renderer: Renderer::new(device, FORMAT),
            background_pipeline,
            blit_pipeline,
            uniforms,
            sampler,
            photo,
            photo_size: (1, 1),
            background_group,
            targets: None,
        }
    }

    fn background_group(
        device: &wgpu::Device,
        pipeline: &wgpu::RenderPipeline,
        uniforms: &wgpu::Buffer,
        sampler: &wgpu::Sampler,
        photo: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("background"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(photo) },
            ],
        })
    }

    fn upload_photo(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, image: &image::RgbaImage) {
        let (width, height) = image.dimensions();
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("photo"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            image.as_raw(),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * width), rows_per_image: Some(height) },
            size,
        );
        self.photo = texture.create_view(&Default::default());
        self.photo_size = (width, height);
        self.background_group = Self::background_group(device, &self.background_pipeline, &self.uniforms, &self.sampler, &self.photo);
    }

    fn uniform(&self, background: &Background, size: [u32; 2]) -> BackgroundUniform {
        let (w, h) = (size[0] as f32, size[1] as f32);
        let info = |mode: f32| [mode, w, h, 0.0];
        let vec4 = |c: [f32; 3]| [c[0], c[1], c[2], 1.0];
        match background {
            Background::Solid(c) => BackgroundUniform { a: vec4(background::linear(*c)), b: [0.0; 4], photo: [0.0; 4], info: info(0.0) },
            Background::Gradient(a, b) => {
                BackgroundUniform { a: vec4(background::oklab(*a)), b: vec4(background::oklab(*b)), photo: [0.0; 4], info: info(1.0) }
            }
            Background::Photo(_) => {
                // Cover: scale the photo until it fills the window, centre it, crop the rest.
                let (pw, ph) = (self.photo_size.0 as f32, self.photo_size.1 as f32);
                let scale = (w / pw).max(h / ph);
                let (sw, sh) = (pw * scale, ph * scale);
                let photo = [1.0 / sw, 1.0 / sh, (sw - w) * 0.5 / sw, (sh - h) * 0.5 / sh];
                BackgroundUniform { a: [0.0; 4], b: [0.0; 4], photo, info: info(2.0) }
            }
        }
    }

    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        screen: &egui_wgpu::ScreenDescriptor,
        shared: &mut Shared,
    ) {
        let size = screen.size_in_pixels;
        if size[0] == 0 || size[1] == 0 {
            return; // minimized
        }
        let mut content_changed = false;
        if self.targets.as_ref().is_none_or(|t| t.size != size) {
            let content = texture(device, "content", size, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
            let glass = texture(device, "glass", size, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
            let blit_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("blit"),
                layout: &self.blit_pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&glass) }],
            });
            self.targets = Some(Targets { size, content, glass, blit_group });
            content_changed = true;
        }
        if let Some(photo) = shared.photo_upload.take() {
            self.upload_photo(device, queue, &photo);
            content_changed = true;
        } else if !matches!(shared.background, Background::Photo(_)) && self.photo_size != (1, 1) {
            // Switched away from a photo: free its texture (up to ~60 MB at 4K).
            self.photo = texture(device, "photo placeholder", [1, 1], wgpu::TextureUsages::TEXTURE_BINDING);
            self.photo_size = (1, 1);
            self.background_group = Self::background_group(device, &self.background_pipeline, &self.uniforms, &self.sampler, &self.photo);
        }
        content_changed |= std::mem::take(&mut shared.background_dirty);
        let Some(targets) = &self.targets else { return };

        if content_changed {
            queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&self.uniform(&shared.background, size)));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("background"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.content,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.background_pipeline);
            pass.set_bind_group(0, &self.background_group, &[]);
            pass.draw(0..3, 0..1);
        }
        if content_changed || shared.redraw {
            let frame = liquid_rust::Frame {
                content: &targets.content,
                target: &targets.glass,
                size: (size[0], size[1]),
                scale: screen.pixels_per_point,
                content_changed,
            };
            self.renderer.render(device, queue, encoder, &frame, &shared.scene);
            shared.redraw = false;
        }
    }

    fn paint(&self, pass: &mut wgpu::RenderPass<'static>) {
        if let Some(targets) = &self.targets {
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &targets.blit_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    /// The shader is checked by naga at test time, so a WGSL mistake fails `cargo test`
    /// even on a machine without a GPU.
    #[test]
    fn background_shader_is_valid() {
        use eframe::egui_wgpu::wgpu::naga;
        let source = include_str!("background.wgsl");
        let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
        // No optional capabilities: must run on baseline WebGPU / GL.
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("WGSL validation error: {e:?}"));
        for entry in ["vs_full", "fs_background", "fs_blit_encode", "fs_blit_linear"] {
            assert!(module.entry_points.iter().any(|e| e.name == entry), "missing entry point {entry}");
        }
    }
}
