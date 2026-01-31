//! Hierarchical Render Tree System (V2)
//!
//! A new rendering architecture that uses an explicit render tree instead of
//! stack-based push/pop for transforms and clips. This eliminates coordinate
//! system confusion and fragile ordering issues.
//!
//! # Architecture
//!
//! - Each widget creates a [`RenderNode`] with its local transform and draw commands
//! - World transforms are computed automatically by walking the tree during flatten
//! - Overlays (like ripples) naturally render after children
//!
//! Note: Clipping is temporarily disabled and will be re-implemented in a future PR.
//!
//! # Usage
//!
//! Enable with the `renderer_v2` feature:
//! ```bash
//! cargo run --example status_bar --features renderer_v2
//! ```

mod commands;
mod context;
mod flatten;
mod gpu;
mod image_quad;
mod render;
mod text_quad;
mod tree;

pub use commands::{Border, DrawCommand};
pub use context::PaintContextV2;
pub use flatten::{FlattenedCommand, flatten_tree};
pub use render::RendererV2;
pub use tree::{NodeId, RenderNode, RenderTree};
