# Image Widget

The Image widget displays raster images (PNG, JPEG, GIF, WebP) and SVG vector graphics. Images are rendered as GPU textures and compose with container transforms (rotate, scale, translate). An image up to 256 texels a side shares an atlas page with other small ones, so a screen of icons is a few draw calls rather than one per icon — see `docs/RENDERER.md`, "Textured Quads".

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

This ensures SVGs remain crisp when scaled up via transforms. The scale is
quantized to quarters and the box to whole pixels before the raster's pixel
size is worked out (`svg_extent` in `src/renderer/image_quad.rs`), so a scale
animation asks for a raster per step rather than per frame.

## Decoding Off the Frame

A raster `ImageSource::Path` or `ImageSource::Bytes` is decoded on a worker
thread, and a large SVG rasterized there, never on the render path
(`src/image_decode.rs`). A 2912×1632 PNG takes about 140 ms to decode; done
inside the first frame, that frame is held back until it finishes, which a lock
screen shows as the compositor's "locker has not drawn" colour. An SVG
rasterized at 1024 px costs 4–8 ms the same way.

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
  An SVG has no header: its entry parses the document once
  (`image_metadata::parse_svg`) for the size, and keeps it, so every raster
  of it is drawn from that one parse.
- **An SVG is rasterized where the renderer decides.** What an SVG becomes
  depends on the size it is drawn at, which only the renderer knows — it
  carries the transform, the HiDPI factor and the box. So an SVG's
  `DecodedImage` holds a slot per pixel size, and a frame that needs a size it
  has no texture of calls `DecodedImage::take_or_rasterize`. Up to
  `INLINE_RASTER_BYTES` of pixels — 128 × 128, a 64 px icon at the 2× quality
  multiplier — the raster is drawn there, inside the frame. That is what
  Chromium (below 1 MB, on its raster threads), iced, Qt's QIcon and
  Android's VectorDrawable do with small vectors: a 6 KB icon costs
  140–220 µs at 56 px, less than drawing it a frame late and repainting it
  when it lands. The line is lower than Chromium's because this renderer runs
  on the loop's thread, where a 1 MB raster costs 1.1–2.7 ms.
- **A larger raster is asked of the worker.** The frame sends it a job — a
  job on a channel, not a signal write, so it is on its way before the frame
  is out. Until it lands the frame draws the raster that source was last drawn
  from, stretched to the new box: a large SVG in a resize animation does not
  blink out on every step. Elsewhere that is an opt-in (Flutter's
  gaplessPlayback, Qt Quick's retainWhileLoading); here #547 decided it.
  The first raster has nothing before it, so it draws nothing, as a raster
  source does. The paint pushes an SVG's command while it is still `Pending`
  for that reason — the push is what carries the size to the renderer — where a
  raster source is pushed only once it is `Ready` (`image_decode::paints`).
- **An SVG's readiness is the renderer's report.** Only the renderer knows
  whether a frame drew it, and it may not write a signal, so it reports: a
  raster drawn, or nothing drawn while one is on the worker. The loop settles
  those into `Ready` and `Pending`. An entry starts `Ready` when its raster at
  its intrinsic size would be drawn inline, `Pending` when not — a guess, which
  the first report corrects.
- **The worker.** One `guido-image-decode` thread per application, spawned by
  the first decode and ended when the application is dropped (its channel
  closes). It puts the pixels in the entry's `DecodedImage` and writes `Ready`
  through the entry's `WriteSignal` — always, so a large SVG already ready is
  told of each new size that lands — so the
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
  is not there (evicted from the texture cache, or its renderer dropped) and
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
  from a widget written outside the crate is held by nobody, so nothing
  releases it: it goes with its texture's eviction, and if it is decoded but
  never drawn it keeps its pixels as long as the application.
- **Eviction spares what is in view.** The texture cache is bounded by bytes,
  not by count: an image with a texture of its own costs width × height × 4
  (image textures have no mip levels), one packed onto an atlas page costs
  its place there — (width + 2) × (height + 2) × 4, gutter included — and the
  budget is 100 MB (`DEFAULT_IMAGE_CACHE_BUDGET`, Flutter's
  `ImageCache` default) unless the application sets its own with
  `App::image_cache_budget` or `set_image_cache_budget`. Under the budget
  nothing is evicted, whatever the count. Over it, `ImageQuadRenderer::trim`
  evicts the least recently drawn first — once the frame has drawn, so what
  it drew is stamped as drawn now — and never one a frame drew within the
  last second: a raster texture is the only copy of its pixels, so evicting an
  image still in view would blank it and send it back to the worker every
  frame. When those alone exceed the budget the cache grows instead. The
  cache is the renderer's, shared by every surface: a surface that has not
  drawn for a second is not protected from another surface's trim. The pages
  themselves are not counted: a page is 1024 × 1024 × 4 bytes, 4 MB, whether
  it holds one entry or four hundred, and goes only once eviction has emptied
  it — so the budget bounds what the entries occupy, not the GPU memory the
  pages reserve.
- **Ready signal.** `Image::ready()` is a `Signal<bool>`: true once the source
  is decoded or a raster of it drawn — from the start for `Rgba` and for an
  SVG small enough to draw inline — and never for a failed one.
  There is no built-in fade — Flutter's frameBuilder shape rather than a
  fade inside the widget — so an application fades in with `opacity`:

```rust
let wallpaper = image("./wallpaper.png").content_fit(ContentFit::Cover);
let ready = wallpaper.ready();
container()
    .opacity((move || if ready.get() { 1.0 } else { 0.0 }).transition(200.0))
    .child(wallpaper)
```

`ImageSource::Rgba` needs no decode and is the way to have an image in the
first frame.

`tests/image_decode.rs` holds the worker (`Headless::hold_image_decodes`) to
make "not yet decoded" deterministic, and is a binary of its own because the
background-write queue is process-wide.

## Tint

`Image::tint` draws every texel in one colour and keeps its alpha:
`rgb = tint.rgb`, `a = sampled.a * tint.a * opacity` — written to the
framebuffer premultiplied, as everything the textured-quad pipeline draws is
(see Alpha below). It travels on the
textured-quad vertex (`TexturedVertex::tint`, `[r, g, b, amount]`) beside the
opacity and for the same reason — a new colour is not a new texture — so it is
neither in the texture cache's key nor a cause of a new raster. Paint reads it,
so a new tint repaints and lays nothing out. Transformed text shares the
pipeline and passes `NO_TINT`, an amount of zero, which leaves the texel as it
was.

## Alpha

The textured-quad pipeline — images and transformed text — composites
premultiplied alpha (`One / OneMinusSrcAlpha`), as Skia stores its textures, so
every texture it samples holds colour already scaled by coverage. glyphon fills a
text quad's texture that way and tiny-skia an SVG's raster; a decoded image is
premultiplied on the worker and an `ImageSource::Rgba` when it is uploaded (a
copy only when a texel is not opaque). Bilinear filtering then mixes colour
weighted by coverage, so a scaled image's edge does not pick up the black of the
empty texels beside it.

## Texture Caching

The image texture renderer includes LRU caching:
- A source that decodes cached by its decode entry, which has already
  settled whether two sources are one; an SVG by its entry and the raster
  size it was drawn at
- `ImageSource::Rgba` cached by its pixels, sampled for the hash and compared
  in full for equality, and dropped once nothing but the cache holds them
- A byte budget, 100 MB by default, growing past it only while what was
  drawn in the last second exceeds it
- Past the budget, eviction of least-recently-used entries

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
