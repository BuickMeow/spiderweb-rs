//! Custom wgpu instanced note renderer: piano-roll notes move from the egui painter to one
//! instanced draw.
//!
//! - 16 bytes per instance: `start` (tick) | `end` (tick) | `meta` | `pad`,
//!   `meta = key | vel << 8 | slot << 16 | layer << 24` (layer 1 = notes of selected shapes).
//! - All coordinate transform happens in the vertex shader (see `note_gpu.wgsl`): pan / zoom
//!   only touch uniforms, and the instance buffer is only repacked when notes or selection
//!   change (`App::notes_revision`).
//! - Normal notes come first and selected notes after, in two draw calls, so the selection
//!   is drawn on top.
//! - Fallback: without a wgpu render state (`App::note_gpu == None`) the painter path is still used.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};

use eframe::egui_wgpu::{self, CallbackTrait, RenderState, ScreenDescriptor, wgpu};
use egui::{PaintCallback, PaintCallbackInfo, Pos2, Rect, Rgba};

use crate::roll::{SELECTED_COLOR, SLOT_COLORS, View};

/// 16 linear color sets: 15 slot colors + selection color.
type Palette = [[f32; 4]; 16];

/// One instance is 16 bytes, corresponding to `@location(0)` of the wgsl vertex entry point.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct NoteInstance {
    pub start: u32,
    pub end: u32,
    pub meta: u32,
    pub _pad: u32,
}

impl NoteInstance {
    /// Packs a rendered note row `(start, end, pitch, velocity, slot, owner)` into one instance.
    pub fn pack(n: &[i64; 6], layer: u32) -> Self {
        let start = n[0].max(0) as u32;
        let end = n[1].max(0) as u32;
        let key = n[2].clamp(0, 255) as u32;
        let vel = n[3].clamp(0, 127) as u32;
        let slot = (n[4].unsigned_abs() as u32) % SLOT_COLORS.len() as u32;
        Self {
            start,
            end,
            meta: key | (vel << 8) | (slot << 16) | ((layer & 0xff) << 24),
            _pad: 0,
        }
    }

    /// Test-only: accessors for the meta fields (production code only writes them).
    #[cfg(test)]
    pub fn key(&self) -> u32 {
        self.meta & 0xff
    }

    #[cfg(test)]
    pub fn vel(&self) -> u32 {
        (self.meta >> 8) & 0xff
    }

    #[cfg(test)]
    pub fn slot(&self) -> u32 {
        (self.meta >> 16) & 0xff
    }

    #[cfg(test)]
    pub fn layer(&self) -> u32 {
        (self.meta >> 24) & 0xff
    }
}

/// Packing result: the normal layer (layer 0) comes first, the selected layer (layer 1) after.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackedInstances {
    pub instances: Vec<NoteInstance>,
    /// The first `normal_count` are normal notes, the rest are selected notes (the split between the two draw calls).
    pub normal_count: usize,
}

/// Packs all rendered notes into instances (no visibility cull).
pub fn pack_instances(rendered: &[[i64; 6]], sels: &BTreeSet<usize>) -> PackedInstances {
    let mut normal: Vec<NoteInstance> = Vec::with_capacity(rendered.len());
    let mut selected: Vec<NoteInstance> = Vec::new();
    for n in rendered {
        let is_sel = usize::try_from(n[5])
            .map(|owner| sels.contains(&owner))
            .unwrap_or(false);
        let inst = NoteInstance::pack(n, u32::from(is_sel));
        if is_sel {
            selected.push(inst);
        } else {
            normal.push(inst);
        }
    }
    let normal_count = normal.len();
    normal.extend(selected);
    PackedInstances {
        instances: normal,
        normal_count,
    }
}

/// Per-frame uniform; the layout must match `Globals` in `note_gpu.wgsl` (std140 rules).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Globals {
    /// Canvas top-left (logical points, screen coordinates)
    pub canvas_min: [f32; 2],
    /// Physical pixels per logical point (overwritten in prepare from the ScreenDescriptor)
    pub ppp: f32,
    pub kb_w: f32,
    /// Tick at the left edge of the view
    pub origin_tick: u32,
    pub _pad0: u32,
    /// Logical points per tick = sx / ppq
    pub px_per_tick: f32,
    pub top: f32,
    pub sy: f32,
    pub ruler_h: f32,
    /// Screen size (physical pixels, overwritten in prepare from the ScreenDescriptor)
    pub screen_px: [f32; 2],
    /// Whether the target format is sRGB (overwritten in prepare from RenderState::target_format)
    pub srgb: u32,
    pub _pad1: u32,
    pub _pad2: [u32; 2],
    pub fill: Palette,
    pub outline: Palette,
}

/// Builds the uniform for the current view; `ppp / screen_px / srgb` are placeholders here and filled in by prepare.
pub fn globals_for(view: &View, ppq: i64, canvas_min: Pos2) -> Globals {
    let ppq_f = ppq.max(1) as f64;
    let (fill, outline) = palette();
    Globals {
        canvas_min: [canvas_min.x, canvas_min.y],
        ppp: 1.0,
        kb_w: view.kb_w,
        origin_tick: (view.t * ppq_f).round() as i64 as u32,
        _pad0: 0,
        px_per_tick: (view.sx / ppq_f) as f32,
        top: view.top as f32,
        sy: view.sy as f32,
        ruler_h: view.ruler_h,
        screen_px: [1.0, 1.0],
        srgb: 0,
        _pad1: 0,
        _pad2: [0, 0],
        fill,
        outline,
    }
}

/// Converts the palette to egui linear colors (15 slots + selection color).
fn palette() -> (Palette, Palette) {
    let mut fill = [[0.0f32; 4]; 16];
    let mut outline = [[0.0f32; 4]; 16];
    for (i, (f, o)) in SLOT_COLORS.iter().enumerate() {
        fill[i] = Rgba::from(*f).to_array();
        outline[i] = Rgba::from(*o).to_array();
    }
    fill[15] = Rgba::from(SELECTED_COLOR.0).to_array();
    outline[15] = Rgba::from(SELECTED_COLOR.1).to_array();
    (fill, outline)
}

/// Internal state: wgpu resources + CPU-side instances waiting to be uploaded.
struct Inner {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    globals_buffer: wgpu::Buffer,
    vertex_buffer: wgpu::Buffer,
    /// Current capacity of vertex_buffer (bytes)
    vertex_bytes: usize,
    cpu: Vec<NoteInstance>,
    normal_count: usize,
    /// true = CPU instances changed; re-upload in prepare (possibly growing the buffer first)
    pending_upload: bool,
    /// App::notes_revision of the last sync
    last_revision: u64,
}

/// Note GPU renderer: `Arc<RenderState>` + interior mutable state (pipeline / buffers / CPU instances).
pub struct NoteGpu {
    pub render_state: Arc<RenderState>,
    inner: Arc<Mutex<Inner>>,
}

impl NoteGpu {
    pub fn new(render_state: Arc<RenderState>) -> Self {
        let device = &render_state.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("note_gpu_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("note_gpu.wgsl").into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("note_gpu_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("note_gpu_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("note_gpu_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<NoteInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Uint32x4],
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            // eframe disables MSAA by default (NativeOptions::multisampling = 0), so sample count = 1
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: render_state.target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("note_gpu_globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("note_gpu_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });
        let vertex_bytes = 256 * std::mem::size_of::<NoteInstance>();
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("note_gpu_instances"),
            size: vertex_bytes as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            render_state,
            inner: Arc::new(Mutex::new(Inner {
                pipeline,
                bind_group,
                globals_buffer,
                vertex_buffer,
                vertex_bytes,
                cpu: Vec::new(),
                normal_count: 0,
                pending_upload: false,
                last_revision: u64::MAX,
            })),
        }
    }

    /// Repacks the CPU instances only when revision changed (no GPU upload here; prepare does the upload).
    pub fn sync(&self, rendered: &[[i64; 6]], sels: &BTreeSet<usize>, revision: u64) {
        let mut inner = self.lock();
        if inner.last_revision == revision {
            return;
        }
        let packed = pack_instances(rendered, sels);
        inner.cpu = packed.instances;
        inner.normal_count = packed.normal_count;
        inner.pending_upload = true;
        inner.last_revision = revision;
    }

    /// Builds this frame's paint callback; the uniform values are computed here and prepare only writes the buffer.
    pub fn callback(&self, globals: Globals, rect: Rect) -> PaintCallback {
        egui_wgpu::Callback::new_paint_callback(
            rect,
            NoteCallback {
                render_state: Arc::clone(&self.render_state),
                gpu: Arc::clone(&self.inner),
                globals,
            },
        )
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Paint callback registered each frame (egui calls it in the middle of the render pass).
struct NoteCallback {
    render_state: Arc<RenderState>,
    gpu: Arc<Mutex<Inner>>,
    globals: Globals,
}

impl CallbackTrait for NoteCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_descriptor: &ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        _callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let mut inner = self.gpu.lock().unwrap_or_else(|e| e.into_inner());
        if inner.pending_upload {
            let needed = inner.cpu.len() * std::mem::size_of::<NoteInstance>();
            if needed > inner.vertex_bytes {
                let cap = needed.next_power_of_two();
                inner.vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("note_gpu_instances"),
                    size: cap as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                inner.vertex_bytes = cap;
            }
            if needed > 0 {
                let bytes = bytemuck::cast_slice(&inner.cpu);
                queue.write_buffer(&inner.vertex_buffer, 0, bytes);
            }
            inner.pending_upload = false;
        }
        // Only the uniform is written every frame (pan / zoom leave the instance buffer alone)
        let mut g = self.globals;
        g.ppp = screen_descriptor.pixels_per_point;
        g.screen_px = [
            screen_descriptor.size_in_pixels[0] as f32,
            screen_descriptor.size_in_pixels[1] as f32,
        ];
        g.srgb = u32::from(self.render_state.target_format.is_srgb());
        queue.write_buffer(&inner.globals_buffer, 0, bytemuck::bytes_of(&g));
        Vec::new()
    }

    fn paint(
        &self,
        info: PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let inner = self.gpu.lock().unwrap_or_else(|e| e.into_inner());
        if inner.cpu.is_empty() {
            return;
        }
        let clip = info.clip_rect_in_pixels();
        if clip.width_px <= 0 || clip.height_px <= 0 {
            return;
        }
        // Don't draw outside the roll area (scissor in logical points × ppp)
        render_pass.set_scissor_rect(
            clip.left_px as u32,
            clip.top_px as u32,
            clip.width_px as u32,
            clip.height_px as u32,
        );
        // egui sets the default viewport to the callback rect, but the vertex shader computes
        // NDC for the whole screen, so restore the full-screen viewport explicitly for the
        // coordinates to line up.
        render_pass.set_viewport(
            0.0,
            0.0,
            info.screen_size_px[0] as f32,
            info.screen_size_px[1] as f32,
            0.0,
            1.0,
        );
        render_pass.set_pipeline(&inner.pipeline);
        render_pass.set_bind_group(0, &inner.bind_group, &[]);
        render_pass.set_vertex_buffer(0, inner.vertex_buffer.slice(..));
        let normal = (inner.normal_count as u32).min(inner.cpu.len() as u32);
        let total = inner.cpu.len() as u32;
        // Two draw calls: normal notes first, selected notes after (not relying on primitive order within one draw)
        if normal > 0 {
            render_pass.draw(0..4, 0..normal);
        }
        if total > normal {
            render_pass.draw(0..4, normal..total);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CPU mirror of the vertex shader's x computation (wrapping u32 subtraction + two's-complement i32 read).
    fn shader_x_px(g: &Globals, tick: i64, ppp: f32) -> f32 {
        let dt = (tick as u32).wrapping_sub(g.origin_tick) as i32 as f32;
        ((g.canvas_min[0] + g.kb_w + dt * g.px_per_tick) * ppp).round()
    }

    /// CPU mirror of the vertex shader's y computation (top / bottom edges, at least 1 physical pixel).
    fn shader_y_px(g: &Globals, key: i64, ppp: f32) -> (f32, f32) {
        let key = key as f32;
        let y0 = ((g.canvas_min[1] + g.ruler_h + (g.top - key - 0.5) * g.sy) * ppp).round();
        let y1 = ((g.canvas_min[1] + g.ruler_h + (g.top - key + 0.5) * g.sy) * ppp).round();
        (y0, y1.max(y0 + 1.0))
    }

    #[test]
    fn instance_packing_layout() {
        assert_eq!(std::mem::size_of::<NoteInstance>(), 16);
        let inst = NoteInstance::pack(&[960, 1920, 60, 127, 17, 3], 1);
        assert_eq!(inst.start, 960);
        assert_eq!(inst.end, 1920);
        assert_eq!(inst.key(), 60);
        assert_eq!(inst.vel(), 127);
        assert_eq!(inst.slot(), 2); // 17 % 15
        assert_eq!(inst.layer(), 1);
        assert_eq!(inst._pad, 0);
        // Negative ticks, out-of-range pitch / velocity and negative slots are clamped into range (256 keys: key up to 255)
        let weird = NoteInstance::pack(&[-5, -1, 200, 300, -16, 0], 0);
        assert_eq!(weird.start, 0);
        assert_eq!(weird.end, 0);
        assert_eq!(weird.key(), 200);
        assert_eq!(weird.vel(), 127);
        let max = NoteInstance::pack(&[0, 10, 999, 300, 0, 0], 0);
        assert_eq!(max.key(), 255);
        assert_eq!(weird.slot(), 1); // 16 % 15
        assert_eq!(weird.layer(), 0);
    }

    #[test]
    fn selected_layer_drawn_last() {
        let rendered = vec![
            [0, 10, 60, 100, 0, 0],
            [0, 10, 61, 100, 0, 1],
            [0, 10, 62, 100, 1, 2],
            [0, 10, 63, 100, 2, 1],
        ];
        let sels: BTreeSet<usize> = [1usize].into_iter().collect();
        let p = pack_instances(&rendered, &sels);
        assert_eq!(p.instances.len(), 4);
        assert_eq!(p.normal_count, 2);
        let layers: Vec<u32> = p.instances.iter().map(NoteInstance::layer).collect();
        assert_eq!(layers, vec![0, 0, 1, 1]);
        let keys: Vec<u32> = p.instances.iter().map(NoteInstance::key).collect();
        assert_eq!(keys, vec![60, 62, 61, 63]);
    }

    #[test]
    fn globals_match_view_and_shader_layout() {
        let view = View {
            t: 12.5,
            top: 96.25,
            sx: 40.0,
            sy: 7.0,
            ..Default::default()
        };
        let g = globals_for(&view, 480, Pos2::new(300.0, 50.0));
        assert_eq!(g.canvas_min, [300.0, 50.0]);
        assert_eq!(g.kb_w, view.kb_w);
        assert_eq!(g.ruler_h, view.ruler_h);
        assert_eq!(g.origin_tick, 6000); // 12.5 * 480
        assert_eq!(g.px_per_tick, 40.0 / 480.0);
        assert_eq!(g.top, 96.25);
        assert_eq!(g.sy, 7.0);
        // Palette: egui linear colors, the first 15 are slot colors and the 16th the selection color
        assert_eq!(g.fill[0], Rgba::from(SLOT_COLORS[0].0).to_array());
        assert_eq!(g.fill[14], Rgba::from(SLOT_COLORS[14].0).to_array());
        assert_eq!(g.outline[0], Rgba::from(SLOT_COLORS[0].1).to_array());
        assert_eq!(g.fill[15], Rgba::from(SELECTED_COLOR.0).to_array());
        assert_eq!(g.outline[15], Rgba::from(SELECTED_COLOR.1).to_array());
        // std140 layout (matching the naga-validated wgsl)
        assert_eq!(std::mem::size_of::<Globals>(), 576);
        assert_eq!(std::mem::offset_of!(Globals, screen_px), 40);
        assert_eq!(std::mem::offset_of!(Globals, _pad2), 56);
        assert_eq!(std::mem::offset_of!(Globals, fill), 64);
        assert_eq!(std::mem::offset_of!(Globals, outline), 320);
    }

    #[test]
    fn wrapping_sub_keeps_tick_difference_precision() {
        let view = View {
            t: 1_000_000.0,
            sx: 60.0,
            ..Default::default()
        };
        let g = globals_for(&view, 960, Pos2::new(120.0, 40.0));
        assert_eq!(g.origin_tick, 960_000_000);

        // Left edge of the view: x = canvas_min.x + kb_w
        let edge = shader_x_px(&g, 960_000_000, 1.0);
        assert_eq!(edge, 120.0 + view.kb_w);
        // Half a beat (480 ticks) before the left edge: lands exactly on edge - 30
        let before = shader_x_px(&g, 960_000_000 - 480, 1.0);
        assert_eq!(before, edge - 30.0);
        // Subtracting directly in f32 loses precision (ulp near 960_000_000 is 64 ticks)
        let naive = ((960_000_000i64 - 480) as f32 - 960_000_000.0f32) * g.px_per_tick;
        assert!(naive < -31.0, "naive = {naive}");
        // One beat after the right edge
        let after = shader_x_px(&g, 960_000_000 + 960, 1.0);
        assert_eq!(after, edge + 60.0);

        // y: round to whole physical pixels, at least 1 pixel; the ppp scale takes part
        let (y0, y1) = shader_y_px(&g, 60, 1.0);
        assert_eq!(
            y0,
            (40.0 + view.ruler_h + (127.5 - 60.0 - 0.5) * 6.0).round()
        );
        assert_eq!(
            y1,
            (40.0 + view.ruler_h + (127.5 - 60.0 + 0.5) * 6.0).round()
        );
        let (_, y1_min) = shader_y_px(&g, 60, 0.01);
        let (y0_min, _) = shader_y_px(&g, 60, 0.01);
        assert!(y1_min >= y0_min + 1.0);
    }
}
