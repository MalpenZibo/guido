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
//! A scenario with no snapshot gets one with:
//!
//! ```sh
//! UPDATE_SNAPSHOTS=1 cargo test --test render_snapshots
//! ```
//!
//! It refuses to overwrite one that already exists — the rule both harnesses
//! read is `common::blessing`. Rewriting after an intended change is
//! `REBLESS_SNAPSHOTS=1`, which a hook refuses and which a pull request carries
//! the `golden-update` label to authorise. Read the diff before rewriting it —
//! that diff *is* the review.

use std::fmt::Write as _;

mod common;

use guido::layout::Constraints;
use guido::prelude::*;
use guido::renderer::{BlurShape, DrawCommand, FlattenScratch, RenderNode, flatten_root_into};
use guido::tree::Tree;
use guido::widgets::Widget;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Lay out `widget` in a `width` x `height` viewport, paint it, and dump the
/// resulting render tree.
fn render(widget: impl Widget + 'static, width: f32, height: f32) -> String {
    let mut out = String::new();
    dump_node(&painted(widget, width, height), 0, &mut out);
    out
}

/// The render tree `widget` paints in a `width` x `height` viewport.
fn painted(widget: impl Widget + 'static, width: f32, height: f32) -> RenderNode {
    let mut tree = Tree::new();
    let root = tree.register(Box::new(widget));
    tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
    tree.layout_widget(root, Constraints::new(0.0, 0.0, width, height));

    let mut node = RenderNode::new(root.as_u64());
    tree.paint_widget(root, &mut node, 1.0);
    node
}

/// The same, and then what flatten made of it: every command in the order it
/// is drawn, with where it was placed. What only exists after flatten — a
/// merged shape — is read here.
fn render_flattened(widget: impl Widget + 'static, width: f32, height: f32) -> String {
    let node = painted(widget, width, height);
    let mut out = String::new();
    dump_node(&node, 0, &mut out);

    let (mut commands, mut layers) = (Vec::new(), Vec::new());
    let _ = flatten_root_into(
        &node,
        1.0,
        &mut commands,
        &mut layers,
        &mut FlattenScratch::default(),
    );
    let _ = writeln!(out, "flattened");
    for layer in &layers {
        for range in [
            &layer.backdrop,
            &layer.shapes,
            &layer.images,
            &layer.text,
            &layer.overlay,
        ] {
            for cmd in &commands[range.clone()] {
                let t = cmd.world_transform;
                let _ = writeln!(out, "  at {},{}", n(t.tx()), n(t.ty()));
                dump_command(&cmd.command, 2, "draw", &mut out);
            }
        }
    }
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
        let o = node.pivot;
        format!(" origin={:?}/{:?}", o.horizontal, o.vertical)
    };
    // One number while the four agree, so a uniform clip reads as it always
    // did and a per-corner one is visible as four.
    let clip_of = |label: &str, clip: Option<(&Rect, &CornerRadii)>| match clip {
        Some((at, r)) if *r == CornerRadii::uniform(r.top_left) => {
            format!(" {label}({} r={})", rect(at), n(r.top_left))
        }
        Some((at, r)) => format!(
            " {label}({} r={}/{}/{}/{})",
            rect(at),
            n(r.top_left),
            n(r.top_right),
            n(r.bottom_right),
            n(r.bottom_left)
        ),
        None => String::new(),
    };
    // The node's own clip, then the one only its children are cut to.
    let clip = clip_of(
        "clip",
        node.clip.as_ref().map(|c| (&c.rect, &c.corner_radius)),
    ) + &clip_of(
        "children_clip",
        node.children_clip
            .as_ref()
            .map(|c| (&c.rect, &c.corner_radius)),
    );
    let partial = if node.partial { " partial" } else { "" };
    let merge = match &node.merge {
        Some(merge) => format!(" merge(distance={})", n(merge.distance)),
        None => String::new(),
    };

    let _ = writeln!(
        out,
        "{pad}node {}{transform}{origin}{clip}{merge}{partial}",
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
        // Written by flatten in place of a subtree it composites; no widget
        // paints one, so a render tree never holds it.
        DrawCommand::Layer { .. } => unreachable!("a layer's composite in a render tree"),
        // Merged by flatten, out of the blurs its members painted; a render
        // tree holds only those.
        DrawCommand::BackdropBlur {
            shape: BlurShape::Merged(_),
            ..
        } => unreachable!("a merged blur in a render tree"),
        DrawCommand::BackdropBlur {
            rect: r,
            sources,
            radius,
            shape:
                BlurShape::Rounded {
                    corner_radii,
                    curvature,
                },
        } => {
            out.push_str(&format!(
                "{pad}{kind} backdrop-blur {} sources={:?} radius={} corners={}/{}/{}/{} k={}\n",
                rect(r),
                sources,
                n(*radius),
                n(corner_radii.top_left),
                n(corner_radii.top_right),
                n(corner_radii.bottom_right),
                n(corner_radii.bottom_left),
                n(*curvature),
            ));
        }
        DrawCommand::InputRegion {
            rect: r,
            corner_radii,
            curvature,
            takes,
        } => {
            out.push_str(&format!(
                "{pad}{kind} input-region {} takes={} corners={}/{}/{}/{} k={}\n",
                rect(r),
                takes,
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
            ..
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
                if s.inset {
                    line.push_str(" inset");
                }
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
        DrawCommand::MergedShape {
            members,
            distance,
            border,
            shadows,
            bounds,
        } => {
            let mut line = format!(
                "{pad}{kind} merged {} distance={}",
                rect(bounds),
                n(*distance)
            );
            if let Some(b) = border {
                let _ = write!(line, " border={}@{}", color(&b.color), n(b.width));
            }
            for s in shadows.as_slice() {
                let _ = write!(
                    line,
                    " shadow={}+{},{} blur={}",
                    color(&s.color),
                    n(s.offset.0),
                    n(s.offset.1),
                    n(s.blur)
                );
            }
            let _ = writeln!(out, "{line}");
            for m in members {
                let t = m.placement;
                let mut line = format!(
                    "{pad}  member {} at {},{} fill={} radius={}",
                    rect(&m.rect),
                    n(t.tx()),
                    n(t.ty()),
                    color(&m.color),
                    n(m.radius.top_left),
                );
                if let Some(b) = m.border {
                    let _ = write!(line, " border={}@{}", color(&b.color), n(b.width));
                }
                if m.opacity != 1.0 {
                    let _ = write!(line, " opacity={}", n(m.opacity));
                }
                let _ = writeln!(out, "{line}");
            }
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
        DrawCommand::Arc {
            center,
            radius,
            start,
            sweep,
            width,
            color: c,
            cap,
        } => {
            let _ = writeln!(
                out,
                "{pad}{kind} arc at {},{} r={} from={} sweep={} width={} {:?} stroke={}",
                n(center.x),
                n(center.y),
                n(*radius),
                n(*start),
                n(*sweep),
                n(*width),
                cap,
                color(c)
            );
        }
        DrawCommand::Path { path, fill, stroke } => {
            let mut line = format!(
                "{pad}{kind} path {} segments in {}",
                path.elements().len(),
                rect(&path.bounds())
            );
            if let Some(f) = fill {
                let _ = write!(line, " fill={} {:?}", color(&f.color), f.rule);
            }
            if let Some(s) = stroke {
                let _ = write!(
                    line,
                    " stroke={}@{} {:?} {:?}",
                    color(&s.color),
                    n(s.width),
                    s.cap,
                    s.join
                );
            }
            let _ = writeln!(out, "{line}");
        }
        // Deliberately metric-free: see the module docs.
        DrawCommand::Text { .. } => {
            let _ = writeln!(out, "{pad}{kind} text <not snapshotted>");
        }
        DrawCommand::TextBackdropBlur { radius, .. } => {
            let _ = writeln!(out, "{pad}{kind} text backdrop blur r={}", n(*radius));
        }
        DrawCommand::Image { rect: r, tint, .. } => {
            let _ = write!(out, "{pad}{kind} image {}", rect(r));
            if let Some(t) = tint {
                let _ = write!(out, " tint={}", color(t));
            }
            let _ = writeln!(out);
        }
    }
}

/// Compare against the golden file, or rewrite it under `UPDATE_SNAPSHOTS=1`.
#[track_caller]
fn assert_snapshot(name: &str, actual: String) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("{name}.snap"));

    let blessing = common::blessing_from_env("SNAPSHOTS", path.exists());
    if common::write_if_blessed(&path, blessing, || {
        std::fs::write(&path, &actual).expect("write snapshot");
    }) {
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!("missing snapshot {name}. Create it with UPDATE_SNAPSHOTS=1 cargo test")
    });

    if expected != actual {
        panic!(
            "render tree changed for `{name}`.\n\
             Read the diff: it is the review. If the change is intended, the \n\
             rewrite is somebody's decision and the pull request carries the \n\
             `golden-update` label to say they took it:\n  \
             REBLESS_SNAPSHOTS=1 cargo test --test render_snapshots\n\n\
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

/// A translucent card, the shape both backdrop-blur scenarios blur behind.
fn panel() -> Container {
    box_of(80.0, 60.0)
        .background(Color::rgba(1.0, 1.0, 1.0, 0.15))
        .corners(16.0)
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
                    .corners(4.0)
                    .translate((0.0, i as f32))
            }))
    };

    let view = container()
        .layout(Flex::row().spacing(16.0))
        .padding(8.0)
        .child(
            box_of(200.0, 200.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .corners(8.0)
                .scroll(Scroll::vertical())
                .child(column(20)),
        )
        .child(
            box_of(200.0, 200.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .scroll(Scroll::vertical())
                .child(column(2)),
        )
        .child(
            box_of(200.0, 80.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .scroll(Scroll::horizontal())
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
    let card = |c: Color| swatch(80.0, 40.0, c).corners(6.0);

    let view = container()
        .layout(Flex::column().spacing(12.0))
        .padding(20.0)
        .child(card(Color::RED).rotate(30.0))
        .child(card(Color::GREEN).rotate(30.0).pivot(Pivot::TOP_LEFT))
        .child(card(Color::BLUE).scale(1.5))
        .child(card(Color::YELLOW).scale((2.0, 0.5)))
        .child(card(Color::CYAN).translate((15.0, -5.0)))
        .child(
            box_of(200.0, 80.0)
                .background(Color::rgb(0.2, 0.2, 0.25))
                .rotate(10.0)
                .child(card(Color::MAGENTA).rotate(20.0).scale(0.8)),
        );

    assert_snapshot("transforms_and_origins", render(view, 400.0, 600.0));
}

/// After `examples/showcase.rs` and `shadow_example.rs`: the corner curvature
/// family and a ladder of shadows, both of which land entirely in the emitted
/// draw commands.
#[test]
fn corners_borders_and_shadows() {
    let base = |c: Color| swatch(70.0, 70.0, c).corners(16.0);

    let view = container()
        .layout(Flex::column().spacing(10.0))
        .padding(10.0)
        .child(
            container()
                .layout(Flex::row().spacing(10.0))
                .child(base(Color::rgb(0.3, 0.5, 0.9)))
                .child(base(Color::rgb(0.3, 0.5, 0.9)).corners(Corners::squircle(16.0)))
                .child(base(Color::rgb(0.3, 0.5, 0.9)).corners(Corners::bevel(16.0)))
                .child(base(Color::rgb(0.3, 0.5, 0.9)).corners(Corners::scoop(16.0)))
                .child(base(Color::rgb(0.3, 0.5, 0.9)).corners(Corners::superellipse(16.0, 1.5))),
        )
        .child(
            container()
                .layout(Flex::row().spacing(10.0))
                .children(common::LADDER.iter().map(|&step| {
                    swatch(60.0, 60.0, Color::rgb(0.9, 0.9, 0.95))
                        .corners(8.0)
                        .shadow(step)
                })),
        )
        .child(
            container()
                .layout(Flex::row().spacing(10.0))
                .child(base(Color::TRANSPARENT).border(2.0, Color::WHITE))
                .child(
                    box_of(70.0, 70.0)
                        .corners(CornerRadii {
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

    assert_snapshot("corners_borders_and_shadows", render(view, 500.0, 400.0));
}

/// A border takes room inside its box, as CSS's `border-box` does (#767): a
/// child of a box with no padding starts at the border width, a box sized to
/// its content grows by border and padding, a fill child stops at the border,
/// and a scroller's viewport and extents are inside its border — its content
/// starts there, and its handle is the share of the track that 90 of 180 is,
/// not 100 of 180.
#[test]
fn a_border_takes_room_inside_its_box() {
    let rows = container().layout(Flex::column()).children((0..6).map(|i| {
        swatch(
            90.0,
            30.0,
            if i % 2 == 0 { Color::RED } else { Color::BLUE },
        )
    }));
    let view = container()
        .layout(Flex::row().spacing(10.0))
        .padding(10.0)
        .child(
            box_of(60.0, 60.0)
                .border(4.0, Color::WHITE)
                .child(swatch(20.0, 20.0, Color::RED)),
        )
        .child(
            container()
                .border(3.0, Color::WHITE)
                .padding(2.0)
                .child(swatch(20.0, 20.0, Color::GREEN)),
        )
        .child(
            box_of(60.0, 60.0).border(5.0, Color::WHITE).child(
                container()
                    .width(fill())
                    .height(fill())
                    .background(Color::BLUE),
            ),
        )
        .child(
            box_of(100.0, 100.0)
                .border(5.0, Color::WHITE)
                .scroll(Scroll::vertical())
                .child(rows),
        );

    assert_snapshot(
        "a_border_takes_room_inside_its_box",
        render(view, 500.0, 200.0),
    );
}

/// An outline is a ring outside its box, drawn after everything the box holds
/// and taking no room (#767): the siblings of an outlined box sit where they
/// would without it, a rounded corner grows by the ring's reach and a square
/// one stays square, a negative offset draws the ring inside the box, and a
/// scroller's outline is drawn over its scrollbar.
#[test]
fn an_outline_takes_no_room() {
    let view = container()
        .layout(Flex::row().spacing(10.0))
        .padding(20.0)
        .child(
            box_of(60.0, 60.0)
                .background(Color::rgb(0.2, 0.2, 0.3))
                .corners(10.0)
                .outline(2.0, Color::WHITE, 3.0)
                .child(swatch(80.0, 20.0, Color::GREEN)),
        )
        .child(swatch(20.0, 60.0, Color::RED))
        .child(
            box_of(60.0, 60.0)
                .background(Color::rgb(0.2, 0.2, 0.3))
                .outline(2.0, Color::WHITE, 4.0),
        )
        .child(
            box_of(60.0, 60.0)
                .background(Color::rgb(0.2, 0.2, 0.3))
                .corners(Corners::squircle(12.0))
                .outline(3.0, Color::WHITE, -6.0),
        )
        .child(
            box_of(80.0, 60.0)
                .background(Color::rgb(0.2, 0.2, 0.3))
                .outline(2.0, Color::WHITE, 2.0)
                .scroll(Scroll::vertical())
                .child(box_of(80.0, 200.0)),
        );

    assert_snapshot("an_outline_takes_no_room", render(view, 500.0, 120.0));
}

/// After `examples/blur_example.rs`: the backdrop blur, which is the one command
/// carrying a property the golden is the only reader of — `sources` decides
/// which half of the effect runs, and until this scene existed no golden held a
/// blur command at all.
///
/// The transformed cases are here for what reaches the *renderer*: the command
/// keeps its own rect and radii and carries the matrix, so the golden is where a
/// transform silently baked into the geometry would show up. What the compositor
/// is told is derived from that matrix later and is not in this file — the
/// arithmetic is measured in `renderer::flatten`, and the wiring end to end in
/// `a_transformed_blur_publishes_the_shape_it_is_drawn_as`.
#[test]
fn backdrop_blur_sources_and_geometry() {
    let frosted = |radius: f32| panel().backdrop_blur(radius);

    let view = container()
        .layout(Flex::row().spacing(20.0))
        .padding(20.0)
        .child(frosted(24.0))
        .child(
            frosted(24.0).backdrop_blur(BackdropBlur::new(24.0).sources(BackdropSources::SURFACE)),
        )
        .child(
            frosted(24.0)
                .backdrop_blur(BackdropBlur::new(24.0).sources(BackdropSources::COMPOSITOR)),
        )
        // A radius of zero is "no blur", so this one emits no command at all.
        .child(frosted(0.0))
        .child(frosted(24.0).rotate(45.0))
        .child(frosted(24.0).scale(2.0));

    assert_snapshot("backdrop_blur", render(view, 700.0, 200.0));
}

/// After `examples/blur_example.rs`, which asks the compositor to blur the
/// desktop behind a translucent panel and nothing of its own.
///
/// The radius is the radius of the blur guido runs in its own shader.
/// `ext-background-effect-v1` carries none, so a compositor source is asked for
/// by its sources alone — and a zero radius there is a request, not a refusal.
/// Where the surface is a source the radius is the only thing that could say
/// "off", so zero still means off.
#[test]
fn a_compositor_blur_needs_no_radius_of_its_own() {
    let view = container()
        .layout(Flex::row().spacing(20.0))
        .padding(20.0)
        // Asks for a region and draws nothing itself.
        .child(panel().backdrop_blur(BackdropBlur::new(0.0).sources(BackdropSources::COMPOSITOR)))
        // Both sources, and a radius of zero: still off, and no command at all.
        .child(panel().backdrop_blur(BackdropBlur::new(0.0)));

    assert_snapshot(
        "compositor_blur_without_a_radius",
        render(view, 300.0, 120.0),
    );
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
                .corners(12.0)
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
                        .corners(8.0)
                        // A box only lays content out larger than itself along an axis it
                        // scrolls, so the overflow the clip cuts is the content's own.
                        .scroll(Scroll::horizontal().visibility(ScrollbarVisibility::Hidden))
                        .child(swatch(200.0, 30.0, Color::GREEN)),
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
            |(_, height)| swatch(120.0, height, Color::rgb(0.3, 0.3, 0.4)).corners(4.0),
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
            .corners(10.0)
            .when_hovered(|s| s.lighter(0.1))
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
                .scroll(Scroll::horizontal())
                .background(Color::rgb(0.15, 0.15, 0.2))
                .corners(6.0)
                .child(
                    container()
                        .layout(Flex::row().spacing(4.0))
                        .children((0..6).map(|_| swatch(30.0, 20.0, Color::CYAN))),
                ),
        )
        .child(module(50.0, Color::rgb(0.3, 0.2, 0.2)).shadow(common::LADDER[2]));

    assert_snapshot("bar_like_composition", render(view, 600.0, 36.0));
}

/// A line height that does not depend on the machine's fonts, for the two
/// scenarios below that hold a text: `Normal` is the drawing face's own, and
/// which face draws the default family is the machine's to say.
const FIXED_LINE: LineHeight = LineHeight::Relative(1.2);

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
            .corners(6.0)
            .child(
                text_input(value)
                    .color(Color::WHITE)
                    .font_size(14.0)
                    .line_height(FIXED_LINE),
            ),
    );

    assert_snapshot(
        "text_input_clips_overflowing_content",
        render(view, 400.0, 60.0),
    );
}

/// After `examples/simple_lock.rs`: a password field in a centred column on a
/// wide surface (#714). `at_least` is a floor and never a cap, so a field
/// declared at least 280 wide takes everything its column offers, as an input
/// with a CSS `min-width` does; the field the example now declares at exactly
/// 280 stays 280, centred.
#[test]
fn at_least_around_a_text_input_grows_to_what_it_is_offered() {
    let field = |width: Length| {
        container()
            .width(width)
            .padding(10.0)
            .background(Color::rgb(0.15, 0.15, 0.2))
            .corners(8.0)
            .child(
                text_input(create_signal(String::new()))
                    .font_size(14.0)
                    .line_height(FIXED_LINE),
            )
    };

    let view = container()
        .width(fill())
        .layout(
            Flex::column()
                .spacing(10.0)
                .cross_alignment(CrossAlignment::Center),
        )
        .child(field(at_least(280.0)))
        .child(field(Length::from(280.0)));

    assert_snapshot("at_least_around_a_text_input", render(view, 800.0, 120.0));
}

/// Texts of different sizes in a row, aligned on their baselines.
///
/// `Start` puts every child's top edge on the same line, so a 24px label and
/// a 12px one float at unrelated heights. `Baseline` puts them on the line
/// they are written on, which is what reads as one sentence. The two are
/// snapshotted together so the difference is visible as geometry, not prose.
#[test]
fn baseline_alignment_lines_text_up() {
    let row = |align: CrossAlignment| {
        container()
            .layout(Flex::row().spacing(8.0).cross_alignment(align))
            .child(container().child(text("Big").font_size(24.0).line_height(FIXED_LINE)))
            .child(container().child(text("small").font_size(12.0).line_height(FIXED_LINE)))
            .child(container().child(text("mid").font_size(16.0).line_height(FIXED_LINE)))
            // A box reports no baseline: it aligns by its bottom edge.
            .child(swatch(20.0, 30.0, Color::CYAN))
    };

    let view = container()
        .layout(Flex::column().spacing(10.0))
        .padding(8.0)
        .child(row(CrossAlignment::Start))
        .child(row(CrossAlignment::Baseline));

    assert_snapshot("baseline_alignment", render(view, 400.0, 160.0));
}

/// Where a declaration about input sits in the frame, which is the part the
/// region's composition depends on: before the container's own background, and
/// before the children whose declarations refine it.
#[test]
fn an_input_declaration_is_carried_in_paint_order() {
    let view = container()
        .padding(8.0)
        .layout(Flex::column().spacing(8.0))
        .child(
            container()
                .width(120.0)
                .height(40.0)
                .corners(20.0)
                .background(Color::rgb(0.2, 0.2, 0.3))
                .takes_input(true)
                .child(swatch(20.0, 20.0, Color::CYAN)),
        )
        .child(
            container()
                .width(120.0)
                .height(40.0)
                .background(Color::rgb(0.3, 0.2, 0.2))
                .takes_input(false)
                .child(swatch(20.0, 20.0, Color::MAGENTA).takes_input(true)),
        );

    assert_snapshot("input_regions", render(view, 200.0, 140.0));
}

/// A tint is part of what an image draws (#545), and only a tinted image's
/// line says so: an untinted one reads as it always has.
#[test]
fn a_tinted_image_carries_its_tint() {
    let icon = || {
        ImageSource::SvgBytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"/>"#
                .to_vec()
                .into(),
        )
    };
    let view = container()
        .padding(8.0)
        .layout(Flex::row().spacing(8.0))
        .child(box_of(16.0, 16.0).child(image(icon())))
        .child(box_of(16.0, 16.0).child(image(icon()).tint(Color::RED)));

    assert_snapshot("tinted_image", render(view, 64.0, 32.0));
}

/// A widget that strokes a sparkline and fills a triangle under it.
struct Chart;

impl Widget for Chart {
    fn layout(
        &mut self,
        _ctx: &mut guido::tree::LayoutCtx,
        constraints: Constraints,
    ) -> guido::layout::Size {
        constraints.constrain(guido::layout::Size::new(40.0, 16.0))
    }

    fn paint(&self, ctx: &mut guido::renderer::PaintContext) {
        use guido::renderer::{Fill, Join, Path, Stroke};
        let line = Path::new()
            .move_to((0.0, 12.0))
            .line_to((10.0, 4.0))
            .line_to((20.0, 9.0))
            .quad_to((30.0, 0.0), (40.0, 6.0));
        ctx.draw_path(
            &Path::new()
                .move_to((0.0, 16.0))
                .line_to((40.0, 16.0))
                .line_to((40.0, 6.0))
                .close(),
            Fill::color(Color::BLUE),
            None,
        );
        ctx.draw_path(
            &line,
            None,
            Stroke::new(1.6, Color::WHITE).joins(Join::Round),
        );
    }
}

/// An arc and a path are what was drawn, in the order it was drawn: a ring's
/// track before the part its progress covers, and a chart's fill before the
/// line over it.
#[test]
fn arcs_and_paths_are_carried_in_paint_order() {
    let view = container()
        .padding(8.0)
        .layout(Flex::row().spacing(8.0))
        .child(
            arc()
                .size(20.0)
                .stroke(3.0)
                .track(Color::WHITE.with_alpha(0.25))
                .color(Color::GREEN)
                .caps(Cap::Round)
                .start(-90.0)
                .progress(0.25),
        )
        .child(Chart);

    assert_snapshot("arcs_and_paths", render(view, 96.0, 36.0));
}

// ---------------------------------------------------------------------------
// A dynamic child lays out as the same child added statically (#558)
// ---------------------------------------------------------------------------

/// A `ZStack` follower that fills: sized to the sibling that leads the stack,
/// whether it was added as a widget or as a closure.
///
/// Red before #558: the wrapper a dynamic child is adopted as did not forward
/// `layout_hints`, so the stack saw no fill, let the follower lead, and grew to
/// the whole viewport with everything centred around it.
#[test]
fn a_dynamic_zstack_follower_fills_like_a_static_one() {
    let follower = || {
        container()
            .width(fill())
            .height(fill())
            .layout(Flex::column().center())
            .child(swatch(100.0, 20.0, Color::BLUE))
    };
    let view = |dynamic: bool| {
        let stack = container()
            .layout(ZStack::new())
            .child(swatch(360.0, 90.0, Color::RED));
        let stack = if dynamic {
            stack.child(move || Some(follower()))
        } else {
            stack.child(follower())
        };
        container()
            .width(fill())
            .height(fill())
            .layout(Flex::column().center())
            .child(stack)
    };
    assert_eq!(
        render(view(true), 1000.0, 800.0),
        render(view(false), 1000.0, 800.0),
        "the dynamic follower laid out differently from the static one"
    );
}

/// A stack that aligns its children (#781): centred by the stack, a badge on
/// the top-end corner and a tag on the bottom-start one by their own
/// `align_self`, and a background that follows the stack's size and so stays
/// where the stack is. A second stack sends its child to the bottom centre.
#[test]
fn an_aligned_stack_places_each_child_by_its_alignment() {
    let view = container()
        .layout(Flex::row().spacing(20.0))
        .padding(20.0)
        .child(
            box_of(200.0, 100.0)
                .layout(ZStack::new().align(Alignment::CENTER))
                .child(
                    container()
                        .width(fill())
                        .height(fill())
                        .background(Color::WHITE),
                )
                .child(swatch(60.0, 30.0, Color::BLUE))
                .child(swatch(12.0, 12.0, Color::RED).align_self(Alignment::TOP_END))
                .child(swatch(40.0, 10.0, Color::GREEN).align_self(Alignment::BOTTOM_START)),
        )
        .child(
            box_of(40.0, 100.0)
                .layout(ZStack::new().align(Alignment::BOTTOM_CENTER))
                .child(
                    container()
                        .width(fill())
                        .height(fill())
                        .background(Color::WHITE),
                )
                .child(swatch(20.0, 20.0, Color::BLUE)),
        );

    assert_snapshot("an_aligned_stack", render(view, 400.0, 200.0));
}

/// A child at its natural size inside parents narrower than it (#782): in a
/// clipping stack that centres it, overhanging both sides by 30 and cut there;
/// in a parent that does not clip, running past its end as it paints; and,
/// for the difference, an undeclared length in the same clipping stack, cut
/// to the parent's 100.
#[test]
fn a_natural_child_overhangs_a_narrower_parent() {
    let content = |width: Length| {
        container()
            .width(width)
            .height(natural())
            .child(swatch(160.0, 20.0, Color::BLUE))
    };
    let view = container()
        .layout(Flex::column().spacing(20.0))
        .padding(20.0)
        .child(
            box_of(100.0, 40.0)
                .overflow(Overflow::Hidden)
                .background(Color::WHITE)
                .layout(ZStack::new().align(Alignment::CENTER))
                .child(content(natural())),
        )
        .child(
            box_of(100.0, 40.0)
                .background(Color::WHITE)
                .child(content(natural())),
        )
        .child(
            box_of(100.0, 40.0)
                .overflow(Overflow::Hidden)
                .background(Color::WHITE)
                .layout(ZStack::new().align(Alignment::CENTER))
                .child(content(Length::default())),
        );

    assert_snapshot("a_natural_child", render(view, 300.0, 200.0));
}

/// A `Flex` row of `[100, fill, 50]` in 400: the fill takes the 250 left over,
/// whether it was added as a widget or as a closure.
///
/// Red before #558: measured as a child that does not fill, against the whole
/// 400, it pushed the last sibling out of the row.
#[test]
fn a_dynamic_flex_child_fills_like_a_static_one() {
    let filler = || {
        container()
            .width(fill())
            .height(10.0)
            .background(Color::BLUE)
    };
    let view = |dynamic: bool| {
        let row = container()
            .layout(Flex::row())
            .child(swatch(100.0, 10.0, Color::RED));
        let row = if dynamic {
            row.child(move || Some(filler()))
        } else {
            row.child(filler())
        };
        row.child(swatch(50.0, 10.0, Color::GREEN))
    };
    assert_eq!(
        render(view(true), 400.0, 10.0),
        render(view(false), 400.0, 10.0),
        "the dynamic fill child laid out differently from the static one"
    );
}

/// A row 200 wide whose children and gaps come to 276: two clipping panels
/// that give the overflow up in proportion to their widths, a padded one held
/// at its padding, and a button whose icon has an exact width and so keeps it
/// — the button is never narrower than the icon and its padding. Under it a
/// column 100 tall, where a scroller under a header takes what the header
/// leaves instead of running past the column.
#[test]
fn children_that_do_not_fit_share_the_overflow() {
    let clipped = |w: f32, c: Color| {
        container()
            .overflow(Overflow::Hidden)
            .background(c)
            .child(box_of(w, 20.0))
    };
    let row = container()
        .width(200.0)
        .layout(Flex::row().spacing(4.0))
        .child(clipped(120.0, Color::RED))
        .child(clipped(60.0, Color::GREEN))
        .child(
            container()
                .padding([2.0, 25.0])
                .background(Color::BLUE)
                .child(clipped(10.0, Color::BLACK)),
        )
        .child(
            container()
                .padding(4.0)
                .background(Color::WHITE)
                .child(swatch(16.0, 16.0, Color::CYAN)),
        );
    let column = container()
        .height(100.0)
        .layout(Flex::column())
        .child(swatch(40.0, 30.0, Color::RED))
        .child(
            container()
                .scroll(Scroll::vertical())
                .background(Color::GREEN)
                .child(box_of(40.0, 200.0)),
        );
    let view = container()
        .layout(Flex::column().spacing(10.0))
        .padding(8.0)
        .child(row)
        .child(column);

    assert_snapshot("children_that_do_not_fit", render(view, 400.0, 200.0));
}

/// A container merging its children's backgrounds: the merged command appears
/// once, at the container's place in paint order — after its own background,
/// before what its children hold — and the backgrounds it stands in for are
/// not drawn on their own as well.
#[test]
fn merged_children_are_drawn_once_where_their_container_is() {
    let island = container()
        .width(120.0)
        .height(36.0)
        .corners(18.0)
        .background(Color::BLACK)
        .border(1.0, Color::WHITE)
        .padding(10.0)
        .child(swatch(16.0, 16.0, Color::GREEN));
    let bubble = swatch(36.0, 36.0, Color::BLACK)
        .corners(18.0)
        .translate((6.0, 0.0));
    let view = container().padding(8.0).child(
        container()
            .layout(Flex::row())
            .background(Color::rgba(1.0, 1.0, 1.0, 0.1))
            .merge_children(14.0)
            .child(island)
            .child(bubble),
    );

    assert_snapshot(
        "merged_children_are_drawn_once_where_their_container_is",
        render_flattened(view, 240.0, 60.0),
    );
}
