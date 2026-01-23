# Ripple Effect Design Document

This document describes how the ripple effect feature should be implemented in Guido.

## Overview

Ripple effects provide visual feedback for user interactions (hover and click) on containers. The effect consists of an expanding circle that originates from the interaction point and spreads to fill the container bounds.

## Types of Ripple Effects

### 1. Hover Ripple
- Triggered when the mouse enters a container
- Circle expands from the entry point to fill the container
- On mouse exit, the ripple fades out from the last known mouse position
- Alpha decreases as the ripple expands during exit

### 2. Click Ripple
- Triggered on mouse down
- Circle expands from the click point
- On mouse up, the ripple reverses (shrinks back) toward the release point
- More pronounced visual feedback than hover ripple

## Visual Behavior

### Animation Parameters
```rust
const RIPPLE_ENTER_SPEED: f32 = 0.12;      // Progress per frame during hover enter
const RIPPLE_EXIT_SPEED: f32 = 0.20;       // Progress per frame during hover exit
const CLICK_RIPPLE_EXPAND_SPEED: f32 = 0.08;   // Progress per frame during click expand
const CLICK_RIPPLE_REVERSE_SPEED: f32 = 0.15;  // Progress per frame during click reverse
```

### Radius Calculation
The ripple radius is calculated as the maximum distance from the ripple center to any corner of the container:
```rust
let dx1 = center_x - bounds.x;
let dx2 = (bounds.x + bounds.width) - center_x;
let dy1 = center_y - bounds.y;
let dy2 = (bounds.y + bounds.height) - center_y;
let max_radius = (dx1.max(dx2).powi(2) + dy1.max(dy2).powi(2)).sqrt();
let current_radius = max_radius * progress;
```

### Alpha Calculation
- **Hover enter**: `alpha = base_alpha * (1.0 - progress * 0.5)` - slight fade as it expands
- **Hover exit**: `alpha = base_alpha * (1.0 - progress).powi(2)` - quadratic fade out
- **Click**: `alpha = base_alpha * (1.0 - progress * 0.5)` - slight fade as it expands/contracts

## Clipping Requirements

The ripple circle MUST be clipped to the container's visual boundary:
- Respect the container's corner radius
- Respect the container's border (clip inside the border, not outside)
- Respect superellipse curvature (K-value) for non-circular corners

### Challenge: Transforms

When a container has a transform (translate, rotate, scale), clipping becomes complex:

1. **Translation**: Works correctly - the clip rect translates with the container
2. **Scale**: Works correctly - the clip rect scales with the container
3. **Rotation**: **Challenging** - an axis-aligned clip rect cannot represent a rotated container boundary

#### Rotation Clipping Options

**Option A: Evaluate SDF in local space**
- Pass the inverse transform to the shader
- Transform `local_pos` back to container-local coordinates before evaluating clip SDF
- The clip rect is always axis-aligned in local space
- **Pros**: Mathematically correct
- **Cons**: Requires additional vertex attributes or uniform, shader complexity

**Option B: Use transformed clip polygon**
- Pre-compute the 4 corners of the rotated clip rect
- Pass all 4 corners to the shader
- Use half-plane SDFs to clip to the rotated quad
- **Pros**: Accurate for rectangular containers
- **Cons**: More complex shader, doesn't handle rounded corners well

**Option C: Skip clipping for rotated containers**
- Detect rotation and disable clipping
- Ripple may extend slightly outside container bounds
- **Pros**: Simple implementation
- **Cons**: Visual imperfection

**Recommended approach**: Option A (evaluate SDF in local space) for proper correctness.

## Rendering Layer

Ripples are rendered as **overlay shapes** - drawn on top of text and other content within the container. This ensures the ripple effect is visible over all child content.

The renderer needs an `overlay_shapes` layer that is rendered after the text layer:
1. Background shapes (rounded rects, etc.)
2. Text
3. Overlay shapes (ripples)

## API Design

```rust
impl Container {
    /// Enable ripple effect with default white color
    pub fn ripple(mut self) -> Self {
        self.ripple_enabled = true;
        self
    }

    /// Enable ripple effect with custom color
    pub fn ripple_with_color(mut self, color: Color) -> Self {
        self.ripple_enabled = true;
        self.ripple_color = color;
        self
    }
}
```

## Container State

```rust
struct Container {
    // Ripple configuration
    ripple_enabled: bool,
    ripple_color: Color,

    // Hover ripple state
    ripple_center: Option<(f32, f32)>,  // Current ripple center position
    ripple_progress: f32,                // 0.0 to 1.0 animation progress
    ripple_from_click: bool,             // Tracks if current ripple originated from click
    ripple_is_exit: bool,                // True when animating exit
    last_mouse_pos: Option<(f32, f32)>,  // For exit animation positioning

    // Click ripple state
    click_ripple_center: Option<(f32, f32)>,
    click_ripple_progress: f32,
    click_ripple_reversing: bool,
    click_ripple_release_pos: Option<(f32, f32)>,
}
```

## Event Handling

### MouseEnter
- If not already rippling, start hover ripple from entry point
- Set `ripple_center = Some((x, y))`
- Set `ripple_progress = 0.0`

### MouseMove
- If hover state changes (entered or exited bounds):
  - On enter: Start new ripple at entry point
  - On exit: Start exit animation from last known position

### MouseDown
- Start click ripple at click point
- Set `click_ripple_center = Some((x, y))`
- Set `click_ripple_progress = 0.0`

### MouseUp
- Begin click ripple reverse animation
- Set `click_ripple_reversing = true`
- Store release position for center interpolation

### MouseLeave
- Start exit animation from last mouse position

## Animation Integration

Ripple animations should:
- Request animation frames when active
- Advance progress each frame based on speed constants
- Clean up state when animation completes
- Work independently of other container animations

## Future Considerations

- **Customizable timing**: Allow users to specify animation durations
- **Easing functions**: Support different easing curves for ripple expansion
- **Multiple ripples**: Support overlapping click ripples (currently only one at a time)
- **Touch support**: Adapt ripple behavior for touch interactions
- **Accessibility**: Ensure ripple doesn't interfere with reduced-motion preferences
