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

## Future Performance Improvements

### Relayout Boundaries (Level 2 Optimization)

**Problem:** Currently, dirty checking starts from the root widget and traverses down the entire tree. For complex UIs with many widgets, this becomes inefficient.

**Solution:** Implement "relayout boundaries" inspired by Flutter's rendering system.

**Concept:**
- A relayout boundary is a widget whose size is independent of its children's layout
- When a child of a boundary changes, only the subtree under that boundary needs relayout
- The boundary's parent and siblings are unaffected

**Examples of natural boundaries:**
- Fixed-size containers (`width: 100px, height: 50px`)
- Containers with `overflow: hidden` that clip children
- Flex children with `flex: 0` (don't grow/shrink)

**Implementation outline:**

1. **Add parent pointers to widgets:**
   ```rust
   struct WidgetNode {
       widget: Box<dyn Widget>,
       parent: Option<Weak<RefCell<WidgetNode>>>,
       is_relayout_boundary: bool,
   }
   ```

2. **Mark boundaries automatically:**
   - Container with fixed width AND height → boundary
   - Or allow explicit `.relayout_boundary(true)` builder method

3. **Dirty propagation:**
   ```rust
   fn mark_needs_layout(&mut self) {
       self.dirty_flags |= NEEDS_LAYOUT;
       // Walk up to nearest boundary, not root
       if !self.is_relayout_boundary {
           if let Some(parent) = &self.parent {
               parent.upgrade()?.borrow_mut().mark_needs_layout();
           }
       }
   }
   ```

4. **Layout phase:**
   - Only traverse from dirty boundaries downward
   - Maintain `dirty_boundaries: Vec<WidgetId>` instead of traversing from root

**Rust ownership challenges:**
- Parent pointers require `Weak<RefCell<T>>` or arena allocation
- Consider using `slotmap` or `generational-arena` crate for stable IDs
- Alternative: Store tree structure separately from widget data

**When to implement:**
- Profile shows >1ms in layout/paint phases
- Building complex UIs with 100+ widgets
- Scrolling lists or virtualized content

**Estimated effort:** 500-800 lines, 3-5 days
