# V2 Renderer TODO

## Minor Issues

1. **Outdated comment** in `mod.rs:13-14`
   - Comment says clipping is disabled, but it's been re-implemented
   - Action: Remove or update the comment

2. **Unwrap calls** that could be more graceful:
   - `context.rs:351` - `self.node.children.last_mut().unwrap()`
   - `text_quad.rs:422` - `.expect("Failed to render text to texture")`

## Potential Improvements

1. **Overlay clipping with transform**
   - V1 has `draw_overlay_circle_clipped_with_transform()`
   - V2 has simpler overlay API without this variant
   - Consider adding if needed for complex ripple scenarios

2. **Unified `draw_shape()` method**
   - V1 accepts pre-built `RoundedRect` objects with builder pattern
   - V2 has separate specialized methods
   - Lower priority - current API is functional
