# Images

Guido supports displaying both raster images (PNG, JPEG, GIF, WebP) and SVG vector graphics. Images are rendered as GPU textures and compose seamlessly with container transforms.

## Basic Usage

The `image()` function creates an image widget from a file path:

```rust
use guido::prelude::*;

// Load a PNG image
image("./icon.png")
    .width(32.0)
    .height(32.0)

// Load an SVG (auto-detected by extension)
image("./logo.svg")
    .width(100.0)
    .height(100.0)
```

## Image Sources

You can load images from different sources using `ImageSource`:

```rust
// From file path (raster)
ImageSource::Path("./photo.jpg".into())

// From memory (raster)
ImageSource::Bytes(image_bytes.into())

// From file path (SVG)
ImageSource::SvgPath("./icon.svg".into())

// From memory (SVG)
ImageSource::SvgBytes(svg_string.as_bytes().into())
```

When using a string path with `image()`, the file extension determines the type automatically: `.svg` files use SVG rendering, all others use raster decoding.

## Sizing

You can specify explicit dimensions or let the image use its intrinsic size:

```rust
// Explicit width and height
image("./icon.png")
    .width(32.0)
    .height(32.0)

// Only width - height calculated from aspect ratio
image("./banner.png")
    .width(200.0)

// Only height - width calculated from aspect ratio
image("./logo.svg")
    .height(48.0)

// No size - uses intrinsic dimensions
image("./icon.png")
```

## Content Fit Modes

The `content_fit()` method controls how images fit within their bounds:

| Mode | Description |
|------|-------------|
| `ContentFit::Contain` | Fit within bounds, preserving aspect ratio (default) |
| `ContentFit::Cover` | Cover the bounds, may crop, preserving aspect ratio |
| `ContentFit::Fill` | Stretch to fill exactly, ignoring aspect ratio |
| `ContentFit::None` | Use intrinsic size, ignoring widget bounds |

```rust
// Cover mode - fills the space, may crop
image("./photo.jpg")
    .width(200.0)
    .height(150.0)
    .content_fit(ContentFit::Cover)
```

## Transform Composition

Images inherit transforms from parent containers, just like text:

```rust
// Rotated image
container()
    .rotate(15.0)
    .child(
        image("./badge.svg")
            .width(32.0)
            .height(32.0)
    )

// Scaled image
container()
    .scale(1.5)
    .child(
        image("./icon.png")
            .width(24.0)
            .height(24.0)
    )

// Combined transforms
container()
    .rotate(45.0)
    .scale(2.0)
    .child(image("./logo.svg"))
```

## SVG Quality

SVGs are automatically rasterized at the appropriate scale for crisp rendering:

- HiDPI displays: SVGs render at the display scale factor
- Transforms: When scaled via container transforms, SVGs re-rasterize at the higher resolution
- Quality: A 2x supersampling multiplier ensures smooth edges

This means SVGs stay crisp regardless of how they're scaled or transformed.

## In-Memory SVGs

For dynamically generated or embedded SVGs:

```rust
let svg_data = r##"
    <svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
        <circle cx="50" cy="50" r="40" fill="#4f46e5" />
    </svg>
"##;

image(ImageSource::SvgBytes(svg_data.as_bytes().into()))
    .width(48.0)
    .height(48.0)
```

## Reactive Images

Image sources can be reactive, allowing dynamic image changes:

```rust
let icon_source = create_signal(ImageSource::Path("./play.png".into()));

// The image updates when the signal changes
image(icon_source)
    .width(32.0)
    .height(32.0)

// Change the image on click
container()
    .on_click(move || {
        icon_source.set(ImageSource::Path("./pause.png".into()));
    })
    .child(image(icon_source))
```

## Supported Formats

### Raster Formats
- PNG
- JPEG
- GIF
- WebP

### Vector Formats
- SVG

## Example

Here's a complete example showing various image features:

```rust
use guido::prelude::*;

fn main() {
    let svg_icon = r##"
        <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
            <circle cx="12" cy="12" r="10" fill="#3b82f6"/>
        </svg>
    "##;

    let view = container()
        .padding(16.0)
        .layout(Flex::row().spacing(16.0))
        .child(
            // PNG image
            image("./logo.png")
                .width(48.0)
                .height(48.0)
        )
        .child(
            // SVG from memory
            image(ImageSource::SvgBytes(svg_icon.as_bytes().into()))
                .width(32.0)
                .height(32.0)
        )
        .child(
            // Rotated image
            container()
                .rotate(15.0)
                .child(
                    image("./icon.svg")
                        .width(24.0)
                        .height(24.0)
                )
        );

    App::new()
        .height(80)
        .run(view);
}
```

## Performance Notes

- Images are cached as GPU textures
- The cache holds up to 64 textures with LRU eviction
- SVGs are re-rasterized when their display scale changes significantly
- Texture uploads happen once per unique image/scale combination
