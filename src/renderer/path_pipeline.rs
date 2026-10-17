//! Drawing a path: the cache of its tessellations, the frame's triangles, and
//! the pipeline that draws them.
//!
//! A path is tessellated once per shape and scale (`path_mesh.rs`) and kept,
//! keyed by its segments, how it is filled or stroked, and the surface scale —
//! the fringe is a physical pixel, so a new scale is a new mesh. A transform is
//! not part of the key: the mesh is in the path's own space, and the vertex
//! shader places it, so a path that moves, turns or fades draws the triangles
//! it already has.
//!
//! Each frame, every fill and stroke drawn copies its mesh into one vertex and
//! one index buffer, tagged with the index of a [`PathRecord`] — its colour,
//! its transform, its clip — and paths drawn back to back in a group are one
//! draw call. The copy is the price of keeping a mesh on the CPU, where it can
//! be shared between the paths that draw it; it is linear in vertices and
//! allocates nothing once the buffers have grown.

use std::collections::hash_map::Entry;
use std::ops::Range;

use rustc_hash::FxHashMap;
use wgpu::{BindGroup, BindGroupLayout, Buffer, Device, Queue, RenderPass, RenderPipeline};

use super::clip_records::ClipRecord;
use super::commands::DrawCommand;
use super::flatten::FlattenedCommand;
use super::path::{Cap, FillRule, Join, Path, Stroke};
use super::path_mesh::{PathMesh, Tessellator};
use crate::render_stats::{self, Pipeline};
use crate::widgets::Color;

/// One corner of a path's triangles, as the shader reads it.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PathVertex {
    /// Physical pixels, in the path's own space.
    pub position: [f32; 2],
    /// How much of the colour this corner carries — zero at the outer edge of
    /// the fringe.
    pub coverage: f32,
    /// The [`PathRecord`] of the fill or stroke this belongs to.
    pub record: u32,
}

impl PathVertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32, 2 => Uint32];
        wgpu::VertexBufferLayout {
            array_stride: size_of::<PathVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRIBUTES,
        }
    }
}

/// One fill or one stroke, as the shader reads it: what colour, placed where,
/// cut by what. Mirrors `PathRecord` in `path_shader.wgsl`, checked field for
/// field by the test beside it.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PathRecord {
    pub color: [f32; 4],
    /// The world transform, `[a, b, tx, c, d, ty]`, its translation in
    /// physical pixels — the mesh is already scaled.
    pub transform: [f32; 6],
    /// Where `clip` begins, because a `ClipRecord` aligns to sixteen bytes.
    pub _pad: [f32; 2],
    /// The clip, as the first link of its chain.
    pub clip: ClipRecord,
}

/// What a mesh was made from besides the path: one fill rule, or one stroke's
/// geometry. The colour is not here — it is the record's, so a path that
/// changes colour keeps its triangles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Style {
    Fill(FillRule),
    Stroke {
        width: u32,
        cap: Cap,
        join: Join,
        miter_limit: u32,
    },
}

/// One mesh a path command asks for: its fill, or its stroke.
#[derive(Clone, Copy)]
enum Drawn<'a> {
    Fill(FillRule),
    Stroke(&'a Stroke),
}

impl Drawn<'_> {
    fn style(self) -> Style {
        match self {
            Drawn::Fill(rule) => Style::Fill(rule),
            Drawn::Stroke(stroke) => Style::Stroke {
                width: stroke.width.to_bits(),
                cap: stroke.cap,
                join: stroke.join,
                miter_limit: stroke.miter_limit.to_bits(),
            },
        }
    }

    fn tessellate(self, tessellator: &mut Tessellator, path: &Path, scale: f32) -> PathMesh {
        match self {
            Drawn::Fill(rule) => tessellator.fill(path, rule, scale),
            Drawn::Stroke(stroke) => tessellator.stroke(path, stroke, scale),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct MeshKey {
    path: u64,
    style: Style,
    scale: u32,
}

struct CachedMesh {
    /// Kept to tell two paths apart whose hashes collide.
    path: Path,
    mesh: PathMesh,
    /// The frame that last drew it.
    used: u64,
}

/// Frames a mesh nobody drew is kept for, so that a path hidden for a moment
/// — a tab switched away and back — is not tessellated again.
const KEPT_FOR: u64 = 120;

/// How often the meshes are swept for the ones [`KEPT_FOR`] has passed.
const SWEEP_EVERY: u64 = 32;

pub(crate) struct PathRenderer {
    pipeline: RenderPipeline,
    layout: BindGroupLayout,
    /// The uniforms and this frame's records. Rebuilt when the record buffer
    /// grows, since a bind group holds the buffer it was made from.
    bind_group: Option<BindGroup>,

    vertex_buffer: Option<Buffer>,
    index_buffer: Option<Buffer>,
    record_buffer: Option<Buffer>,

    /// This frame's triangles and records, uploaded once before the pass.
    vertices: Vec<PathVertex>,
    indices: Vec<u32>,
    records: Vec<PathRecord>,

    meshes: FxHashMap<MeshKey, CachedMesh>,
    frame: u64,
    tessellator: Tessellator,
}

impl PathRenderer {
    pub(crate) fn new(
        device: &Device,
        format: wgpu::TextureFormat,
        clip_records: &BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Path Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("path_shader.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Path Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Sized for the reason `ClipRecords::ENTRY` gives: an unsized
                // storage binding is checked at every draw, which allocates.
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<PathRecord>() as u64),
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Path Pipeline Layout"),
            bind_group_layouts: &[Some(&layout), Some(clip_records)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Path Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(PathVertex::desc())],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(super::gpu::STRAIGHT_ALPHA_BLEND),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            layout,
            bind_group: None,
            vertex_buffer: None,
            index_buffer: None,
            record_buffer: None,
            vertices: Vec::new(),
            indices: Vec::new(),
            records: Vec::new(),
            meshes: FxHashMap::default(),
            frame: 0,
            tessellator: Tessellator::default(),
        }
    }

    /// Forget the last frame's triangles; keep the meshes.
    pub(crate) fn begin_frame(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.records.clear();
        self.frame += 1;
    }

    /// How many indices this frame holds — where the next path's triangles
    /// will begin, which is how a run of paths is addressed.
    pub(crate) fn index_count(&self) -> u32 {
        self.indices.len() as u32
    }

    /// Add a path command's fill, then its stroke, to this frame. Anything
    /// else adds nothing.
    pub(crate) fn push(&mut self, cmd: &FlattenedCommand, scale: f32) {
        let DrawCommand::Path { path, fill, stroke } = &*cmd.command else {
            return;
        };
        let transform = cmd.world_transform.physical(scale);
        let clip = cmd.clip().map_or(ClipRecord::NONE, |resolved| {
            ClipRecord::new(&resolved.first, resolved.rest, scale)
        });
        let record = |color: Color| {
            let color = color.scale_alpha(cmd.opacity);
            PathRecord {
                color: [color.r, color.g, color.b, color.a],
                transform,
                _pad: [0.0; 2],
                clip,
            }
        };
        if let Some(fill) = fill {
            self.push_mesh(path, Drawn::Fill(fill.rule), scale, record(fill.color));
        }
        if let Some(stroke) = stroke {
            self.push_mesh(path, Drawn::Stroke(stroke), scale, record(stroke.color));
        }
    }

    /// One mesh into the frame, from the cache or tessellated afresh.
    fn push_mesh(&mut self, path: &Path, drawn: Drawn<'_>, scale: f32, record: PathRecord) {
        let key = MeshKey {
            path: path.content_hash(),
            style: drawn.style(),
            scale: scale.to_bits(),
        };
        let cached = match self.meshes.entry(key) {
            Entry::Occupied(entry) => {
                let cached = entry.into_mut();
                let same = cached.path == *path;
                render_stats::record_path_mesh(same);
                if !same {
                    // Two paths with one hash: rare, and the newer one wins.
                    cached.mesh = drawn.tessellate(&mut self.tessellator, path, scale);
                }
                // The path drawing it now, so the next frame's comparison is a
                // pointer's rather than every segment's.
                cached.path = path.clone();
                cached
            }
            Entry::Vacant(entry) => {
                render_stats::record_path_mesh(false);
                entry.insert(CachedMesh {
                    path: path.clone(),
                    mesh: drawn.tessellate(&mut self.tessellator, path, scale),
                    used: 0,
                })
            }
        };
        cached.used = self.frame;
        let mesh = &cached.mesh;
        if mesh.indices.is_empty() {
            return;
        }

        let index = self.records.len() as u32;
        self.records.push(record);
        let base = self.vertices.len() as u32;
        self.vertices
            .extend(mesh.vertices.iter().map(|v| PathVertex {
                position: v.position,
                coverage: v.coverage,
                record: index,
            }));
        self.indices.extend(mesh.indices.iter().map(|i| base + i));
    }

    /// Write this frame's triangles and records to the GPU, before the pass
    /// opens, and drop the meshes nobody has drawn for a while. Allocates only
    /// when a frame holds more than any frame before it.
    pub(crate) fn upload(&mut self, device: &Device, queue: &Queue, uniforms: &Buffer) {
        // Swept now and then rather than every frame: a mesh kept a few
        // frames past its time costs memory, a scan of every mesh every frame
        // costs time.
        let frame = self.frame;
        if frame.is_multiple_of(SWEEP_EVERY) {
            self.meshes
                .retain(|_, cached| frame - cached.used <= KEPT_FOR);
        }
        if self.indices.is_empty() {
            return;
        }
        grow(
            device,
            &mut self.vertex_buffer,
            self.vertices.len() * size_of::<PathVertex>(),
            wgpu::BufferUsages::VERTEX,
            "Path Vertices",
        );
        grow(
            device,
            &mut self.index_buffer,
            self.indices.len() * size_of::<u32>(),
            wgpu::BufferUsages::INDEX,
            "Path Indices",
        );
        let records_grew = grow(
            device,
            &mut self.record_buffer,
            self.records.len() * size_of::<PathRecord>(),
            wgpu::BufferUsages::STORAGE,
            "Path Records",
        );
        let (Some(vertices), Some(indices), Some(records)) =
            (&self.vertex_buffer, &self.index_buffer, &self.record_buffer)
        else {
            return;
        };
        if records_grew {
            self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Path Bind Group"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: records.as_entire_binding(),
                    },
                ],
            }));
        }
        queue.write_buffer(vertices, 0, bytemuck::cast_slice(&self.vertices));
        queue.write_buffer(indices, 0, bytemuck::cast_slice(&self.indices));
        queue.write_buffer(records, 0, bytemuck::cast_slice(&self.records));
    }

    /// Draw the paths whose triangles are `indices` of this frame's, in one
    /// call.
    pub(crate) fn draw(
        &self,
        pass: &mut RenderPass<'_>,
        indices: Range<u32>,
        clip_records: &BindGroup,
    ) {
        let (Some(bind_group), Some(vertices), Some(index_buffer)) =
            (&self.bind_group, &self.vertex_buffer, &self.index_buffer)
        else {
            return;
        };
        if indices.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_bind_group(1, clip_records, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(indices, 0, 0..1);
        render_stats::record_draw_calls(Pipeline::Paths, 1);
    }
}

/// Make `buffer` hold at least `bytes`, doubling, and say whether it was made
/// anew.
fn grow(
    device: &Device,
    buffer: &mut Option<Buffer>,
    bytes: usize,
    usage: wgpu::BufferUsages,
    label: &str,
) -> bool {
    let capacity = buffer.as_ref().map_or(0, |b| b.size() as usize);
    if buffer.is_some() && bytes <= capacity {
        return false;
    }
    *buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes.max(capacity * 2).max(4096) as u64,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    }));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    /// The record is read by offset in the shader, so the two declarations
    /// have to agree to the byte — and its clip is a `ClipRecord`, laid out
    /// as the chain's own links are.
    #[test]
    fn the_shader_and_the_record_agree_field_for_field() {
        let rust = [
            ("color", offset_of!(PathRecord, color)),
            ("transform", offset_of!(PathRecord, transform)),
            ("clip", offset_of!(PathRecord, clip)),
        ];
        let (wgsl, size) = super::super::gpu::instance_layout::wgsl_fields(
            "src/renderer/path_shader.wgsl",
            "PathRecord",
        );
        assert_eq!(
            wgsl.iter()
                .map(|(name, offset)| (name.as_str(), *offset))
                .collect::<Vec<_>>(),
            rust
        );
        assert_eq!(size, size_of::<PathRecord>());
    }
}
