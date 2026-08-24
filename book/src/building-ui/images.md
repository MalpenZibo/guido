# Images

Guido supports displaying both raster images (PNG, JPEG, GIF, WebP) and SVG vector graphics. Images are rendered as GPU textures and compose seamlessly with container transforms.

## Basic Usage

The `image()` function creates an image widget from a file path:

```rust
use guido::prelude::*;

// Load a PNG image
container()
    .width(32.0)
    .height(32.0)
    .child(image("./icon.png"))

// Load an SVG (auto-detected by extension)
container()
    .width(100.0)
    .height(100.0)
    .child(image("./logo.svg"))
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

// Raw pre-decoded RGBA8 pixels (width * height * 4 bytes, row-major)
ImageSource::Rgba { width: 22, height: 22, pixels: rgba_bytes.into() }
```

When using a string path with `image()`, the file extension determines the type automatically: `.svg` files use SVG rendering, all others use raster decoding.

`ImageSource::Rgba` skips decoding entirely — use it for pixel data that never existed in an encoded format, such as tray icon pixmaps or album art received over D-Bus.

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

Images inherit transforms from parent containers, just like text:

```rust
// Rotated image
container()
    .transform(Transform::rotate_degrees(15.0))
    .child(
        container()
            .width(32.0)
            .height(32.0)
            .child(image("./badge.svg"))
    )

// Scaled image
container()
    .transform(Transform::scale(1.5))
    .child(
        container()
            .width(24.0)
            .height(24.0)
            .child(image("./icon.png"))
    )

// Combined transforms
container()
    .transform(Transform::rotate_degrees(45.0).then_scale(2.0))
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

container()
    .width(48.0)
    .height(48.0)
    .child(image(ImageSource::SvgBytes(svg_data.as_bytes().into())))
```

An `Image` carries no box of its own: the container it sits in is what gives it
one, exactly as with a `text`. `content_fit` then decides how the picture uses
that box.

## Reactive Images

Image sources can be reactive, allowing dynamic image changes:

```rust
let icon_source = create_signal(ImageSource::Path("./play.png".into()));

// The image updates when the signal changes
container()
    .width(32.0)
    .height(32.0)
    .child(image(icon_source))

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
    App::new().run(|app| {
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
                container()
                    .width(48.0)
                    .height(48.0)
                    .child(image("./logo.png"))
            )
            .child(
                // SVG from memory
                container()
                    .width(32.0)
                    .height(32.0)
                    .child(image(ImageSource::SvgBytes(svg_icon.as_bytes().into())))
            )
            .child(
                // Rotated image
                container()
                    .transform(Transform::rotate_degrees(15.0))
                    .child(
                        container()
                            .width(24.0)
                            .height(24.0)
                            .child(image("./icon.svg"))
                    )
            );

        app.add_surface(
            SurfaceConfig::new()
                .height(80)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            move || view,
        );
    });
}
```

## Performance Notes

- Images are cached as GPU textures
- The cache holds up to 64 textures with LRU eviction
- SVGs are re-rasterized when their display scale changes significantly
- Texture uploads happen once per unique image/scale combination
