//! GPU Renderer
//!
//! This module provides the hierarchical render tree architecture that uses an
//! explicit render tree instead of stack-based push/pop for transforms and clips.
//! This eliminates coordinate system confusion and fragile ordering issues.
//!
//! # Architecture
//!
//! - Each widget creates a [`RenderNode`] with its local transform and draw commands
//! - World transforms are computed automatically by walking the tree during flatten
//! - Overlays (like ripples) naturally render after children

mod backdrop_pass;
pub(crate) mod clip;
mod clip_records;
mod commands;
pub(crate) mod constants;
mod flatten;
mod gpu;
mod gpu_context;
mod image_atlas;
mod image_quad;
pub(crate) mod layer_blur;
mod layer_pass;
mod merge;
mod paint_context;
mod path;
mod path_mesh;
mod path_pipeline;
mod render;
mod text;
mod text_mask;
mod text_measurer;
mod text_quad;
mod textured_quad;
mod textured_vertex;
pub(crate) mod tree;
mod types;

pub use crate::image_decode::DecodedImage;
pub use clip::{ClipChains, ClipRef, NO_CHAIN, ResolvedClip};
pub use commands::{Border, CornerRadii, DrawCommand};
// `RenderLayer` is exported because `FlattenScratch`'s documentation links to
// it and rustdoc runs with warnings denied, a link the type has carried since
// before it was public. It is not a new hole: `FlattenedCommand::layer` has
// always handed one out, so a caller could read the field and not name what it
// got back.
pub use flatten::{
    CommandLayer, FlatFrame, FlattenScratch, FlattenedCommand, LayerPass, RegionsCarried,
    RenderLayer, ScratchCapacity, flatten_root_into,
};
pub(crate) use gpu::ANTIALIASING_REACH;
#[cfg(test)]
pub(crate) use gpu_context::GPU_GRACE;
pub(crate) use gpu_context::GpuSlot;
#[cfg(any(test, feature = "testing"))]
pub use gpu_context::OffscreenTarget;
pub use gpu_context::{GpuContext, RenderTarget, SurfaceState};
pub use merge::{MAX_MEMBERS, MergeChildren, MergeMember, MergedOutline};
pub use paint_context::PaintContext;
pub use path::{Cap, Dashes, Fill, FillRule, Join, Path, PathElement, Stroke};
pub use render::Renderer;
pub(crate) use text::ResolvedFont;
pub use text_measurer::{
    LineFit, Measured, char_index_from_x, char_index_from_x_styled, measure_char_positions_styled,
    measure_text, measure_text_full, measure_text_styled, measure_text_to_char,
    measure_text_to_char_styled,
};
pub(crate) use text_measurer::{TextMeasurer, measure_line_height};
pub use tree::{NodeId, OpacityMode, RenderNode};
pub use types::{Gradient, GradientDir, Shadow, Shadows, TextEntry};
