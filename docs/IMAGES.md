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
    .rotate(15.0)
    .child(container().width(32.0).height(32.0).child(image("./badge.svg")))

// Scaled image
container()
    .scale(1.5)
    .child(container().width(24.0).height(24.0).child(image("./icon.png")))

// Combined transforms. The three apply in one fixed order — translate, then
// rotate, then scale — so the offset below is in the parent's frame, not the
// turned and scaled one. Composing the other way round is a nested container;
// see docs/TRANSFORMS.md.
container()
    .translate((10.0, 5.0))
    .rotate(45.0)
    .scale(2.0)
    .child(image("./logo.svg"))
```

## SVG Quality

SVGs are rasterized at an effective scale that accounts for:
- Display scale factor (HiDPI)
- Transform scale (from parent containers)
- Quality multiplier (2.0x for crisp rendering)

This ensures SVGs remain crisp when scaled up via transforms.

## Decoding Off the Frame

A raster `ImageSource::Path` or `ImageSource::Bytes` is decoded on a worker
thread, never on the render path (`src/image_decode.rs`). A 2912×1632 PNG takes
about 140 ms to decode; done inside the first frame, that frame is held back
until it finishes, which a lock screen shows as the compositor's "locker has
not drawn" colour.

- **One entry per source, one signal per entry.** The application's decode
  cache (`AppState::decoded_images`) is keyed by the source — a path, or the
  bytes, sampled for the hash and compared in full for equality — and each
  entry holds an `RwSignal<DecodeState>`: `Pending`, `Ready` or `Failed`. The
  pixels are not in the signal: they wait in the entry's `DecodedImage`, a
  handle the entry, the worker's job and every draw command share.
- **The header, synchronously.** Making an entry reads the intrinsic size from
  the header (`image_metadata::get_intrinsic_size`, which for bytes too reads
  only the header), so the first layout gives the box its size. A header that
  cannot be read makes the entry `Failed` at once, and no worker is involved.
- **The worker.** One `guido-image-decode` thread per application, spawned by
  the first decode and ended when the application is dropped (its channel
  closes). It puts the pixels in the entry's `DecodedImage` and writes `Ready`
  through the entry's `WriteSignal`, so the
  write rides the background-write queue: it wakes the loop through the ingress
  channel and is applied at the flush point like any other background write.
  A std thread rather than the service runtime, because that runtime is one
  current-thread tokio runtime driving every service: a 140 ms decode on it
  would stall them all.
- **Paint subscribes.** The `Image` widget reads its held entry's signal while
  painting (a widget written outside the crate gets the same through
  `PaintContext::draw_image`, which looks the entry up by source). A pending or
  failed source draws nothing; a ready one pushes a
  `DrawCommand::Image` carrying the `DecodedImage`. The read happens inside the widget's paint scope, so the write
  that completes the decode repaints exactly the widgets that read the entry —
  on the next frame, with no input from the application.
- **The upload drops the pixels.** The renderer gets the pixels only by
  `DecodedImage::take`, which empties the handle: once uploaded, the texture is
  the image and the cache holds no RGBA — iced's raster cache turning
  `Memory::Host` into `Memory::Device`. The renderer is one per application
  and shared by every surface, so one upload serves them all. No surface can
  draw from dropped pixels, because the only way to them is to take them.
- **A texture that is gone.** When a frame draws a ready source whose texture
  is not there (evicted from the 64-entry cache, or its renderer dropped) and
  whose pixels were already taken, the renderer draws nothing and reports it.
  The source goes back to `Pending` and to the worker, and comes back through
  the same repaint as the first time. `ready()` is false in between.
- **Reports are settled by the loop.** The renderer and the widgets never write
  the cache's signals: a texture missing or evicted, and an image letting go of
  its entry, are `ImageEvent`s in a deferred queue (`AppState::image_events`)
  that the loop settles once per pass, next to the owner disposals.
- **Lifetime.** An `Image` widget holds its source's entry (`DecodeHandle`)
  while it shows it. An entry nobody holds stays while its texture does — so an
  image mounted again is drawn from it with no decode — and goes when the
  renderer evicts that texture, or at once if it never had one; pixels that
  land after that are dropped on arrival. An entry made by a bare `draw_image`
  from a widget written outside the crate is held by nobody and follows the
  same rule.
- **Ready signal.** `Image::ready()` is a `Signal<bool>`: true once the source
  is decoded, true from the start for `Rgba` and SVG, never for a failed one.
  There is no built-in fade — Flutter's frameBuilder shape rather than a
  fade inside the widget — so an application fades in with `opacity`:

```rust
let wallpaper = image("./wallpaper.png").content_fit(ContentFit::Cover);
let ready = wallpaper.ready();
container()
    .opacity((move || if ready.get() { 1.0 } else { 0.0 }).transition(200.0))
    .child(wallpaper)
```

SVG rasterisation stays synchronous, on the render path, until somebody
measures it as slow. `ImageSource::Rgba` needs no decode and is the way to have
an image in the first frame.

`tests/image_decode.rs` holds the worker (`Headless::hold_image_decodes`) to
make "not yet decoded" deterministic, and is a binary of its own because the
background-write queue is process-wide.

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
