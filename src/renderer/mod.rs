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

mod commands;
mod constants;
mod flatten;
mod gpu;
mod gpu_context;
mod image_quad;
mod paint_context;
mod render;
mod text;
mod text_measurer;
mod text_quad;
mod textured_vertex;
mod tree;
mod types;

pub use commands::{Border, DrawCommand};
pub use flatten::{FlattenedCommand, flatten_tree, flatten_tree_into};
pub use gpu_context::{GpuContext, SurfaceState};
pub use paint_context::PaintContext;
pub use render::Renderer;
pub use text_measurer::{
    char_index_from_x, char_index_from_x_styled, measure_text, measure_text_styled,
    measure_text_to_char, measure_text_to_char_styled,
};
pub use tree::{NodeId, RenderNode, RenderTree};
pub use types::{Gradient, GradientDir, ImageEntry, Shadow, TextEntry};
