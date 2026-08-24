# Image Widget

The Image widget displays raster images (PNG, JPEG, GIF, WebP) and SVG vector graphics. Images are rendered as GPU textures and compose with container transforms (rotate, scale, translate).

## Quick Start

```rust
use guido::prelude::*;

// Load from file path (auto-detects SVG)
container()
    .width(32.0)
    .height(32.0)
    .child(image("./icon.png"))

// SVG from path
container()
    .width(100.0)
    .height(100.0)
    .child(image("./logo.svg"))

// SVG from memory
image(ImageSource::SvgBytes(svg_data.into()))
    .width(48.0)
    .height(48.0)
```

## Image Sources

Images can be loaded from four source types:

```rust
// Raster from file (PNG, JPEG, GIF, WebP)
ImageSource::Path(PathBuf::from("./image.png"))

// Raster from memory
ImageSource::Bytes(bytes.into())

// SVG from file
ImageSource::SvgPath(PathBuf::from("./image.svg"))

// SVG from memory
ImageSource::SvgBytes(svg_bytes.into())
```

File paths are auto-detected: `.svg` extension uses SVG rendering, all others use raster decoding.

## Content Fit Modes

Control how images fit within their bounds:

| Mode | Behavior |
|------|----------|
| `ContentFit::Contain` | Scale to fit within bounds, preserve aspect ratio (default) |
| `ContentFit::Cover` | Scale to cover bounds, may crop, preserve aspect ratio |
| `ContentFit::Fill` | Stretch to fill exactly, ignore aspect ratio |
| `ContentFit::None` | Use intrinsic size, ignore widget bounds |

```rust
container()
    .width(200.0)
    .height(150.0)
    .child(image("./photo.jpg"))
    .content_fit(ContentFit::Cover)
```

## Sizing

The box comes from the enclosing container, like every other size in guido; the
image decides only how its pixels land inside it.

```rust
// A 32x32 box
container().width(32.0).height(32.0).child(image("./icon.png"))

// Width fixed, height from the aspect ratio (the default fit is Contain)
container().width(200.0).child(image("./banner.png"))

// No box at all - the image reports its intrinsic size
image("./icon.png")
```

## Content Fit Modes

The `content_fit()` method controls how the image maps into the box it is
given:

| Mode | Description |
|------|-------------|
| `ContentFit::Contain` | Fit within the box, preserving aspect ratio (default) |
| `ContentFit::Cover` | Fill the box, cropping, preserving aspect ratio |
| `ContentFit::Fill` | Stretch to fill exactly, ignoring aspect ratio |
| `ContentFit::None` | Intrinsic size, ignoring the box |

`Cover` and `Fill` take all the room the container offers; they differ in how
the pixels land. `Contain` takes only the largest rect of the image's own
aspect ratio that fits, so the empty strip is left to the parent's alignment
rather than painted.

```rust
// Cover a 200x150 box, cropping what does not fit
container()
    .width(200.0)
    .height(150.0)
    .child(image("./photo.jpg").content_fit(ContentFit::Cover))

// A wallpaper: cover the whole surface
container()
    .width(fill())
    .height(fill())
    .child(image(wallpaper).content_fit(ContentFit::Cover))
```

## Transform Composition

Images inherit transforms from parent containers, following the same pattern as text:

```rust
// Rotated image
container()
    .transform(Transform::rotate_degrees(15.0))
    .child(container().width(32.0).height(32.0).child(image("./badge.svg")))

// Scaled image
container()
    .transform(Transform::scale(1.5))
    .child(container().width(24.0).height(24.0).child(image("./icon.png")))

// Combined transforms
container()
    .transform(Transform::rotate_degrees(45.0).then_scale(2.0).then_translate(10.0, 5.0))
    .child(image("./logo.svg"))
```

## SVG Quality

SVGs are rasterized at an effective scale that accounts for:
- Display scale factor (HiDPI)
- Transform scale (from parent containers)
- Quality multiplier (2.0x for crisp rendering)

This ensures SVGs remain crisp when scaled up via transforms.

## Texture Caching

The image texture renderer includes LRU caching:
- Raster images cached by source hash
- SVGs cached by source hash + render scale
- Maximum 64 cached textures
- Automatic eviction of least-recently-used entries

## Reactive Sources

Image sources can be reactive:

```rust
let icon = create_signal(ImageSource::Path("./play.png".into()));

// Reactive image
image(icon)

// Toggle icon on click
container()
    .on_click(move || {
        let new_icon = if is_playing.get() {
            ImageSource::Path("./pause.png".into())
        } else {
            ImageSource::Path("./play.png".into())
        };
        icon.set(new_icon);
    })
    .child(image(icon))
```

## Rendering Pipeline

Images are rendered after shapes but before text:

```
1. Background shapes (SDF pipeline)
2. Images (texture pipeline) ← Images rendered here
3. Text - direct (glyphon)
4. Text - transformed (texture pipeline)
5. Overlay shapes (SDF pipeline)
```

## Supported Formats

### Raster
- PNG
- JPEG
- GIF
- WebP

### Vector
- SVG (via resvg)

## Dependencies

Image support uses these crates:
- `image` - Raster image decoding
- `resvg` - SVG parsing and rasterization
- `tiny-skia` - Software rendering for SVG

These are automatically included as Guido dependencies.
