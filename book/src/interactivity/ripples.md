# Ripple Effects

Ripples provide Material Design-style touch feedback. They expand from the click point and create a visual acknowledgment of user interaction.

## Basic Ripple

Add a default ripple to the pressed state:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .corner_radius(8.0)
    .when_pressed(|s| s.ripple())
```

The default ripple uses a semi-transparent white overlay.

## Colored Ripple

Customize the ripple color:

```rust
.when_pressed(|s| s.ripple_with_color(Color::rgba(1.0, 0.8, 0.0, 0.4)))
```

Good ripple colors have transparency (alpha 0.2-0.5):

```rust
// Subtle white
Color::rgba(1.0, 1.0, 1.0, 0.2)

// Yellow accent
Color::rgba(1.0, 0.8, 0.0, 0.4)

// Blue accent
Color::rgba(0.3, 0.5, 1.0, 0.3)
```

## Ripple with Other Effects

Combine ripples with other pressed state changes:

```rust
.when_pressed(|s| s
    .ripple()
    .darker(0.05)
    .transform(Transform::scale(0.98))
)
```

## How Ripples Work

1. **Press** - a disc appears at the click point, already at about a third of
   its final size, and starts spreading
2. **Release inside** - the press happened, so the remaining expansion is
   *completed* rather than interrupted, while the disc fades out
3. **Release elsewhere, or drag off the button** - nothing was activated, and
   the ripple says the same thing `on_click` does: the disc just fades, quickly

Two properties are worth stating outright, because they are what makes the
effect read as an acknowledgement rather than a hesitation:

- **The radius never goes backwards.** The exit is a fade, never a contraction.
- **A short press does not truncate the expansion.** A click lasts 60-150ms
  against a growth measured in hundreds, so the release finishes the growth
  instead of abandoning it half-way.
- **No press is dimmer for being quicker.** The disc always finishes appearing
  before it starts to leave, so a 20ms tap is as bright as a held one.

The disc's centre also drifts toward the container's own centre as it grows, so
a press near a corner settles onto the button instead of staying lopsided. Its
final size comes from the container, not from where you pressed — every ripple
on a given button is the same size, and a full one covers it exactly.

The ripple:
- Respects corner radius and container shape
- Works correctly with transformed containers (rotated, scaled)
- Renders in the overlay layer (on top of content)

## Several at once

Each press is its own ripple, and they overlap:

```rust
container()
    .when_pressed(|s| s.ripple())
    .on_click(increment)
```

Clicking this repeatedly layers one disc over the next, because two clicks are
two events — the second does not erase the first. Up to four are alive at a
time; past that the oldest is dropped, which is the one furthest through its
own fade.

## Timing

| Phase | Duration | Scaled by |
|---|---|---|
| Opacity rise on contact | 75ms | `fade_speed` |
| Growth while held | 1s | `expand_speed` |
| Growth remaining after release | 225ms | `expand_speed` |
| Fade after a release | 375ms | `fade_speed` |
| Fade after an abandoned press | 75ms | `fade_speed` |

The fade never starts before the rise has finished, and the growth after a
release is never given longer than the fade it overlaps — so no combination of
the two speeds can put the disc back where it started, half-grown and already
invisible.

Both speeds are clamped to a sane range, so a zero or a negative one degrades
instead of stalling the frame loop:

```rust
.when_pressed(|s| s.ripple_config(RippleConfig {
    color: Color::rgba(1.0, 1.0, 1.0, 0.3),
    expand_speed: 1.5,   // faster growth
    fade_speed: 1.0,
}))
```

## Ripples on Transformed Containers

Ripples work correctly even with transforms:

```rust
container()
    .padding(16.0)
    .background(Color::rgb(0.4, 0.6, 0.4))
    .corner_radius(8.0)
    .transform(Transform::rotate_degrees(5.0).then(&Transform::translate(10.0, 15.0)))
    .when_hovered(|s| s.lighter(0.1))
    .when_pressed(|s| s.ripple())
```

Click coordinates are properly transformed to local container space.

## Ripples with Corner Curvature

Ripples respect different corner styles:

```rust
// Squircle ripple
container()
    .corner_radius(12.0)
    .squircle()
    .when_pressed(|s| s.ripple())

// Beveled ripple
container()
    .corner_radius(12.0)
    .bevel()
    .when_pressed(|s| s.ripple())
```

## Complete Example

```rust
fn ripple_button(label: &str, color: Color) -> Container {
    container()
        .padding(16.0)
        .background(color)
        .corner_radius(8.0)

        // Subtle hover, ripple on press
        .when_hovered(|s| s.lighter(0.1))
        .when_pressed(|s| s.ripple().transform(Transform::scale(0.98)))

        .on_click(|| println!("Clicked!"))
        .child(container().child(text(label).color(Color::WHITE)))
}

// Usage
ripple_button("Default Ripple", Color::rgb(0.3, 0.5, 0.8))
ripple_button("Action Button", Color::rgb(0.8, 0.3, 0.3))
```

## Ripple Color Guidelines

| Background | Ripple Color |
|------------|--------------|
| Dark | `Color::rgba(1.0, 1.0, 1.0, 0.2-0.3)` |
| Light | `Color::rgba(0.0, 0.0, 0.0, 0.1-0.2)` |
| Colored | Lighter tint with 0.3-0.4 alpha |

## API Reference

```rust
// Default semi-transparent white ripple
.when_pressed(|s| s.ripple())

// Custom colored ripple
.when_pressed(|s| s.ripple_with_color(Color::rgba(r, g, b, a)))
```
