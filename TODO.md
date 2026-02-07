I want create a rust gui library using wgpu and the wulkan renderer.
The primary scope is to create wayland widget using the layer shell protocol.

## Core Components

### Completed
- **Text**: Display text with reactive content and styling ✓
- **Container**: Unified widget with pluggable layout system ✓
  - Supports padding, background, gradients, borders, corner radius, shadows ✓
  - Reactive properties via MaybeDyn ✓
  - Event handlers (click, hover, scroll) ✓
  - Ripple effects ✓
  - **Layout Trait**: Pluggable layout system with Flex (row/column) ✓
  - **Static Children**: `.child()` and `.maybe_child()` methods ✓
  - **Dynamic Children**: `.children_dyn()` with keyed reconciliation (Floem-style) ✓

### Planned
- **Image**: Display images
- **Toggle/Checkbox**: Interactive toggle component
- **Input Text**: Text input field

## Architecture Improvements (Completed)

### Unified Container with Layout Trait (Jan 2026)
Replaced separate Row/Column widgets with a unified Container that accepts pluggable layouts:

- **Layout Trait**: Abstract interface for positioning children
- **Flex Layout**: Row and column layouts with reactive spacing and alignment
- **Children API**:
  - Static: `.child()`, `.maybe_child()` for simple cases
  - Dynamic: `.children_dyn()` with keyed reconciliation for preserving widget state
- **Benefits**:
  - Reduced code duplication
  - Single Container type with flexible layouts
  - State preservation during list reordering (via keyed reconciliation)
  - Easier to extend with custom layouts

The idea is that everything should be composed from these few component.

I want the library to be reactive so each props of these component should accept a fixed value, or a stream of values that should update only want is needed without recreating the whole tree.

It should be pretty so I would like to have an animation support using the hardware to optimize the performance.

---

## Completed Performance Improvements

### Relayout Boundaries ✓

Widgets with fixed width and height are automatically marked as relayout boundaries.
Layout changes inside a boundary don't propagate to the parent, reducing layout
recalculation scope. Dirty flags propagate upward only to the nearest boundary,
and `layout_roots` tracks which boundaries need layout.

### Partial Paint and Damage Tracking ✓

- `needs_paint` flag on Tree nodes, propagates upward like `needs_layout`
- Cached `RenderNode` per widget for paint reuse on clean children
- Skip-frame optimization when root doesn't need paint
- Wayland `damage_buffer()` reporting via `DamageRegion` accumulation
- Incremental flatten with `CachedFlatten` for clean subtrees

## Future Performance Improvements
