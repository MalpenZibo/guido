# Renderer V2 - Next Steps

## Current Status Summary (Updated Jan 2026)

| Phase | Description | Status |
|-------|-------------|--------|
| Phase 1 | Core Infrastructure (tree, commands, context, flatten) | ✅ Complete |
| Phase 2 | GPU Rendering with Instancing | ✅ Complete |
| Phase 3 | Widget Integration (Container.paint_v2) | ✅ Complete |
| Phase 4 | Text Rendering | ✅ Complete |
| Phase 5 | Verification | 🔄 In Progress |

## Recent Completions

### Text Rendering with Transforms (Jan 2026)

Added `text_quad.rs` module for rendering text with rotation/scale transforms:

- **Problem**: Glyphon can only handle translation transforms
- **Solution**: Render transformed text to offscreen textures, display as textured quads
- **Key features**:
  - 2x quality multiplier for crisp text at any scale
  - Per-quad vertex buffers (avoids render pass buffer timing issues)
  - Proper coordinate transformation from local to screen space
  - 10% buffer margin to prevent text clipping at scaled sizes

**Files added/modified**:
- `src/renderer_v2/text_quad.rs` - New textured quad renderer
- `src/renderer_v2/render.rs` - Integrated TextQuadRenderer
- `examples/simple_text_transform.rs` - Test example

### Clipping Removed (Jan 2026)

Clip region code was removed from the V2 renderer as it was causing issues with transformed shapes. Clipping may be re-implemented later with a different approach.

## Immediate Next Steps

### 1. Add Circle Rendering for Ripples (Priority: High)

**What's needed**:
- Add `DrawCommand::Circle` support in shader
- Circle SDF is simple: `length(pos - center) - radius`
- Used for ripple effects in overlay layer

**Files to modify**:
- `src/renderer_v2/gpu.rs` - May need CircleInstance or extend ShapeInstance
- `src/renderer_v2/shader_v2.wgsl` - Add circle SDF path
- `src/renderer_v2/render.rs` - Handle circle rendering

### 2. Re-implement Clipping (Priority: Medium)

**What's needed**:
- Design a clipping approach that works with transformed shapes
- Options:
  - Scissor rect (hardware, but axis-aligned only)
  - SDF-based clipping in shader (flexible but more complex)
  - Stencil buffer approach

### 3. Complete Verification Checklist (Priority: High)

- [x] Simple colored boxes render
- [x] Boxes with borders render
- [x] Rotated boxes (15°) render
- [x] Scaled boxes (0.8) render
- [x] Squircle corners render
- [x] Scoop corners render
- [x] Boxes with shadows/elevation render
- [x] Text with no transform renders (via glyphon)
- [x] Text with rotation renders (via text_quad)
- [x] Text with scale renders (via text_quad)
- [ ] Clickable boxes with ripple effect work
- [x] Nested containers render
- [x] HiDPI scaling works correctly
- [ ] Clipping works correctly
- [ ] Compare visual output with V1 renderer

## Known Issues

1. **No clipping support** - Clipping was removed due to issues with transformed shapes
2. **No ripple/circle support** - Circles not yet implemented in V2

## Files Reference

### Core V2 Files
```
src/renderer_v2/
├── mod.rs          # Module exports
├── tree.rs         # RenderNode, RenderTree
├── commands.rs     # DrawCommand enum
├── context.rs      # PaintContextV2 API
├── flatten.rs      # Tree → flat commands
├── render.rs       # GPU rendering + TextQuadRenderer integration
├── gpu.rs          # ShapeInstance, uniforms
├── shader_v2.wgsl  # Instanced shape shader
└── text_quad.rs    # Textured quad renderer for transformed text
```

### Test Examples
```
examples/renderer_v2_test.rs       # Basic V2 renderer test
examples/text_transform_example.rs # Text transform showcase
examples/simple_text_transform.rs  # Simple text transform test
```

## Command Reference

```bash
# Build with V2 feature
cargo build --features renderer_v2

# Run test examples
cargo run --example renderer_v2_test --features renderer_v2
cargo run --example text_transform_example --features renderer_v2
cargo run --example simple_text_transform --features renderer_v2

# Check and lint
cargo check --features renderer_v2
cargo clippy --features renderer_v2

# Run tests
cargo test --features renderer_v2
```
