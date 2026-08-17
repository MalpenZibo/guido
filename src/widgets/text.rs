use crate::default_font_family;
use crate::jobs::JobType;
use crate::layout::{Constraints, Size};
use crate::reactive::{IntoSignal, OptionSignalExt, Signal, with_signal_tracking};
use crate::renderer::{PaintContext, measure_text_full};
use crate::tree::{Tree, WidgetId};

use super::font::{FontFamily, FontWeight};
use super::text_style::{TextShadow, TextStroke};
use super::widget::{Color, Rect, Widget};

/// How far a stroke and a shadow reach past the glyphs they decorate.
///
/// Used for the damage slop, not for layout: decoration must not change how
/// much room the text takes, or a shadow would push its neighbours around.
pub(crate) fn decoration_overflow(stroke: Option<TextStroke>, shadow: Option<TextShadow>) -> f32 {
    let from_stroke = stroke.map(|s| s.width.max(0.0)).unwrap_or(0.0);
    let from_shadow = shadow
        .map(|s| s.blur.max(0.0) + s.offset.0.abs().max(s.offset.1.abs()))
        .unwrap_or(0.0);
    from_stroke.max(from_shadow)
}

/// A run of text.
///
/// Content only: colour, size, family and weight are declared on an enclosing
/// container and inherited from the nearest one that sets them — see
/// [`TextStyle`](super::TextStyle).
///
/// ```ignore
/// container().font_size(21.0).text_color(theme.text)
///     .child(text("Hello"))
/// ```
pub struct Text {
    content: Signal<String>,
    /// If true, text won't wrap and will be clipped by parent container
    nowrap: bool,
    /// Cached values for painting (avoid re-reading signals)
    cached_text: String,
    cached_font_size: f32,
    cached_font_family: FontFamily,
    cached_font_weight: FontWeight,
}

impl Text {
    pub fn new<M>(content: impl IntoSignal<String, M>) -> Self {
        let content = content.into_signal();
        // Don't read content during widget creation - this would register layout dependencies
        // with the wrong widget (the parent container that's currently being laid out).
        // The cached_text will be populated during the first layout via refresh().
        let default_family = default_font_family();
        Self {
            content,
            nowrap: false,
            cached_text: String::new(), // Will be set during first layout
            cached_font_size: 14.0,
            cached_font_family: default_family,
            cached_font_weight: FontWeight::NORMAL,
        }
    }

    /// Prevent text from wrapping. Text will be clipped by parent container.
    /// Use this for text inside animated containers to prevent re-wrapping during animation.
    pub fn nowrap(mut self) -> Self {
        self.nowrap = true;
        self
    }

    /// Refresh cached values from the inherited style.
    ///
    /// The signals are read here, inside this widget's tracking scope, so the
    /// text is re-laid-out when whichever ancestor supplied a metric changes
    /// it — and is left alone when an ancestor it does not depend on changes.
    ///
    /// Returns how far the decoration reaches past the glyphs, which the
    /// caller records as damage slop.
    fn refresh(&mut self, tree: &Tree, id: WidgetId) -> f32 {
        with_signal_tracking(id, JobType::Layout, || {
            let style = tree.inherited_text_style(id);
            self.cached_text = self.content.get();
            self.cached_font_size = style.font_size.get_or(14.0);
            self.cached_font_family = style.font_family.get_or_else(default_font_family);
            self.cached_font_weight = style.font_weight.get_or(FontWeight::NORMAL);
            decoration_overflow(style.stroke.map(|s| s.get()), style.shadow.map(|s| s.get()))
        })
    }
}

impl Widget for Text {
    fn layout(&mut self, tree: &mut Tree, id: WidgetId, constraints: Constraints) -> Size {
        // Text widgets are never relayout boundaries
        tree.set_relayout_boundary(id, false);

        // Same early-out as Container: an unchanged text under unchanged
        // constraints neither re-measures nor gets repainted, even when an
        // ancestor re-runs its layout. Content and style changes come through
        // signals tracked below, which mark this widget for layout.
        let constraints_changed = tree.cached_constraints(id) != Some(constraints);
        let reactive_changed = tree.needs_layout(id);
        if !(constraints_changed || reactive_changed) {
            crate::render_stats::record_layout_skipped();
            return tree.cached_size(id).unwrap_or_default();
        }
        crate::render_stats::record_layout_executed_with_reasons(
            crate::render_stats::LayoutReasons {
                constraints_changed,
                reactive_changed,
            },
        );

        // Refresh cached values from content and inherited style.
        // This reads signals and registers layout dependencies.
        let overflow = self.refresh(tree, id);
        tree.set_paint_overflow(id, overflow);

        // Determine the effective max_width for measurement
        // If nowrap is true, don't pass max_width so text won't wrap
        let max_width = if self.nowrap {
            None
        } else if constraints.max_width.is_finite() {
            Some(constraints.max_width)
        } else {
            None
        };

        // Measure text (TextMeasurer caches results internally)
        let measured = measure_text_full(
            &self.cached_text,
            self.cached_font_size,
            max_width,
            &self.cached_font_family,
            self.cached_font_weight,
        );

        // A parent aligning on the baseline needs this; it comes out of the
        // same shaping pass, so reporting it is free.
        tree.set_baseline(id, measured.baseline);

        let size = Size::new(
            measured
                .size
                .width
                .max(constraints.min_width)
                .min(constraints.max_width),
            measured
                .size
                .height
                .max(constraints.min_height)
                .min(constraints.max_height),
        );

        // Cache constraints and size for partial layout
        tree.cache_layout(id, constraints, size);

        // Clear needs_layout flag since layout is complete
        tree.clear_needs_layout(id);

        size
    }

    fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext) {
        // Draw in LOCAL coordinates (0,0 is widget origin)
        // Parent Container sets position transform
        let size = tree.cached_size(id).unwrap_or_default();
        let local_bounds = Rect::new(0.0, 0.0, size.width, size.height);
        // Read the painted properties with tracking so a change on whichever
        // ancestor supplied them repaints this text and nothing else.
        let (color, stroke, shadow) = with_signal_tracking(id, JobType::Paint, || {
            let style = tree.inherited_text_style(id);
            (
                style.color.get_or(Color::WHITE),
                style.stroke.map(|s| s.get()),
                style.shadow.map(|s| s.get()),
            )
        });
        ctx.draw_text_decorated(
            &self.cached_text,
            local_bounds,
            color,
            self.cached_font_size,
            self.cached_font_family.clone(),
            self.cached_font_weight,
            stroke,
            shadow,
        );
    }
}

/// Create a text widget
///
/// Accepts static strings, closures, or signals:
/// ```ignore
/// text("Hello")  // static string
/// text(move || format!("Count: {}", count.get()))  // reactive closure
/// text(my_signal)  // reactive signal
/// ```
///
/// Styling lives on an enclosing container:
/// ```ignore
/// container().font_size(18.0).bold().child(text("Hello"))
/// ```
pub fn text<M>(content: impl IntoSignal<String, M>) -> Text {
    Text::new(content)
}

#[cfg(test)]
mod tests {
    use rustc_hash::FxHashSet;

    use super::*;
    use crate::jobs;
    use crate::layout::Constraints;
    use crate::reactive::create_signal;
    use crate::renderer::{DrawCommand, RenderNode};
    use crate::widgets::container;

    /// Lay out and paint, returning the first text command's colour and size.
    ///
    /// The queued jobs are processed first, which is what turns a signal write
    /// into `needs_layout` on the widgets that read it — and, because
    /// `mark_needs_layout` walks to the relayout boundary, on the ancestors
    /// that have to descend to reach them. Skip it and every container takes
    /// its unchanged-constraints early-out, so the text is never asked to lay
    /// out again and the test measures the cache instead of the resolution.
    fn frame(tree: &mut Tree, root: WidgetId) -> (Color, f32) {
        let roots: FxHashSet<WidgetId> = [root].into_iter().collect();
        jobs::distribute_jobs(tree, &roots);
        let drained = jobs::drain_surface_jobs(root);
        let mut layout_roots = Vec::new();
        jobs::process_jobs(&drained, tree, &mut layout_roots);
        jobs::recycle_job_buffer(drained);
        jobs::recycle_job_buffer(jobs::drain_orphan_jobs());

        tree.with_widget_mut(root, |w, id, t| {
            w.layout(t, id, Constraints::new(0.0, 0.0, 800.0, 600.0))
        });
        let mut node = RenderNode::new(root.as_u64());
        tree.with_widget_mut(root, |w, id, t| {
            let mut ctx = PaintContext::new(&mut node);
            w.paint(t, id, &mut ctx);
        });

        fn find(node: &RenderNode) -> Option<(Color, f32)> {
            for cmd in &node.commands {
                if let DrawCommand::Text {
                    color, font_size, ..
                } = &**cmd
                {
                    return Some((*color, *font_size));
                }
            }
            node.children.iter().find_map(|c| find(c))
        }
        find(&node).expect("a text command")
    }

    #[test]
    fn a_text_follows_the_ancestor_signal_it_resolved_from() {
        let size = create_signal(10.0f32);
        let color = create_signal(Color::RED);

        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            container()
                .font_size(move || size.get())
                .text_color(move || color.get())
                // A plain container in between: the text must still end up
                // subscribed to the signals two levels up.
                .child(container().child(text("hi"))),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        assert_eq!(frame(&mut tree, root), (Color::RED, 10.0));

        size.set(40.0);
        color.set(Color::BLUE);

        assert_eq!(
            frame(&mut tree, root),
            (Color::BLUE, 40.0),
            "a change to an inherited declaration must re-measure and repaint \
             the text that read it"
        );
    }
}
