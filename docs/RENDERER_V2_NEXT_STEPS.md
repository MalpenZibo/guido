# Renderer V2 - Status

## Current Status Summary (Updated Jan 2026)

| Phase | Description | Status |
|-------|-------------|--------|
| Phase 1 | Core Infrastructure (tree, commands, context, flatten) | ✅ Complete |
| Phase 2 | GPU Rendering with Instancing | ✅ Complete |
| Phase 3 | Widget Integration (Container.paint_v2) | ✅ Complete |
| Phase 4 | Text Rendering | ✅ Complete |
| Phase 5 | Clipping & Effects | ✅ Complete |

## Features

### Core Rendering
- **Instanced rendering**: All shapes rendered with a single draw call per layer
- **Local coordinate system**: Widgets draw in local space, transforms handled by GPU
- **SDF-based shapes**: Rounded rectangles with superellipse corners

### Clipping
- **GPU-based SDF clipping**: Supports rounded corners and curvature
- **Local clip mode**: For overlay effects on transformed containers (ripples)
- **Clip regions**: Set via `set_clip()` and `set_overlay_clip()` in PaintContextV2

### Effects
- **Ripple effects**: Circle-based overlays clipped to container bounds
- **Shadows**: Elevation-based drop shadows
- **Gradients**: Horizontal, vertical, and diagonal linear gradients

### Transforms
- **Full transform support**: Rotation, scale, translation
- **Transform composition**: Parent position + child user transform
- **Text with transforms**: Rendered via texture quads for rotation/scale

### Images
- **Image support**: Raster and SVG images with proper clipping
- **Content fit modes**: Cover, contain, fill, etc.

## Architecture

### Local Coordinate System

Widgets draw in their own local coordinate space:
- All widgets draw at `Rect::new(0, 0, width, height)` (local bounds)
- Parent sets child's position via `child_ctx.set_transform(translate(offset))`
- Child applies its own user transform via `ctx.apply_transform(user_transform)`
- Transforms are COMPOSED: parent position + child user transform

### Overlay Clipping for Transforms

For ripple effects on transformed containers:
- Overlay clips stay in LOCAL space (not transformed to world AABB)
- Shader uses `frag_pos` instead of `world_pos` for local clips
- Clip boundary follows container's rotation/scale

## Files Reference

### Core V2 Files
```
src/renderer_v2/
├── mod.rs          # Module exports
├── tree.rs         # RenderNode, RenderTree
├── commands.rs     # DrawCommand enum
├── context.rs      # PaintContextV2 API
├── flatten.rs      # Tree → flat commands
├── render.rs       # GPU rendering
├── gpu.rs          # ShapeInstance, uniforms
├── shader_v2.wgsl  # Instanced shape shader
├── text_quad.rs    # Textured quad renderer for transformed text
└── image_quad.rs   # Image rendering
```

## Command Reference

```bash
# Build with V2 feature
cargo build --features renderer_v2

# Run examples with V2 renderer
cargo run --example state_layer_example --features renderer_v2
cargo run --example renderer_v2_test --features renderer_v2

# Check and lint
cargo clippy --all-features

# Run tests
cargo test --all-features
```
