//! A frame's clip chains, where the shaders can walk them.
//!
//! A clip that could not be merged exactly with the clips above it keeps them
//! as a chain — see [`ResolvedClip`](super::clip::ResolvedClip). The first
//! shape of a chain travels with each instance or vertex, as one clip always
//! has; the rest are written here once per frame, as one storage buffer every
//! pipeline that cuts by a clip binds at group 1, and each instance names the
//! record its chain continues at.

use super::clip::ClipChains;
use crate::shape::PlacedShape;

use super::gpu::EMPTY_CLIP_RECT;

/// One link of a chain as a shader reads it. Mirrors `ClipRecord` in the
/// shared clip-chain block every shader carries, checked field for field by
/// the test beside it.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ClipRecord {
    /// `[x, y, width, height]` in the clip's own space, logical pixels — zero
    /// extents for a placement that collapsed, which lets nothing through.
    pub rect: [f32; 4],
    /// `[top_left, top_right, bottom_right, bottom_left]`, in that space.
    pub radii: [f32; 4],
    /// Physical world pixels to that space, `[a, b, tx, c, d, ty]`.
    pub inverse: [f32; 6],
    pub curvature: f32,
    /// The next record, or [`NO_CHAIN`](super::clip::NO_CHAIN).
    pub next: u32,
}

impl ClipRecord {
    /// No clip at all: the negative extents every shader reads as "nothing
    /// cuts this", and no chain after it. What a path drawn outside any clip
    /// carries, since its record always has a clip field to fill.
    pub(crate) const NONE: Self = Self {
        rect: super::gpu::NO_CLIP_RECT,
        radii: [0.0; 4],
        inverse: crate::transform::Transform::IDENTITY.data,
        curvature: 1.0,
        next: super::clip::NO_CHAIN,
    };

    /// `shape` as a shader cuts by it, at the surface's `scale`, continuing
    /// at `next`. The one place a clip is written for a GPU to read, beside
    /// the instance's own `with_clip`, which is kept inline for its cost.
    pub(crate) fn new(shape: &PlacedShape, next: u32, scale: f32) -> Self {
        let Some(inverse) = shape.to_local(scale) else {
            return Self {
                rect: EMPTY_CLIP_RECT,
                radii: [0.0; 4],
                inverse: crate::transform::Transform::IDENTITY.data,
                curvature: 1.0,
                next,
            };
        };
        let r = shape.rect;
        Self {
            rect: [r.x, r.y, r.width, r.height],
            radii: shape.radii.to_array(),
            inverse: inverse.data,
            curvature: shape.curvature,
            next,
        }
    }
}

/// The records buffer and the bind group every clipping pipeline reads it
/// through.
pub(crate) struct ClipRecords {
    layout: wgpu::BindGroupLayout,
    buffer: wgpu::Buffer,
    /// Records the buffer holds.
    capacity: usize,
    bind_group: wgpu::BindGroup,
    /// This frame's records, kept so the next frame fills the same room.
    staging: Vec<ClipRecord>,
}

/// Records the buffer starts with. A binding cannot be empty, and a frame
/// with no chain at all — most of them — binds it all the same.
const INITIAL_RECORDS: usize = 16;

impl ClipRecords {
    /// The one binding of group 1.
    ///
    /// **Its size is declared, not left to the pipeline.** A storage binding
    /// with no `min_binding_size` is checked late, and wgpu-core keeps that
    /// check in a list each render pass builds afresh: one allocation every
    /// frame, chain or no chain, which is how a frame with no chain in it came
    /// to allocate more than it did before chains existed. One record is the
    /// least the buffer ever holds, so it is the size to declare.
    const ENTRY: wgpu::BindGroupLayoutEntry = wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(size_of::<ClipRecord>() as u64),
        },
        count: None,
    };

    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Clip Records Layout"),
            entries: &[Self::ENTRY],
        });
        let (buffer, bind_group) = Self::allocate(device, &layout, INITIAL_RECORDS);
        Self {
            layout,
            buffer,
            capacity: INITIAL_RECORDS,
            bind_group,
            staging: Vec::new(),
        }
    }

    fn allocate(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        capacity: usize,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Clip Records"),
            size: (capacity * size_of::<ClipRecord>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Clip Records Bind Group"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        (buffer, bind_group)
    }

    /// The layout of group 1, for a pipeline that cuts by a clip.
    pub(crate) fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// The bind group to set at group 1.
    pub(crate) fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    /// Write this frame's chains, at this frame's scale.
    pub(crate) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        chains: &ClipChains,
        scale: f32,
    ) {
        self.staging.clear();
        self.staging.extend(
            chains
                .links()
                .iter()
                .map(|link| ClipRecord::new(&link.shape, link.next, scale)),
        );
        if self.staging.is_empty() {
            return;
        }
        if self.staging.len() > self.capacity {
            self.capacity = (self.capacity * 2).max(self.staging.len());
            (self.buffer, self.bind_group) = Self::allocate(device, &self.layout, self.capacity);
        }
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&self.staging));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    /// The record is one layout written four times — here and in each shader
    /// that walks a chain — and a field read from the wrong bytes cuts by the
    /// wrong shape. The shaders' three copies are held to each other by
    /// `tests/shader_sdf_is_one_definition.rs`; this holds the first to Rust.
    #[test]
    fn the_shader_and_the_record_agree_field_for_field() {
        let rust = [
            ("rect", offset_of!(ClipRecord, rect)),
            ("radii", offset_of!(ClipRecord, radii)),
            ("inverse", offset_of!(ClipRecord, inverse)),
            ("curvature", offset_of!(ClipRecord, curvature)),
            ("next", offset_of!(ClipRecord, next)),
        ];
        let (wgsl, size) = super::super::gpu::instance_layout::wgsl_fields(
            "src/renderer/shader.wgsl",
            "ClipRecord",
        );
        assert_eq!(
            wgsl.iter()
                .map(|(name, offset)| (name.as_str(), *offset))
                .collect::<Vec<_>>(),
            rust
        );
        assert_eq!(size, size_of::<ClipRecord>());
        assert_eq!(size, 64, "four records to a cache line");
    }

    /// The binding says how large a record is, so wgpu checks it when the
    /// bind group is made rather than at every draw. Left unsized, it costs
    /// every frame an allocation whether or not it has a chain to walk.
    /// Measured with `CountingAllocator` over `benches/static_clip`'s
    /// overflow-hidden play, twenty frames a phase, on lavapipe: 9306 frame
    /// allocations before chains existed, 9386 with the binding unsized, 9306
    /// with it sized.
    #[test]
    fn the_records_binding_is_sized_so_no_frame_pays_to_check_it() {
        let wgpu::BindingType::Buffer {
            min_binding_size, ..
        } = ClipRecords::ENTRY.ty
        else {
            panic!("the records binding is a buffer");
        };
        assert_eq!(
            min_binding_size.map(|size| size.get()),
            Some(size_of::<ClipRecord>() as u64)
        );
    }

    /// A record keeps the clip in the logical units it was declared in and
    /// carries the surface scale in its map, as the instance's own clip does;
    /// a placement that collapsed lets nothing through.
    #[test]
    fn a_record_is_scaled_by_its_map_and_a_collapsed_one_cuts_it_all() {
        use crate::transform::Transform;
        let shape = PlacedShape::placed(
            crate::widgets::Rect::new(10.0, 20.0, 100.0, 50.0),
            crate::renderer::CornerRadii::uniform(4.0),
            2.0,
            Transform::IDENTITY,
        );
        let record = ClipRecord::new(&shape, 7, 2.0);
        assert_eq!(record.rect, [10.0, 20.0, 100.0, 50.0]);
        assert_eq!(record.radii, [4.0; 4]);
        assert_eq!((record.curvature, record.next), (2.0, 7));
        let [a, b, tx, c, d, ty] = record.inverse;
        assert_eq!(
            (a * 20.0 + b * 40.0 + tx, c * 20.0 + d * 40.0 + ty),
            (10.0, 20.0)
        );

        let collapsed = PlacedShape {
            placement: Transform::scale(0.0),
            ..shape
        };
        let record = ClipRecord::new(&collapsed, 7, 2.0);
        assert_eq!(record.rect, EMPTY_CLIP_RECT);
        assert_eq!(record.next, 7, "and the chain goes on past it");
    }
}
