use crate::default_font_family;
use crate::jobs::JobType;
use crate::layout::{Constraints, Size};
use crate::reactive::signal::{RwSignal, create_signal};
use crate::reactive::{IntoSignal, OptionSignalExt, Signal, with_signal_tracking};
use crate::renderer::{PaintContext, measure_text_full};
use crate::tree::{Tree, WidgetId};

use super::control::Control;
use super::font::{FontFamily, FontWeight};
use super::state_layer::{StateWhen, Stateful};
use super::text_style::{TextShadow, TextStroke, TextStyle, TextStyled};
use super::widget::{Color, Event, EventResponse, Rect, Widget};

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
/// Style may be declared here — [`TextStyled`](super::TextStyled) — or on an
/// enclosing container, in which case it is inherited from the nearest one
/// that sets each property. A declaration on the text itself is the nearest
/// there is, so it wins.
///
/// ```ignore
/// text("Hello").font_size(21.0).color(theme.text)
/// container().font_size(21.0).child(text("Hello"))   // same, for a group
/// ```
pub struct Text {
    content: Signal<String>,
    /// What this text declares about itself. Boxed and absent by default: a
    /// text that says nothing pays a null check, not a struct.
    style: Option<Box<TextStyle>>,
    /// Overrides that apply while this text's control is in a state, in
    /// declaration order. Empty for almost every text there is.
    states: Vec<(StateWhen, TextStyle)>,
    /// Set only once a pointer state is declared, and read only when no
    /// control encloses this text — the case where it is its own unit and has
    /// to notice the pointer itself.
    own_hover: Option<RwSignal<bool>>,
    /// If true, text won't wrap and will be clipped by parent container
    nowrap: bool,
    /// Blur radius for the backdrop the glyphs cut out of what is behind them.
    /// `None` for every text that is not made of glass.
    backdrop_blur: Option<Signal<f32>>,
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
            style: None,
            states: Vec::new(),
            own_hover: None,
            nowrap: false,
            backdrop_blur: None,
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

    /// Blur what is behind the glyphs, so the text reads as frosted glass.
    ///
    /// The letters become the window: what they cover is drawn blurred, and the
    /// text's own colour is the tint laid over it — which is why this is usually
    /// paired with a translucent one.
    ///
    /// ```ignore
    /// text("09:41")
    ///     .font_size(76.0)
    ///     .color(Color::rgba(1.0, 1.0, 1.0, 0.35))
    ///     .backdrop_blur(16.0)
    /// ```
    ///
    /// It filters what *this surface* has already drawn — a wallpaper, a photo,
    /// the panel underneath — and only that. A container's
    /// [`backdrop_blur`](super::Container::backdrop_blur) can also reach the
    /// desktop behind the surface, through `ext-background-effect-v1`; that
    /// protocol takes a region, and regions are rectangles, so glyphs cannot be
    /// expressed in it.
    ///
    /// Unlike the rest of a text's style this is not inherited from a container,
    /// on purpose: each frosted text ends the render pass to filter the target,
    /// so it is asked for one text at a time rather than dressed onto a subtree.
    /// A rotated or scaled text ignores it — the mask is axis-aligned.
    ///
    /// Legibility is not what it buys. Frost softens the background instead of
    /// darkening it, so over a busy photograph
    /// [`text_shadow`](super::TextStyled::text_shadow) still does more; the two
    /// compose.
    pub fn backdrop_blur<M>(mut self, radius: impl IntoSignal<f32, M>) -> Self {
        self.backdrop_blur = Some(radius.into_signal());
        self
    }

    /// This text's own declaration, completed by the ancestors' for whatever
    /// it leaves out. The walk is skipped when nothing is left to find.
    ///
    /// Called inside the caller's tracking scope, like the walk it wraps: the
    /// signals come back unread, and reading them is what subscribes.
    fn resolved_style(&self, tree: &Tree, id: WidgetId) -> TextStyle {
        let mut style = TextStyle::default();
        // Active overrides first, last declared first, so they outrank the
        // text's own declaration — and `inherit_from` takes only what is still
        // missing, which is what makes the whole chain resolve per property.
        if !self.states.is_empty() {
            let control = tree.nearest_control(id);
            for (when, override_) in self.states.iter().rev() {
                if self.is_state_active(id, control.as_ref(), when) {
                    style.inherit_from(override_);
                }
            }
        }
        if let Some(own) = self.style.as_deref() {
            style.inherit_from(own);
        }
        if !style.is_complete() {
            style.inherit_from(&tree.inherited_text_style(id));
        }
        style
    }

    /// Whether an override applies. Reading the answer is what subscribes the
    /// text to the control, so it is asked only for a state it declares.
    fn is_state_active(&self, id: WidgetId, control: Option<&Control>, when: &StateWhen) -> bool {
        match (when, control) {
            (StateWhen::When(condition), _) => condition.get(),
            (StateWhen::Hovered, Some(control)) => control.is_hovered(),
            (StateWhen::Pressed, Some(control)) => control.is_pressed(),
            (StateWhen::Focused, Some(control)) => control.has_focus(),
            // No control above: this text is its own unit. It can notice the
            // pointer over its own bounds, and it can hold the focus if
            // something gave it — but it cannot be pressed, because being
            // pressed means being activated and it has nothing to activate.
            (StateWhen::Hovered, None) => self.own_hover.is_some_and(|h| h.get()),
            (StateWhen::Focused, None) => crate::reactive::focus::focus_path().contains(id),
            (StateWhen::Pressed, None) => false,
        }
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
            let style = self.resolved_style(tree, id);
            self.cached_text = self.content.get();
            self.cached_font_size = style.font_size.get_or(14.0);
            self.cached_font_family = style.font_family.get_or_else(default_font_family);
            self.cached_font_weight = style.font_weight.get_or(FontWeight::NORMAL);
            decoration_overflow(style.stroke.map(|s| s.get()), style.shadow.map(|s| s.get()))
        })
    }
}

impl Stateful for Text {
    type Style = TextStyle;

    fn push_state_style(&mut self, when: StateWhen, style: TextStyle) {
        if matches!(when, StateWhen::Hovered) && self.own_hover.is_none() {
            self.own_hover = Some(create_signal(false));
        }
        self.states.push((when, style));
    }
}

impl TextStyled for Text {
    fn text_style_mut(&mut self) -> &mut TextStyle {
        self.style.get_or_insert_with(Box::default)
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

    /// Notice the pointer, but only for a text that is its own interaction
    /// unit. Inside a control it is the control that tracks, and asking twice
    /// would light this text on its own glyphs rather than on the button whose
    /// label it is.
    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
        let Some(hover) = self.own_hover else {
            return EventResponse::Ignored;
        };
        if tree.nearest_control(id).is_some() {
            return EventResponse::Ignored;
        }

        let inside = match event {
            Event::MouseMove { x, y } | Event::MouseEnter { x, y } => tree
                .get_bounds(id)
                .is_some_and(|bounds| bounds.contains(*x, *y)),
            Event::MouseLeave => false,
            _ => return EventResponse::Ignored,
        };
        // Written only on a change: an unchanged pointer move must not wake
        // every subscriber.
        if hover.get_untracked() != inside {
            hover.set(inside);
        }
        // Never handled: noticing the pointer is not consuming it.
        EventResponse::Ignored
    }

    fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext) {
        // Draw in LOCAL coordinates (0,0 is widget origin)
        // Parent Container sets position transform
        let size = tree.cached_size(id).unwrap_or_default();
        let local_bounds = Rect::new(0.0, 0.0, size.width, size.height);
        // Read the painted properties with tracking so a change on whichever
        // ancestor supplied them repaints this text and nothing else.
        let (color, stroke, shadow, blur) = with_signal_tracking(id, JobType::Paint, || {
            let style = self.resolved_style(tree, id);
            (
                style.color.get_or(Color::WHITE),
                style.stroke.map(|s| s.get()),
                style.shadow.map(|s| s.get()),
                self.backdrop_blur.map(|radius| radius.get()),
            )
        });
        // First, so the glyphs and their decorations land over the frost.
        if let Some(radius) = blur {
            ctx.draw_text_backdrop_blur(
                &self.cached_text,
                local_bounds,
                radius,
                self.cached_font_size,
                self.cached_font_family.clone(),
                self.cached_font_weight,
            );
        }
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
    use crate::widgets::text_style::TextStyled;

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

    /// Every command a paint produced, in order, as short names.
    fn commands(widget: impl Widget + 'static) -> Vec<&'static str> {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(widget));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        tree.with_widget_mut(root, |w, id, t| {
            w.layout(t, id, Constraints::new(0.0, 0.0, 800.0, 600.0))
        });
        let mut node = RenderNode::new(root.as_u64());
        tree.with_widget_mut(root, |w, id, t| {
            let mut ctx = PaintContext::new(&mut node);
            w.paint(t, id, &mut ctx);
        });
        node.commands
            .iter()
            .map(|cmd| match &**cmd {
                DrawCommand::Text { .. } => "text",
                DrawCommand::TextBackdropBlur { .. } => "frost",
                _ => "other",
            })
            .collect()
    }

    /// The frost filters what is already drawn, so it has to be asked for
    /// before the glyphs that sit on it — and before their decorations, which
    /// are glyphs too.
    #[test]
    fn a_frosted_text_asks_for_its_backdrop_before_it_draws() {
        assert_eq!(
            commands(text("hi").backdrop_blur(8.0)),
            vec!["frost", "text"]
        );
        let stroked = commands(
            text("hi")
                .backdrop_blur(8.0)
                .text_stroke(TextStroke::new(1.0, Color::BLACK)),
        );
        assert_eq!(stroked[0], "frost");
        assert!(
            stroked[1..].iter().all(|cmd| *cmd == "text"),
            "a stroke is more glyphs, and they go over the frost too: {stroked:?}"
        );
    }

    #[test]
    fn a_text_asks_for_nothing_it_would_not_use() {
        assert_eq!(commands(text("hi")), vec!["text"], "no blur, no command");
        assert_eq!(
            commands(text("hi").backdrop_blur(0.0)),
            vec!["text"],
            "a radius of zero is not an effect"
        );
        assert_eq!(
            commands(text("").backdrop_blur(8.0)),
            Vec::<&str>::new(),
            "nothing to cut a hole in the shape of"
        );
    }

    #[test]
    fn the_frost_radius_is_reactive_like_every_other_declaration() {
        let radius = create_signal(0.0f32);
        let mut tree = Tree::new();
        let root = tree.register(Box::new(text("hi").backdrop_blur(move || radius.get())));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let paint = |tree: &mut Tree| {
            tree.with_widget_mut(root, |w, id, t| {
                w.layout(t, id, Constraints::new(0.0, 0.0, 800.0, 600.0))
            });
            let mut node = RenderNode::new(root.as_u64());
            tree.with_widget_mut(root, |w, id, t| {
                let mut ctx = PaintContext::new(&mut node);
                w.paint(t, id, &mut ctx);
            });
            node.commands
                .iter()
                .filter(|cmd| matches!(&***cmd, DrawCommand::TextBackdropBlur { .. }))
                .count()
        };

        assert_eq!(paint(&mut tree), 0);
        radius.set(12.0);
        assert_eq!(
            paint(&mut tree),
            1,
            "the radius is read where it is painted"
        );
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

    /// A declaration on the text is the nearest one there is, per property:
    /// the size below comes from the text, the colour from the container.
    #[test]
    fn a_texts_own_declaration_wins_over_its_ancestors() {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            container()
                .font_size(10.0)
                .text_color(Color::RED)
                .child(text("hi").font_size(40.0)),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        assert_eq!(frame(&mut tree, root), (Color::RED, 40.0));
    }

    /// And it is reactive on the same terms as an inherited one: the signal is
    /// read where the style is resolved, inside the text's own scope.
    #[test]
    fn a_texts_own_declaration_follows_its_signal() {
        let color = create_signal(Color::RED);

        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            container().child(text("hi").color(move || color.get())),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        assert_eq!(frame(&mut tree, root).0, Color::RED);
        color.set(Color::BLUE);
        assert_eq!(frame(&mut tree, root).0, Color::BLUE);
    }
}
