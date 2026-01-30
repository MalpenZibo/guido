# Renderer V2 - Next Steps

Based on the implementation plan at `~/.claude/plans/sunny-munching-moler.md`

## Current Status Summary

| Phase | Description | Status |
|-------|-------------|--------|
| Phase 1 | Core Infrastructure (tree, commands, context, flatten) | ✅ Complete |
| Phase 2 | GPU Rendering with Instancing | ✅ Complete |
| Phase 3 | Widget Integration (Container.paint_v2) | ✅ Complete |
| Phase 4 | Verification | 🔄 In Progress |

## Immediate Next Steps

### 1. Fix Clip Region in Shader (Priority: High)

**Issue**: The clip region code in `shader_v2.wgsl` was causing transformed (rotated/scaled) shapes to not render. It was disabled as a workaround.

**Location**: `src/renderer_v2/shader_v2.wgsl` lines 314-327 (commented out)

**Current workaround**:
```wgsl
// === Apply clip region (DISABLED for debugging) ===
// let clip_width = in.clip_rect.z;
// let clip_height = in.clip_rect.w;
// ...
```

**Investigation needed**:
- Why does clip code affect shapes even when `clip_width == 0` check should prevent it?
- May be related to how `screen_pos` is computed vs `frag_pos`
- Test with explicit no-clip sentinel value (e.g., negative width)

### 2. Add Text Rendering Support (Priority: Medium)

**What's needed**:
- Add `DrawCommand::Text` variant handling in `render.rs`
- Integrate glyphon text renderer (already used in V1)
- Pass text commands through the flattening pipeline

**Files to modify**:
- `src/renderer_v2/commands.rs` - Text variant exists but may need updates
- `src/renderer_v2/render.rs` - Add text rendering pass
- `src/widgets/text.rs` - Implement `paint_v2()` for Text widget

### 3. Add Circle Rendering for Ripples (Priority: Medium)

**What's needed**:
- Add `DrawCommand::Circle` support in shader
- Circle SDF is simple: `length(pos - center) - radius`
- Used for ripple effects in overlay layer

**Files to modify**:
- `src/renderer_v2/gpu.rs` - May need CircleInstance or extend ShapeInstance
- `src/renderer_v2/shader_v2.wgsl` - Add circle SDF path

### 4. Complete Verification Checklist (Priority: High)

Run through all verification items:

- [x] Simple colored boxes render
- [x] Boxes with borders render
- [x] Rotated boxes (15°) render
- [x] Scaled boxes (0.8) render
- [x] Squircle corners render
- [x] Scoop corners render
- [x] Boxes with shadows/elevation render
- [ ] Clickable boxes with ripple effect work
- [x] Nested containers render
- [x] HiDPI scaling works correctly
- [ ] Clipping works correctly (blocked by #1)
- [ ] Compare visual output with V1 renderer (take screenshots)

## Future Improvements

### Performance Optimization
- Batch shapes by layer to minimize state changes
- Consider texture atlas for text glyphs
- Profile memory usage vs V1 (target: ~128 bytes/instance vs ~768 bytes)

### Additional Features
- Gradient support in shapes
- Multiple clip regions (scissor stack)
- Anti-aliased transformed clips (requires SDF in screen space)

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
└── shader_v2.wgsl  # Instanced shader
```

### Test Example
```
examples/renderer_v2_test.rs
```

Run with:
```bash
cargo run --example renderer_v2_test --features renderer_v2
```

## Command Reference

```bash
# Build with V2 feature
cargo build --features renderer_v2

# Run test example
cargo run --example renderer_v2_test --features renderer_v2

# Check without running
cargo check --features renderer_v2

# Run clippy
cargo clippy --features renderer_v2
```
