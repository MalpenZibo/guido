//! Render-tree snapshots over widget trees lifted from `examples/`.
//!
//! The examples are the library's realistic widget trees, but they cannot be
//! imported: each is its own crate root, with the tree built inline in `main`.
//! So the trees are reproduced here — each scenario names the example it comes
//! from — and run through the real layout and paint passes, neither of which
//! needs a compositor or a GPU. The resulting render tree is dumped as text and
//! compared against a committed golden file.
//!
//! What this catches that the unit tests do not: a change in *geometry* or in
//! *what gets drawn*, anywhere in a realistic composition. A widget that moves
//! by a pixel, a clip that stops being emitted, a scrollbar that quietly
//! disappears — all of it shows up as a diff, without anyone having to think of
//! the specific assertion in advance.
//!
//! **No text.** Text metrics depend on the fonts installed on the machine, so a
//! snapshot containing them would pass locally and fail in CI for reasons that
//! have nothing to do with the change under review. Text is covered by the
//! container-level tests instead; the scenarios here stay on the geometry, which
//! is where the layout logic lives anyway.
//!
//! To re-bless after an intended change:
//!
//! ```sh
//! UPDATE_SNAPSHOTS=1 cargo test --test render_snapshots
//! ```
//!
//! Read the resulting diff before committing it — that diff *is* the review.

use std::fmt::Write as _;

use guido::layout::Constraints;
use guido::prelude::*;
use guido::renderer::{DrawCommand, PaintContext, RenderNode};
use guido::tree::Tree;
use guido::widgets::Widget;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Lay out `widget` in a `width` x `height` viewport, paint it, and dump the
/// resulting render tree.
fn render(widget: impl Widget + 'static, width: f32, height: f32) -> String {
    let mut tree = Tree::new();
    let root = tree.register(Box::new(widget));
    tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
    tree.with_widget_mut(root, |w, id, t| {
        w.layout(t, id, Constraints::new(0.0, 0.0, width, height))
    });

    let mut node = RenderNode::new(root.as_u64());
    tree.with_widget_mut(root, |w, id, t| {
        let mut ctx = PaintContext::new(&mut node);
        w.paint(t, id, &mut ctx);
    });

    let mut out = String::new();
    dump_node(&node, 0, &mut out);
    out
}

/// Two decimals is finer than a physical pixel at any sane scale, and coarse
/// enough that an arithmetic reassociation does not rewrite the golden file.
fn n(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r == 0.0 { "0".into() } else { format!("{r}") }
}

fn rect(r: &Rect) -> String {
    format!("{},{} {}x{}", n(r.x), n(r.y), n(r.width), n(r.height))
}

fn color(c: &Color) -> String {
    let (r, g, b, a) = c.to_rgba8();
    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
}

fn dump_node(node: &RenderNode, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    let t = node.local_transform;
    let transform = if t.is_identity() {
        String::new()
    } else if t.is_translation_only() {
        format!(" at {},{}", n(t.tx()), n(t.ty()))
    } else {
        // Full matrix: a rotation or a scale is exactly the kind of thing a
        // refactor can silently reassociate.
        format!(
            " matrix[{} {} {} {} {} {}]",
            n(t.a()),
            n(t.b()),
            n(t.c()),
            n(t.d()),
            n(t.tx()),
            n(t.ty())
        )
    };
    // The origin is resolved during flatten, not baked into local_transform,
    // so two nodes rotating about different pivots carry identical matrices.
    // Without this a regression in origin handling would not show. Only
    // reported where it can matter: a translation is the same about any pivot.
    let origin = if t.is_identity() || t.is_translation_only() {
        String::new()
    } else {
        let o = node.transform_origin;
        format!(" origin={:?}/{:?}", o.horizontal, o.vertical)
    };
    let clip = match &node.clip {
        Some(c) => format!(" clip({} r={})", rect(&c.rect), n(c.corner_radius)),
        None => String::new(),
    };
    let partial = if node.partial { " partial" } else { "" };

    let _ = writeln!(
        out,
        "{pad}node {}{transform}{origin}{clip}{partial}",
        rect(&node.bounds)
    );

    for cmd in &node.commands {
        dump_command(cmd, depth + 1, "draw", out);
    }
    for child in &node.children {
        dump_node(child, depth + 1, out);
    }
    for cmd in &node.overlay_commands {
        dump_command(cmd, depth + 1, "overlay", out);
    }
}

fn dump_command(cmd: &DrawCommand, depth: usize, kind: &str, out: &mut String) {
    let pad = "  ".repeat(depth);
    match cmd {
        DrawCommand::BackdropBlur {
            rect: r,
            radius,
            corner_radii,
            curvature,
        } => {
            out.push_str(&format!(
                "{pad}{kind} backdrop-blur {} radius={} corners={}/{}/{}/{} k={}\n",
                rect(r),
                n(*radius),
                n(corner_radii.top_left),
                n(corner_radii.top_right),
                n(corner_radii.bottom_right),
                n(corner_radii.bottom_left),
                n(*curvature),
            ));
        }
        DrawCommand::RoundedRect {
            rect: r,
            color: c,
            radius,
            curvature,
            border,
            shadow,
            gradient,
        } => {
            let mut line = format!(
                "{pad}{kind} rect {} fill={} radius={}/{}/{}/{}",
                rect(r),
                color(c),
                n(radius.top_left),
                n(radius.top_right),
                n(radius.bottom_right),
                n(radius.bottom_left)
            );
            if *curvature != 1.0 {
                let _ = write!(line, " curvature={}", n(*curvature));
            }
            if let Some(b) = border {
                let _ = write!(line, " border={}@{}", color(&b.color), n(b.width));
            }
            if let Some(s) = shadow {
                let _ = write!(
                    line,
                    " shadow={}+{},{} blur={}",
                    color(&s.color),
                    n(s.offset.0),
                    n(s.offset.1),
                    n(s.blur)
                );
            }
            if let Some(g) = gradient {
                let _ = write!(
                    line,
                    " gradient={}->{} {:?}",
                    color(&g.start_color),
                    color(&g.end_color),
                    g.direction
                );
            }
            let _ = writeln!(out, "{line}");
        }
        DrawCommand::Circle {
            center,
            radius,
            color: c,
        } => {
            let _ = writeln!(
                out,
                "{pad}{kind} circle at {},{} r={} fill={}",
                n(center.0),
                n(center.1),
                n(*radius),
                color(c)
            );
        }
        // Deliberately metric-free: see the module docs.
        DrawCommand::Text { .. } => {
            let _ = writeln!(out, "{pad}{kind} text <not snapshotted>");
        }
        DrawCommand::Image { rect: r, .. } => {
            let _ = writeln!(out, "{pad}{kind} image {}", rect(r));
        }
    }
}

/// Compare against the golden file, or rewrite it under `UPDATE_SNAPSHOTS=1`.
#[track_caller]
fn assert_snapshot(name: &str, actual: String) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("{name}.snap"));

    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &actual).expect("write snapshot");
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!("missing snapshot {name}. Create it with UPDATE_SNAPSHOTS=1 cargo test")
    });

    if expected != actual {
        panic!(
            "render tree changed for `{name}`.\n\
             If the change is intended, re-bless with:\n  \
             UPDATE_SNAPSHOTS=1 cargo test --test render_snapshots\n\n\
             --- expected ---\n{expected}\n--- actual ---\n{actual}"
        );
    }
}

/// A leaf of an exactly known size, so no scenario depends on font metrics.
fn box_of(w: f32, h: f32) -> Container {
    container().width(w).height(h)
}

fn swatch(w: f32, h: f32, c: Color) -> Container {
    box_of(w, h).background(c)
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

/// After `examples/scroll_example.rs`: a vertical scroller whose content
/// overflows (so it carries a scrollbar) beside one whose content fits (so it
/// does not), plus a horizontal scroller.
#[test]
fn scrolling() {
    let column = |n: usize| {
        container()
            .layout(Flex::column().spacing(8.0))
            .padding(8.0)
            .children((0..n).map(|i| {
                swatch(120.0, 24.0, Color::rgb(0.25, 0.25, 0.35))
                    .corner_radius(4.0)
                    .translate(0.0, i as f32)
            }))
    };

    let view = container()
        .layout(Flex::row().spacing(16.0))
        .padding(8.0)
        .child(
            box_of(200.0, 200.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .corner_radius(8.0)
                .scrollable(ScrollAxis::Vertical)
                .child(column(20)),
        )
        .child(
            box_of(200.0, 200.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .scrollable(ScrollAxis::Vertical)
                .child(column(2)),
        )
        .child(
            box_of(200.0, 80.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .scrollable(ScrollAxis::Horizontal)
                .child(
                    container()
                        .layout(Flex::row().spacing(8.0))
                        .padding(8.0)
                        .children((0..10).map(|_| swatch(80.0, 40.0, Color::BLUE))),
                ),
        );

    assert_snapshot("scrolling", render(view, 800.0, 300.0));
}

/// After `examples/flex_layout_test.rs`: the alignment matrix, plus `fill()`
/// sharing what the fixed children leave.
#[test]
fn flex_alignment_matrix() {
    let row = |main: MainAlignment, cross: CrossAlignment| {
        box_of(300.0, 60.0)
            .background(Color::rgb(0.12, 0.12, 0.16))
            .layout(
                Flex::row()
                    .spacing(6.0)
                    .main_alignment(main)
                    .cross_alignment(cross),
            )
            .child(swatch(40.0, 20.0, Color::RED))
            .child(swatch(40.0, 32.0, Color::GREEN))
            .child(swatch(40.0, 44.0, Color::BLUE))
    };

    let view = container()
        .layout(Flex::column().spacing(4.0))
        .child(row(MainAlignment::Start, CrossAlignment::Start))
        .child(row(MainAlignment::Center, CrossAlignment::Center))
        .child(row(MainAlignment::End, CrossAlignment::End))
        .child(row(MainAlignment::SpaceBetween, CrossAlignment::Stretch))
        .child(row(MainAlignment::SpaceAround, CrossAlignment::Center))
        .child(row(MainAlignment::SpaceEvenly, CrossAlignment::Center))
        .child(
            box_of(300.0, 40.0)
                .layout(Flex::row().spacing(6.0))
                .child(swatch(50.0, 20.0, Color::RED))
                .child(
                    container()
                        .width(fill())
                        .height(20.0)
                        .background(Color::GRAY),
                )
                .child(swatch(50.0, 20.0, Color::BLUE)),
        );

    assert_snapshot("flex_alignment_matrix", render(view, 400.0, 500.0));
}

/// After `examples/transform_example.rs` and `transform_origin_test.rs`:
/// rotation and scale about several origins, and a transform nested inside
/// another so the composition shows up.
#[test]
fn transforms_and_origins() {
    let card = |c: Color| swatch(80.0, 40.0, c).corner_radius(6.0);

    let view = container()
        .layout(Flex::column().spacing(12.0))
        .padding(20.0)
        .child(card(Color::RED).rotate(30.0))
        .child(
            card(Color::GREEN)
                .rotate(30.0)
                .transform_origin(TransformOrigin::TOP_LEFT),
        )
        .child(card(Color::BLUE).scale(1.5))
        .child(card(Color::YELLOW).scale_xy(2.0, 0.5))
        .child(card(Color::CYAN).translate(15.0, -5.0))
        .child(
            box_of(200.0, 80.0)
                .background(Color::rgb(0.2, 0.2, 0.25))
                .rotate(10.0)
                .child(card(Color::MAGENTA).rotate(20.0).scale(0.8)),
        );

    assert_snapshot("transforms_and_origins", render(view, 400.0, 600.0));
}

/// After `examples/showcase.rs` and `elevation_example.rs`: the corner
/// curvature family and the elevation ladder, both of which land entirely in
/// the emitted draw commands.
#[test]
fn corners_borders_and_elevation() {
    let base = |c: Color| swatch(70.0, 70.0, c).corner_radius(16.0);

    let view = container()
        .layout(Flex::column().spacing(10.0))
        .padding(10.0)
        .child(
            container()
                .layout(Flex::row().spacing(10.0))
                .child(base(Color::rgb(0.3, 0.5, 0.9)))
                .child(base(Color::rgb(0.3, 0.5, 0.9)).squircle())
                .child(base(Color::rgb(0.3, 0.5, 0.9)).bevel())
                .child(base(Color::rgb(0.3, 0.5, 0.9)).scoop())
                .child(base(Color::rgb(0.3, 0.5, 0.9)).corner_curvature(1.5)),
        )
        .child(
            container()
                .layout(Flex::row().spacing(10.0))
                .children((0..6).map(|level| {
                    swatch(60.0, 60.0, Color::rgb(0.9, 0.9, 0.95))
                        .corner_radius(8.0)
                        .elevation(level as f32)
                })),
        )
        .child(
            container()
                .layout(Flex::row().spacing(10.0))
                .child(base(Color::TRANSPARENT).border(2.0, Color::WHITE))
                .child(
                    box_of(70.0, 70.0)
                        .corner_radii(CornerRadii {
                            top_left: 16.0,
                            top_right: 0.0,
                            bottom_right: 16.0,
                            bottom_left: 0.0,
                        })
                        .background(Color::RED),
                )
                .child(
                    box_of(70.0, 70.0).gradient(LinearGradient::vertical(Color::RED, Color::BLUE)),
                ),
        );

    assert_snapshot("corners_borders_and_elevation", render(view, 500.0, 400.0));
}

/// After `examples/clip_test.rs`: hidden overflow clips its children, and a
/// clip nested inside another intersects.
#[test]
fn overflow_and_clipping() {
    let view = container()
        .layout(Flex::row().spacing(12.0))
        .padding(8.0)
        .child(
            box_of(100.0, 60.0)
                .overflow(Overflow::Hidden)
                .corner_radius(12.0)
                .background(Color::rgb(0.2, 0.2, 0.3))
                .child(swatch(300.0, 40.0, Color::RED)),
        )
        .child(
            box_of(120.0, 80.0)
                .overflow(Overflow::Hidden)
                .background(Color::rgb(0.2, 0.2, 0.3))
                .child(
                    box_of(200.0, 40.0)
                        .overflow(Overflow::Hidden)
                        .corner_radius(8.0)
                        .child(swatch(400.0, 30.0, Color::GREEN)),
                ),
        )
        .child(
            box_of(100.0, 60.0)
                .background(Color::rgb(0.2, 0.2, 0.3))
                .child(swatch(300.0, 40.0, Color::BLUE)),
        );

    assert_snapshot("overflow_and_clipping", render(view, 500.0, 200.0));
}

/// After `examples/children_example.rs`: a keyed list and a reactive child,
/// which is what the reconciler produces once it has settled.
#[test]
fn dynamic_children() {
    let items = create_signal(vec![(1u64, 30.0f32), (2, 50.0), (3, 20.0)]);
    let show_footer = create_signal(true);

    let view = container()
        .layout(Flex::column().spacing(6.0))
        .padding(8.0)
        .children(keyed(
            move || items.get(),
            |(id, _)| *id,
            |(_, height)| swatch(120.0, height, Color::rgb(0.3, 0.3, 0.4)).corner_radius(4.0),
        ))
        .child(move || {
            show_footer
                .get()
                .then(|| swatch(120.0, 16.0, Color::rgb(0.5, 0.2, 0.2)))
        });

    assert_snapshot("dynamic_children", render(view, 300.0, 400.0));
}

/// After `examples/nested_transform_example.rs`, and closest to a real bar: a
/// row of modules, one of them scrolling, inside a rounded background.
#[test]
fn bar_like_composition() {
    let module = |w: f32, c: Color| {
        container()
            .padding([4.0, 8.0])
            .background(c)
            .corner_radius(10.0)
            .hover_state(|s| s.lighter(0.1))
            .child(swatch(w, 14.0, Color::WHITE.with_alpha(0.8)))
    };

    let view = container()
        .width(fill())
        .height(36.0)
        .background(Color::rgb(0.1, 0.1, 0.14))
        .padding([0.0, 8.0])
        .layout(
            Flex::row()
                .spacing(8.0)
                .cross_alignment(CrossAlignment::Center),
        )
        .child(module(40.0, Color::rgb(0.2, 0.2, 0.3)))
        .child(module(70.0, Color::rgb(0.2, 0.3, 0.2)))
        .child(container().width(fill()).height(1.0))
        .child(
            box_of(90.0, 24.0)
                .scrollable(ScrollAxis::Horizontal)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .corner_radius(6.0)
                .child(
                    container()
                        .layout(Flex::row().spacing(4.0))
                        .children((0..6).map(|_| swatch(30.0, 20.0, Color::CYAN))),
                ),
        )
        .child(module(50.0, Color::rgb(0.3, 0.2, 0.2)).elevation(2.0));

    assert_snapshot("bar_like_composition", render(view, 600.0, 36.0));
}

/// After `examples/text_input_example.rs`: a text input narrower than its own
/// content. The field is a viewport — the glyphs that do not fit are cut at
/// its horizontal edges, and only there, so descenders and any glyph
/// decoration survive.
///
/// This is the case that went unwatched: clipping was dropped from the input
/// when the V2 renderer landed, and no snapshot covered the widget, so the
/// text simply drew across whatever sat beside it.
#[test]
fn text_input_clips_overflowing_content() {
    let value = create_signal("gggggggggggggggggggggggggggggggggggggg".to_string());

    let view = container().padding(8.0).child(
        container()
            .width(180.0)
            .padding(8.0)
            .background(Color::rgb(0.18, 0.18, 0.24))
            .corner_radius(6.0)
            .text_color(Color::WHITE)
            .font_size(14.0)
            .child(text_input(value)),
    );

    assert_snapshot(
        "text_input_clips_overflowing_content",
        render(view, 400.0, 60.0),
    );
}
