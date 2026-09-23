use crate::default_font_family;
use crate::layout::{Constraints, Size};
use crate::reactive::signal::{RwSignal, create_signal};
use crate::reactive::{IntoSignal, Prop, Signal};
use crate::renderer::{LineFit, PaintContext, measure_text_full};
use crate::tree::{LayoutCtx, Tree, WidgetId};

use super::container::get_animated_value;
use super::control::Control;
use super::font::{FontFamily, FontWeight};
use super::state_layer::{StateWhen, Stateful};
use super::text_style::{DEFAULT_FONT_SIZE, TextAnims, TextShadow, TextStroke, TextStyle};
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

/// What a text cut short by [`Text::max_lines`] shows where the rest would
/// have been.
///
/// ```no_run
/// # use guido::prelude::*;
/// text("a window title far too long for the bar it sits in")
///     .max_lines(1)
///     .overflow(TextOverflow::Ellipsis);
/// ```
///
/// The line limit and the mark are two properties, as they are in CSS
/// (`line-clamp` and `text-overflow`), Flutter (`maxLines` and `overflow`) and
/// GTK (`lines` and `ellipsize`): how many lines a text may take is a layout
/// decision, and how the cut reads is a matter of taste.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextOverflow {
    /// The lines past the limit are not drawn, and the last one is cut where
    /// the box ends.
    #[default]
    Clip,
    /// The last line drawn ends in `…`.
    Ellipsis,
    /// The `…` opens the last line drawn, which shows the end of what it
    /// holds: a path whose file name is what matters.
    EllipsisStart,
    /// The `…` stands in the middle of the last line drawn, which keeps both
    /// ends.
    EllipsisMiddle,
}

/// A run of text.
///
/// Style is declared here — see `declares_text_style` — because this is the
/// widget that draws the glyphs. Whatever it does not declare falls back to
/// the defaults:
/// white, 14 logical pixels, the registered family, normal weight.
///
/// ```no_run
/// # use guido::prelude::*;
/// # struct Theme { text: Color }
/// # let theme = Theme { text: Color::WHITE };
/// text("Hello").font_size(21.0).color(theme.text);
/// ```
///
/// There is no enclosing declaration to inherit from: a container draws a box
/// and says nothing about what is written inside it. A style shared by several
/// labels is a function that returns one — see
/// [`text_style`](crate::widgets::text_style).
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
    /// Whether the text wraps at the width it is given. `None` is the default,
    /// which wraps — an absent signal costs a null check rather than a read.
    wrap: Prop<bool>,
    /// The most lines the text may take. `None`, the default, is as many as it
    /// needs.
    max_lines: Prop<Option<u32>>,
    /// What marks the cut when the text is longer than its lines.
    overflow: Prop<TextOverflow>,
    /// Blur radius for the backdrop the glyphs cut out of what is behind them.
    /// `None` for every text that is not made of glass.
    backdrop_blur: Prop<f32>,
    /// How the declared colour and size move, when they were declared with a
    /// motion. Absent for every text that only says what it is.
    anims: Option<Box<TextAnims>>,
    /// Cached values for painting (avoid re-reading signals)
    cached_text: String,
    cached_font_size: f32,
    cached_font_family: FontFamily,
    cached_font_weight: FontWeight,
    cached_wrap: bool,
    /// The lines layout cut the text to, handed to paint so every path that
    /// shapes it cuts in the same place.
    cached_fit: Option<LineFit>,
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
            wrap: Prop::Unset,
            max_lines: Prop::Unset,
            overflow: Prop::Unset,
            backdrop_blur: Prop::Unset,
            anims: None,
            cached_text: String::new(), // Will be set during first layout
            cached_font_size: DEFAULT_FONT_SIZE,
            cached_font_family: default_family,
            cached_font_weight: FontWeight::NORMAL,
            cached_wrap: true,
            cached_fit: None,
        }
    }

    /// Prevent text from wrapping. Text will be clipped by parent container.
    /// Use this for text inside animated containers to prevent re-wrapping during animation.
    ///
    /// The shorthand for [`wrap(false)`](Self::wrap), which is the common case
    /// and the one worth a name of its own.
    pub fn nowrap(self) -> Self {
        self.wrap(false)
    }

    /// Whether the text wraps at the width it is given.
    ///
    /// Wrapping is a layout decision — an unwrapped text is measured with no
    /// maximum width and clipped by whatever contains it — so a write here
    /// re-measures rather than merely repainting.
    pub fn wrap<M>(mut self, wrap: impl IntoSignal<bool, M>) -> Self {
        self.wrap = wrap.into_prop();
        self
    }

    /// The most lines the text may take; `None` is as many as it needs.
    ///
    /// A text longer than that is measured as the lines it draws, not as the
    /// content it holds, and [`overflow`](Self::overflow) says how the cut is
    /// marked. A limit of `0` is read as `1`: a text shows at least one line.
    ///
    /// ```no_run
    /// # use guido::prelude::*;
    /// # let track = create_signal(String::from("Something long"));
    /// text(track).max_lines(2).overflow(TextOverflow::Ellipsis);
    /// ```
    ///
    /// A layout decision like [`wrap`](Self::wrap), so a write re-measures.
    pub fn max_lines<M>(mut self, lines: impl IntoSignal<Option<u32>, M>) -> Self {
        self.max_lines = lines.into_prop();
        self
    }

    /// How a text cut short is marked. [`TextOverflow::Clip`] by default.
    ///
    /// It marks the cut [`max_lines`](Self::max_lines) makes, and the one a
    /// [`nowrap`](Self::nowrap) text meets at the edge of its box: there an
    /// ellipsis needs no limit, and each line that runs past the box is
    /// marked on its own.
    pub fn overflow<M>(mut self, overflow: impl IntoSignal<TextOverflow, M>) -> Self {
        self.overflow = overflow.into_prop();
        self
    }

    /// Blur what is behind the glyphs, so the text reads as frosted glass.
    ///
    /// The letters become the window: what they cover is drawn blurred, and the
    /// text's own colour is the tint laid over it — which is why this is usually
    /// paired with a translucent one.
    ///
    /// ```no_run
    /// # use guido::prelude::*;
    /// text("09:41")
    ///     .font_size(76.0)
    ///     .color(Color::rgba(1.0, 1.0, 1.0, 0.35))
    ///     .backdrop_blur(16.0);
    /// ```
    ///
    /// It filters what *this surface* has already drawn — a wallpaper, a photo,
    /// the panel underneath — and only that. A container's
    /// [`backdrop_blur`](super::Container::backdrop_blur) can also reach the
    /// desktop behind the surface, through `ext-background-effect-v1`; that
    /// protocol takes a region, and regions are rectangles, so glyphs cannot be
    /// expressed in it.
    ///
    /// Asked for one text at a time, on purpose: each frosted text ends the
    /// render pass to filter the target. A transform keeps it: the coverage is
    /// cut in the text's own space, so the frost turns and stretches with the
    /// letters rather than switching off. It is rasterized finer under one, so
    /// a transformed text runs into the size ceiling sooner — past it the frost
    /// is dropped with a warning in the log, as it always was.
    ///
    /// Legibility is not what it buys on its own, and of the two decorations
    /// that give it, one composes and one does not.
    ///
    /// A [`text_stroke`](Self::text_stroke) does: over frost it is
    /// drawn as a true contour — dilated from the same coverage mask, laid
    /// outside the letter — rather than as copies of the glyphs under the fill.
    /// A [`text_shadow`](Self::text_shadow) does not: it is still
    /// copies, so it covers the letter's own area as well as its edge, which is
    /// invisible under an opaque fill and an opaque letter over glass.
    pub fn backdrop_blur<M>(mut self, radius: impl IntoSignal<f32, M>) -> Self {
        self.backdrop_blur = radius.into_prop();
        self
    }

    /// Whether an override applies. Reading the answer is what subscribes the
    /// text to the control, so it is asked only for a state it declares.
    fn is_state_active(&self, id: WidgetId, control: Option<&Control>, when: &StateWhen) -> bool {
        match (when, control) {
            (StateWhen::When(condition), _) => condition.get(),
            // Inside a unit, every state is that unit's answer, whatever the
            // state is — the whole point of `Control`.
            (_, Some(control)) => control.is_active(when),
            // No control above: this text is its own unit. It can notice the
            // pointer over its own bounds, and it can hold the focus if
            // something gave it — but it cannot be pressed, because being
            // pressed means being activated and it has nothing to activate,
            // and it cannot be disabled, because it takes no input to refuse.
            (StateWhen::Hovered, None) => self.own_hover.is_some_and(|h| h.get()),
            (StateWhen::Focused, None) => crate::reactive::focus::focus_path().contains(id),
            (StateWhen::Pressed | StateWhen::Disabled, None) => false,
        }
    }

    /// Refresh cached values from the declared style.
    ///
    /// The signals are read here, inside this widget's tracking scope, so a
    /// change to a metric this text declared re-lays-out this text and nothing
    /// else.
    ///
    /// Returns how far the decoration reaches past the glyphs, which the
    /// caller records as damage slop.
    ///
    /// The reads belong to this text because the call it is made inside does:
    /// `LayoutCtx::layout_child` opens the Layout scope around a widget's whole
    /// layout, so a change to a declared metric re-measures this text and
    /// nothing else.
    fn refresh(&mut self, tree: &Tree, id: WidgetId) -> (f32, Option<Color>) {
        let declared_color;
        let overflow = {
            let style = self.resolved_text_style(tree, id);
            self.cached_text = self.content.get();
            self.cached_font_size = style.font_size(id);
            // Only where a colour motion was declared. Reading it here
            // subscribes this text's *layout* to the colour, and a text that
            // merely paints one has no reason to re-measure when it changes —
            // paint reads it under its own scope, as it always did.
            declared_color = self.animates_text_color().then(|| style.color(id));
            self.cached_font_family = style.font_family();
            self.cached_font_weight = style.font_weight();
            self.cached_wrap = self.wrap.get_or(true);
            decoration_overflow(style.stroke(), style.shadow())
        };
        (overflow, declared_color)
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

crate::widgets::text_style::declares_text_style!(Text, style, anims);

impl Widget for Text {
    /// Move the declared motions on, and ask for the frame that carries them
    /// further.
    ///
    /// Where they are pointed is decided in `retarget_text_anims`, during
    /// layout, so there is no target to resolve here — only time to pass.
    fn advance_animations(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        self.advance_text_anims(tree, id)
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, constraints: Constraints) -> Size {
        let id = ctx.id();
        // Text widgets are never relayout boundaries
        ctx.tree().set_relayout_boundary(id, false);

        // Refresh cached values from content and declared style.
        // This reads signals and registers layout dependencies.
        let (reach, declared_color) = self.refresh(ctx.tree(), id);
        // Pointed at the freshly resolved target here, where both it and the
        // frame's instant are in hand.
        self.cached_font_size = self.retarget_text_anims(
            ctx,
            id,
            declared_color.unwrap_or(Color::WHITE),
            self.cached_font_size,
        );
        ctx.tree().set_own_paint_reach(id, reach);

        let offered = constraints
            .max_width
            .is_finite()
            .then_some(constraints.max_width);

        // A cut, when there is one. An unwrapped text's lines are cut by the
        // box already, so a mark asked of it needs no limit: each line that
        // runs past the box is marked, and none is dropped.
        let overflow = self.overflow.get_or(TextOverflow::Clip);
        let limit =
            self.max_lines.get().flatten().or_else(|| {
                (!self.cached_wrap && overflow != TextOverflow::Clip).then_some(u32::MAX)
            });
        self.cached_fit = limit.map(|lines| LineFit {
            // An unwrapped line that nothing marks is the same line at any
            // width, and a width in its fit would only miss the caches.
            width: offered.filter(|_| self.cached_wrap || overflow != TextOverflow::Clip),
            max_lines: lines.max(1),
            overflow,
            wrap: self.cached_wrap,
        });

        // An unwrapped text is measured with no maximum, so it runs on one line
        // and whatever contains it does the clipping.
        let max_width = offered.filter(|_| self.cached_wrap);

        // Measure text (TextMeasurer caches results internally)
        let measured = measure_text_full(
            &self.cached_text,
            self.cached_font_size,
            max_width,
            self.cached_font_family,
            self.cached_font_weight,
            self.cached_fit,
        );

        // A parent aligning on the baseline needs this; it comes out of the
        // same shaping pass, so reporting it is free.
        ctx.tree().set_baseline(id, measured.baseline);

        Size::new(
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
        )
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
            // A pointer with no position is over nothing — the same answer a
            // `MouseLeave` gives, and `contains_at` is where that is decided.
            Event::MouseMove { at, .. } | Event::MouseEnter { at } => tree
                .get_bounds(id)
                .is_some_and(|bounds| bounds.contains_at(*at)),
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

    fn paint(&self, ctx: &mut PaintContext) {
        let (tree, id) = (ctx.tree(), ctx.id());
        // Draw in LOCAL coordinates (0,0 is widget origin)
        // Parent Container sets position transform
        let size = tree.cached_size(id).unwrap_or_default();
        let local_bounds = Rect::new(0.0, 0.0, size.width, size.height);
        // Read inside the scope the framework opened around this call, so a
        // change on whichever ancestor supplied them repaints this text and
        // nothing else.
        let (color, stroke, shadow, blur) = {
            let style = self.resolved_text_style(tree, id);
            (
                get_animated_value(self.anims.as_ref().and_then(|a| a.color.as_ref()), || {
                    style.color(id)
                }),
                style.stroke(),
                style.shadow(),
                self.backdrop_blur.get(),
            )
        };
        // A frosted text takes its stroke as a contour instead: drawn from the
        // same coverage mask, outside the letter rather than under it, so the
        // glass keeps what the frost put in it.
        let frosted = blur.filter(|radius| *radius > 0.0).is_some();
        if let Some(radius) = blur {
            ctx.draw_text_backdrop_blur(
                &self.cached_text,
                local_bounds,
                radius,
                stroke,
                self.cached_font_size,
                self.cached_font_family,
                self.cached_font_weight,
                self.cached_fit,
            );
        }
        ctx.draw_text_decorated(
            &self.cached_text,
            local_bounds,
            color,
            self.cached_font_size,
            self.cached_font_family,
            self.cached_font_weight,
            if frosted { None } else { stroke },
            shadow,
            self.cached_fit,
        );
    }
}

/// Create a text widget
///
/// Accepts static strings, closures, or signals:
/// ```no_run
/// # use guido::prelude::*;
/// # let count = create_signal(0);
/// # let my_signal = create_signal(String::from("hi"));
/// text("Hello");  // static string
/// text(move || format!("Count: {}", count.get()));  // reactive closure
/// text(my_signal);  // reactive signal
/// ```
///
/// Styling is declared here, on the widget that draws the glyphs:
/// ```no_run
/// # use guido::prelude::*;
/// text("Hello").font_size(18.0).bold();
/// ```
pub fn text<M>(content: impl IntoSignal<String, M>) -> Text {
    Text::new(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{Animatable, Animate};
    use crate::jobs;
    use crate::jobs::JobType;
    use crate::layout::Constraints;
    use crate::reactive::create_signal;
    use crate::renderer::{DrawCommand, RenderNode};
    use crate::widgets::container;

    /// Lay out after draining queued jobs, and hand back the measured size.
    fn measured(tree: &mut Tree, root: WidgetId, width: f32) -> crate::layout::Size {
        jobs::pump_and_layout(tree, root, Constraints::new(0.0, 0.0, width, 600.0))
            .expect("the root is registered")
    }

    /// Wrapping is a declared value, so it answers to a write.
    ///
    /// `nowrap()` is the shorthand and stays; `wrap(signal)` is the property.
    /// Asserted on the measured height rather than on anything drawn, because
    /// wrapping is a layout decision — `max_width` is withheld from the
    /// measurer — and the height is what a second line adds. The text is long
    /// enough that no installed font can fit it on one line at this width,
    /// which is what keeps the assertion off the font metrics.
    #[test]
    fn wrapping_answers_to_a_signal() {
        let wrap = create_signal(true);
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            Text::new("a considerable quantity of words, far more than fit").wrap(wrap),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let wrapped = measured(&mut tree, root, 80.0);

        wrap.set(false);
        let unwrapped = measured(&mut tree, root, 80.0);

        assert!(
            unwrapped.height < wrapped.height,
            "the text kept wrapping after the signal said not to: {wrapped:?} \
             then {unwrapped:?}"
        );
        assert!(
            unwrapped.width > wrapped.width,
            "an unwrapped line has to run past the width a wrapped one fits in: \
             {wrapped:?} then {unwrapped:?}"
        );
    }

    /// A text long enough that no installed font fits it on two lines at the
    /// widths below, so the assertions stay off the font's metrics.
    const OVERLONG: &str = "a considerable quantity of words, far more than any two \
                            lines of a narrow box could hold, and then some more";

    /// Lay out one text of a known line height in a box of `width`.
    ///
    /// Twenty pixels, so a line is twenty-four: guido asks every line for
    /// 1.2 times the font size, so the line count is the height over that.
    fn laid_out(text: Text, width: f32) -> crate::layout::Size {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(text.font_size(20.0)));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        measured(&mut tree, root, width)
    }

    const LINE: f32 = 24.0;

    /// `max_lines(1)` with an ellipsis measures one line high.
    ///
    /// Measured, not drawn: the height is what the parent reserves, and a text
    /// that drew one line in a box sized for six would leave the rest empty.
    #[test]
    fn one_line_with_an_ellipsis_measures_one_line() {
        let whole = laid_out(Text::new(OVERLONG), 120.0);
        assert!(
            whole.height > LINE * 2.5,
            "the text has to wrap without a limit, or this proves nothing: {whole:?}"
        );

        let cut = laid_out(
            Text::new(OVERLONG)
                .max_lines(1)
                .overflow(TextOverflow::Ellipsis),
            120.0,
        );
        assert_eq!(cut.height, LINE, "one line is one line high: {cut:?}");
        assert!(cut.width <= 120.0, "and no wider than its box: {cut:?}");
    }

    /// `max_lines(2)` on a wrapped text measures two lines high, whatever marks
    /// the cut.
    #[test]
    fn two_lines_measure_two_lines() {
        for overflow in [TextOverflow::Clip, TextOverflow::Ellipsis] {
            let cut = laid_out(Text::new(OVERLONG).max_lines(2).overflow(overflow), 120.0);
            assert_eq!(
                cut.height,
                LINE * 2.0,
                "two lines with {overflow:?} are two lines high: {cut:?}"
            );
        }
    }

    /// Hard line breaks count toward the limit like wrapped ones do.
    ///
    /// cosmic-text limits the lines of each paragraph on its own, so three
    /// short paragraphs under `max_lines(2)` would come back three lines high
    /// if the limit were only handed to it.
    #[test]
    fn a_line_break_counts_toward_the_limit() {
        let cut = laid_out(Text::new("one\ntwo\nthree").max_lines(2), 400.0);
        assert_eq!(cut.height, LINE * 2.0, "{cut:?}");
    }

    /// An ellipsis on an unwrapped text marks each line that runs past the box
    /// and drops none: the lines a text has are its own until a limit says
    /// otherwise.
    #[test]
    fn an_unwrapped_ellipsis_keeps_every_line() {
        let cut = laid_out(
            Text::new(format!("{OVERLONG}\n{OVERLONG}"))
                .nowrap()
                .overflow(TextOverflow::Ellipsis),
            120.0,
        );
        assert_eq!(cut.height, LINE * 2.0, "{cut:?}");
        assert!(cut.width <= 120.0, "{cut:?}");
    }

    /// The limit is a declared value, so it answers to a write — and lifting
    /// it gives the text back the lines it needs.
    #[test]
    fn the_line_limit_answers_to_a_signal() {
        let lines = create_signal(Some(1u32));
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            Text::new(OVERLONG).font_size(20.0).max_lines(lines),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        assert_eq!(measured(&mut tree, root, 120.0).height, LINE);
        lines.set(Some(3));
        assert_eq!(measured(&mut tree, root, 120.0).height, LINE * 3.0);
        lines.set(None);
        assert!(measured(&mut tree, root, 120.0).height > LINE * 3.0);
    }

    /// Content that fits is measured as it always was: a limit it does not
    /// reach changes nothing.
    #[test]
    fn content_that_fits_is_unchanged() {
        let free = laid_out(Text::new("short"), 400.0);
        let limited = laid_out(
            Text::new("short")
                .max_lines(1)
                .overflow(TextOverflow::Ellipsis),
            400.0,
        );
        assert_eq!(free, limited);
    }

    /// What paint hands the renderer is the fit layout measured with, so the
    /// draw paths cut where the measurer did.
    #[test]
    fn paint_carries_the_fit_layout_measured_with() {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            Text::new(OVERLONG)
                .nowrap()
                .max_lines(1)
                .overflow(TextOverflow::Ellipsis),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        measured(&mut tree, root, 90.0);
        let mut node = RenderNode::new(root.as_u64());
        tree.paint_widget(root, &mut node);

        let fit = node.commands.iter().find_map(|cmd| match &**cmd {
            DrawCommand::Text { fit, .. } => Some(*fit),
            _ => None,
        });
        assert_eq!(
            fit,
            Some(Some(crate::renderer::LineFit {
                width: Some(90.0),
                max_lines: 1,
                overflow: TextOverflow::Ellipsis,
                wrap: false,
            }))
        );
    }

    /// Every text command a frame drew, in paint order.
    fn all_text(tree: &mut Tree, root: WidgetId, at: std::time::Instant) -> Vec<(Color, f32)> {
        tree.set_frame_instant(Some(at));
        measured(tree, root, 800.0);
        let mut node = RenderNode::new(root.as_u64());
        tree.paint_widget(root, &mut node);
        tree.set_frame_instant(None);

        fn walk(node: &RenderNode, out: &mut Vec<(Color, f32)>) {
            for cmd in &node.commands {
                if let DrawCommand::Text {
                    color, font_size, ..
                } = &**cmd
                {
                    out.push((*color, *font_size));
                }
            }
            for c in &node.children {
                walk(c, out);
            }
        }
        let mut out = Vec::new();
        walk(&node, &mut out);
        out
    }

    /// A frame at a named instant: jobs, layout, animations, paint.
    ///
    /// The instant is named because an eased colour is a function of how long
    /// it has been running, and a test that let the clock decide would assert
    /// on whatever the machine managed between two calls.
    fn frame_at(tree: &mut Tree, root: WidgetId, at: std::time::Instant) -> (Color, f32) {
        tree.set_frame_instant(Some(at));
        let out = frame(tree, root);
        tree.set_frame_instant(None);
        out
    }

    /// A declared colour carries how it moves, so it eases where a bare one
    /// snaps.
    ///
    /// The pair differs in nothing but the transition, and both are written the
    /// same signal — so the eased one being *between* the two colours a frame
    /// later is the whole claim, and the plain one being already arrived is
    /// what says the test is not merely watching a slow machine.
    #[test]
    fn a_declared_transition_eases_the_colour() {
        const FROM: Color = Color::rgb(0.0, 0.0, 0.0);
        const TO: Color = Color::rgb(1.0, 1.0, 1.0);

        let sig = create_signal(FROM);

        // Both texts in one tree: two `Tree`s number their widgets from zero,
        // so the pair would share ids and the job queue — which is keyed by id
        // — would hand one widget's animation frame to the other.
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            container()
                .child(Text::new("eased").color((move || sig.get()).transition(400.0)))
                .child(Text::new("plain").color(sig)),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let t0 = std::time::Instant::now();
        all_text(&mut tree, root, t0);

        sig.set(TO);

        // The frame the write lands on is where the ease *begins*, so it is
        // still at the old colour there. The one after it is where the claim is.
        all_text(&mut tree, root, t0 + std::time::Duration::from_millis(1));
        let drawn = all_text(&mut tree, root, t0 + std::time::Duration::from_millis(100));

        let (eased, plain) = (drawn[0].0, drawn[1].0);
        assert!(
            plain.r > 0.99,
            "a colour declared without a transition arrives on the next paint, \
             got {plain:?}"
        );
        assert!(
            eased.r > 0.01 && eased.r < 0.99,
            "a quarter of the way through a 400ms ease the colour has to be \
             between the two, got {eased:?}"
        );

        let settled = all_text(&mut tree, root, t0 + std::time::Duration::from_millis(600));
        assert!(
            settled[0].0.r > 0.99,
            "the ease never finished, got {:?}",
            settled[0].0
        );
    }

    /// A text whose colour declares where it starts.
    ///
    /// Shared by the pair below, which differ in one thing — whether a measure
    /// pass runs first — and must not drift apart in any other.
    fn a_colour_entering(from: Color, to: Color) -> (Tree, WidgetId) {
        let mut tree = Tree::new();
        let root = tree
            .register(Box::new(container().child(
                Text::new("entering").color(to.transition(400.0).entering_from(from)),
            )));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        (tree, root)
    }

    /// A text's colour appears from somewhere else too.
    ///
    /// `TextAnims::retarget` is one of the four initialisers that place a
    /// property for the first time, and it is the one furthest from the
    /// container's seed pass. It asked for the enter only once the take moved
    /// onto `AnimationState` itself; before that a text colour accepted an
    /// enter and arrived at its declared value without a word.
    #[test]
    fn a_text_colour_can_appear_from_somewhere_else() {
        const FROM: Color = Color::rgb(0.0, 0.0, 0.0);
        const TO: Color = Color::rgb(1.0, 1.0, 1.0);

        let (mut tree, root) = a_colour_entering(FROM, TO);

        let t0 = std::time::Instant::now();
        all_text(&mut tree, root, t0);
        let midway = all_text(&mut tree, root, t0 + std::time::Duration::from_millis(100))[0].0;
        assert!(
            midway.r > 0.01 && midway.r < 0.99,
            "the colour is arriving rather than arrived, got {midway:?}"
        );

        let settled = all_text(&mut tree, root, t0 + std::time::Duration::from_millis(600))[0].0;
        assert!(settled.r > 0.99, "and it gets there, got {settled:?}");
    }

    /// And a measure pass does not spend it.
    ///
    /// `TextAnims::retarget` places a colour with `set_immediate` when no enter
    /// begins, which is the same shape the container's seed has — so a label in a
    /// popup lost its enter the same way a padding did, for the same reason and
    /// in a different file. The guard is on `set_immediate` rather than on either
    /// caller, which is what makes this one true without a second edit.
    #[test]
    fn a_measure_pass_leaves_a_text_colour_its_appearance() {
        const FROM: Color = Color::rgb(0.0, 0.0, 0.0);
        const TO: Color = Color::rgb(1.0, 1.0, 1.0);

        let (mut tree, root) = a_colour_entering(FROM, TO);

        // The popup path: a natural size before anything is mapped, and loose,
        // where the layouts after it are tight.
        let t0 = std::time::Instant::now();
        tree.set_frame_instant(Some(t0));
        let _ = jobs::pump_and_measure(&mut tree, root, Constraints::new(0.0, 0.0, 800.0, 800.0));
        tree.set_frame_instant(None);

        all_text(&mut tree, root, t0);
        let midway = all_text(&mut tree, root, t0 + std::time::Duration::from_millis(100))[0].0;
        assert!(
            midway.r > 0.01 && midway.r < 0.99,
            "the colour has to arrive rather than be arrived, as it does without \
             the measure pass, got {midway:?}"
        );
    }

    /// And not by leaving its cache behind, which a text under a box of exact
    /// size is handed the same constraints for in both passes.
    ///
    /// The container above it declares nothing that moves; it is the colour the
    /// measure placed without it appearing that keeps the layout after it from
    /// skipping the text.
    #[test]
    fn a_text_colour_under_an_exact_box_appears_after_a_measure() {
        const FROM: Color = Color::rgb(0.0, 0.0, 0.0);
        const TO: Color = Color::rgb(1.0, 1.0, 1.0);

        let mut tree = Tree::new();
        let root =
            tree.register(Box::new(container().width(200.0).height(50.0).child(
                Text::new("entering").color(TO.transition(400.0).entering_from(FROM)),
            )));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let t0 = std::time::Instant::now();
        tree.set_frame_instant(Some(t0));
        let _ = jobs::pump_and_measure(&mut tree, root, Constraints::new(200.0, 0.0, 200.0, 300.0));
        tree.set_frame_instant(None);

        all_text(&mut tree, root, t0);
        let midway = all_text(&mut tree, root, t0 + std::time::Duration::from_millis(100))[0].0;
        assert!(
            midway.r > 0.01 && midway.r < 0.99,
            "the colour has to arrive rather than be arrived, got {midway:?}"
        );
    }

    /// The same for a size, which moves layout rather than paint — so the claim
    /// is what the *container* measured, not what the text drew.
    ///
    /// Inside a container on purpose. A size that eases has to reflow whatever
    /// encloses it, and `Text` is not a relayout boundary, so the wake walks up
    /// to the nearest one; asserting on a bare root text would prove the value
    /// moved and say nothing about the subtree that has to follow it.
    #[test]
    fn a_declared_transition_eases_the_font_size() {
        let size = create_signal(10.0f32);
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            container().child(Text::new("x").font_size((move || size.get()).transition(400.0))),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let t0 = std::time::Instant::now();
        tree.set_frame_instant(Some(t0));
        let from = measured(&mut tree, root, 800.0).height;
        tree.set_frame_instant(None);

        size.set(30.0);
        // The write's own frame is where the ease begins; the next shows it.
        tree.set_frame_instant(Some(t0 + std::time::Duration::from_millis(1)));
        measured(&mut tree, root, 800.0);
        tree.set_frame_instant(None);

        tree.set_frame_instant(Some(t0 + std::time::Duration::from_millis(100)));
        let midway = measured(&mut tree, root, 800.0).height;
        tree.set_frame_instant(None);

        tree.set_frame_instant(Some(t0 + std::time::Duration::from_millis(600)));
        let arrived = measured(&mut tree, root, 800.0).height;
        tree.set_frame_instant(None);

        assert!(
            midway > from + 0.5 && midway < arrived - 0.5,
            "the container has to be re-measured while the size eases: {from} \
             then {midway} then {arrived}"
        );
        assert!(
            arrived > from + 1.0,
            "and it arrives larger: {from} then {arrived}"
        );
    }

    /// A text that only *paints* a colour signal does not subscribe its layout
    /// to it.
    ///
    /// Retargeting a motion needs the resolved colour during layout, so the
    /// read has to happen in that scope — but only where there is a motion to
    /// retarget. Read unconditionally, every label in a surface re-measures
    /// when a theme colour changes, and `Text` is not a relayout boundary, so
    /// that walk reaches the enclosing subtree.
    #[test]
    fn a_colour_without_a_motion_wakes_no_layout() {
        let hue = create_signal(Color::rgb(0.0, 0.0, 0.0));
        let mut tree = Tree::new();
        let root = tree.register(Box::new(Text::new("x").color(hue)));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        frame(&mut tree, root);
        jobs::clear_pending_jobs();

        hue.set(Color::rgb(1.0, 1.0, 1.0));
        let woken = jobs::queued_job_types(root);
        assert!(
            !woken.contains(&JobType::Layout),
            "a painted colour must not re-measure the text: {woken:?}"
        );

        // And the text that *does* declare a motion is the one that pays for it.
        let moved = create_signal(Color::rgb(0.0, 0.0, 0.0));
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            Text::new("x").color((move || moved.get()).transition(400.0)),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        frame(&mut tree, root);
        jobs::clear_pending_jobs();

        moved.set(Color::rgb(1.0, 1.0, 1.0));
        let woken = jobs::queued_job_types(root);
        assert!(
            woken.contains(&JobType::Layout),
            "a declared motion has to be retargeted, which is a layout read: \
             {woken:?}"
        );
    }

    /// A motion that has never run starts where the signal is now, not where it
    /// was when the builder ran.
    ///
    /// The seed is taken with `get_untracked` at declaration time, so a write
    /// landing between construction and the first layout would otherwise make
    /// the first frame ease up from a stale value instead of simply being there.
    #[test]
    fn a_motion_seeds_rather_than_eases_on_its_first_frame() {
        let hue = create_signal(Color::rgb(0.0, 0.0, 0.0));
        let text = Text::new("x").color((move || hue.get()).transition(400.0));

        // Between the builder and the first layout, as a popup's content is
        // built one frame and laid out the next.
        hue.set(Color::rgb(1.0, 1.0, 1.0));

        let mut tree = Tree::new();
        let root = tree.register(Box::new(text));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let (first, _) = frame_at(&mut tree, root, std::time::Instant::now());
        assert!(
            first.r > 0.99,
            "the first frame has to be where the signal is, not eased up from \
             what the builder saw: got {first:?}"
        );
    }

    /// A timeline declared on a text property plays when its trigger moves.
    ///
    /// `Text` accepted a `timeline` and did nothing with it. `declares_text_style!`
    /// routes both animated properties through `container::declare`, which
    /// installs whatever motion arrived — `Motion::Play` included, as an
    /// `AnimationState` carrying the keyframes — and nothing then played it:
    /// `TextAnims::retarget` only seeded, entered, or eased toward the declared
    /// value. So the declaration compiled, the trigger fired, and the glyphs
    /// never moved (#385).
    ///
    /// A two-stop linear timeline from red to blue, asked for at half its
    /// duration, so the midpoint is the whole claim: at rest it is red, and
    /// once the trigger moves it is halfway. Both halves matter — a sequence
    /// that played *without* being asked would pass the second assertion and
    /// fail the first.
    #[test]
    fn a_declared_timeline_plays_the_colour() {
        use crate::animation::Keyframes;

        const FROM: Color = Color::rgb(1.0, 0.0, 0.0);
        const TO: Color = Color::rgb(0.0, 0.0, 1.0);

        let plays = create_signal(0u32);
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            Text::new("x").color(
                FROM.timeline(
                    Keyframes::new(200.0)
                        .at(0.0, FROM)
                        .at(1.0, TO)
                        .played_by(plays),
                ),
            ),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let t0 = std::time::Instant::now();
        let at_rest = frame_at(&mut tree, root, t0).0;
        assert!(
            at_rest.r > 0.99 && at_rest.b < 0.01,
            "a sequence waits to be asked, so before the trigger moves the \
             declared colour is what is drawn: {at_rest:?}"
        );

        plays.set(1);
        // The frame the trigger is taken on starts the sequence; the one after
        // it, halfway through, is where the midpoint is.
        frame_at(&mut tree, root, t0 + std::time::Duration::from_millis(1));
        let halfway = frame_at(&mut tree, root, t0 + std::time::Duration::from_millis(101)).0;
        assert!(
            (halfway.r - 0.5).abs() < 0.1 && (halfway.b - 0.5).abs() < 0.1,
            "halfway through a linear two-stop timeline the colour is the \
             midpoint of its stops: {halfway:?}"
        );
    }

    /// And the other animated property, which is the one that moves layout.
    ///
    /// Both rows of `declares_text_style!` route through the same `declare`
    /// and the same `retarget`, so the colour passing is most of the claim —
    /// but a size is measured rather than painted, and `retarget` returns it
    /// for the measurement rather than reading it back. A row that played its
    /// sequence and did not hand the moving value to the measure would draw
    /// the right colour and the wrong size.
    #[test]
    fn a_declared_timeline_plays_the_font_size() {
        use crate::animation::Keyframes;

        let plays = create_signal(0u32);
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            Text::new("x").font_size(
                20.0f32.timeline(
                    Keyframes::new(200.0)
                        .at(0.0, 20.0)
                        .at(1.0, 60.0)
                        .played_by(plays),
                ),
            ),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let t0 = std::time::Instant::now();
        let at_rest = frame_at(&mut tree, root, t0).1;
        assert!(
            (at_rest - 20.0).abs() < 0.5,
            "before the trigger moves the declared size is what is measured: {at_rest}"
        );

        plays.set(1);
        frame_at(&mut tree, root, t0 + std::time::Duration::from_millis(1));
        let halfway = frame_at(&mut tree, root, t0 + std::time::Duration::from_millis(101)).1;
        assert!(
            (halfway - 40.0).abs() < 4.0,
            "halfway through a linear 20-to-60 timeline the size is about 40: {halfway}"
        );
    }

    /// A transition inside its delay keeps asking to be woken, and stops asking
    /// once it has arrived.
    ///
    /// The delay is the window where a motion is running and has produced no
    /// new value. Asking for a paint there wakes the loop every frame for a
    /// picture that has not changed; asking for *nothing at all* strands the
    /// ease, because the frame that would carry it never comes. So it asks to
    /// be woken without asking for a picture.
    ///
    /// Note what this test must not do: clear the pending jobs mid-flight. The
    /// queued Animation job *is* the chain — `Text` takes an
    /// unchanged-constraints early-out, so a cleared queue means no layout, no
    /// retarget and no advance, and the ease stops for good. The first draft of
    /// this test did exactly that and read the result as a defect.
    #[test]
    fn a_delayed_transition_asks_to_be_woken_without_asking_for_a_picture() {
        use crate::animation::{TimingFunction, Transition};

        let hue = create_signal(Color::rgb(0.0, 0.0, 0.0));
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            Text::new("x").color(
                (move || hue.get())
                    .transition(Transition::new(100.0, TimingFunction::Linear).delay(200.0)),
            ),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let t0 = std::time::Instant::now();
        frame_at(&mut tree, root, t0);
        hue.set(Color::rgb(1.0, 1.0, 1.0));
        frame_at(&mut tree, root, t0 + std::time::Duration::from_millis(1));

        // Inside the delay: nothing drawn has changed, and it still has to come
        // back for the frames that will carry it.
        let inside = frame_at(&mut tree, root, t0 + std::time::Duration::from_millis(50)).0;
        assert!(
            inside.r < 0.01,
            "the delay has not elapsed, so the colour must not have moved: {inside:?}"
        );
        assert!(
            jobs::queued_job_types(root).contains(&JobType::Animation),
            "a motion inside its delay has to ask to be woken: {:?}",
            jobs::queued_job_types(root)
        );

        // Past the delay and the duration, it has arrived.
        let arrived = frame_at(&mut tree, root, t0 + std::time::Duration::from_millis(400)).0;
        assert!(
            arrived.r > 0.99,
            "a delayed ease still has to run once the delay is up: {arrived:?}"
        );

        // And then it stops asking, or a still surface never idles. No clearing
        // here: an empty queue would mean no job to process, so nothing would
        // ask for another frame either way and the assertion would hold however
        // the code behaved.
        frame_at(&mut tree, root, t0 + std::time::Duration::from_millis(500));
        frame_at(&mut tree, root, t0 + std::time::Duration::from_millis(600));
        assert!(
            !jobs::queued_job_types(root).contains(&JobType::Animation),
            "a settled motion has to stop asking for frames: {:?}",
            jobs::queued_job_types(root)
        );
    }

    /// A font-size motion does not drag the colour into the layout scope with
    /// it.
    ///
    /// The guard is per property: a text that eases its size still paints its
    /// colour, so a colour write must not re-measure it. Read off the jobs,
    /// because the widget has an animation box either way — which is the case
    /// the `is_some_and` on the box cannot distinguish on its own.
    #[test]
    fn easing_a_size_does_not_make_the_colour_a_layout_read() {
        let hue = create_signal(Color::rgb(0.0, 0.0, 0.0));
        let size = create_signal(10.0f32);
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            Text::new("x")
                .font_size((move || size.get()).transition(400.0))
                .color(hue),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        frame(&mut tree, root);
        jobs::clear_pending_jobs();

        hue.set(Color::rgb(1.0, 1.0, 1.0));
        let woken = jobs::queued_job_types(root);
        assert!(
            !woken.contains(&JobType::Layout),
            "only the size was declared with a motion, so the colour is still a \
             paint read: {woken:?}"
        );
    }

    /// A state override reaches every text property, not only the colour.
    ///
    /// `TextStyle`'s setters are the override half of the vocabulary, and each
    /// one is a separate two-line body — `cargo mutants` emptied five of the
    /// six and nothing objected, because the only override anyone had tested
    /// was a colour. Asserted on the size, which is the one an override can
    /// move that shows up in what was measured.
    #[test]
    fn a_state_override_reaches_more_than_the_colour() {
        let hot = create_signal(true);
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            Text::new("x")
                .font_size(10.0)
                .state(hot, |s: TextStyle| s.font_size(30.0)),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let (_, overridden) = frame(&mut tree, root);
        assert!(
            (overridden - 30.0).abs() < 0.01,
            "the override supplies the size while it is active, got {overridden}"
        );

        hot.set(false);
        let (_, declared) = frame(&mut tree, root);
        assert!(
            (declared - 10.0).abs() < 0.01,
            "and the declaration comes back when it is not, got {declared}"
        );
    }

    /// An override nobody can compute is passed over, and the declaration it
    /// displaced is what the text uses.
    ///
    /// `Container` answers the same question this way, at `resolve_state_value`:
    /// a layer whose value is not finite is passed over exactly as one that says
    /// nothing about the property is. What makes it possible here is that the
    /// walk keeps a chain — a fold that picked a winner and put the door on
    /// whatever it picked would have dropped this text's own 32 before the bad
    /// override was ever looked at, and landed on 14.
    #[test]
    fn an_override_that_is_not_a_number_falls_through_to_the_declaration() {
        let hot = create_signal(true);
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            Text::new("x")
                .font_size(32.0)
                .color(Color::RED)
                .state(hot, |s: TextStyle| {
                    s.font_size(f32::NAN)
                        .color(Color::rgba(f32::NAN, 0.0, 0.0, 1.0))
                }),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        #[cfg(debug_assertions)]
        let before = crate::reactive::diagnostics::report_count();
        let (color, size) = frame(&mut tree, root);
        assert!(
            (size - 32.0).abs() < 0.01,
            "a bad override falls through to the size the text declares, not to \
             the default: got {size}"
        );
        assert_eq!(color, Color::RED, "and to the colour it declares");

        // And says nothing about it, as a container passes a layer over in
        // silence: what the text is left following is exactly what it declared,
        // so there is nothing to tell anybody. The diagnostic belongs to a
        // declaration that is itself wrong — see
        // `a_bad_declaration_is_reported_even_under_a_good_override`.
        #[cfg(debug_assertions)]
        assert_eq!(
            crate::reactive::diagnostics::report_count() - before,
            0,
            "a passed-over override must not be reported"
        );
    }

    /// A declaration the application got wrong is reported even when an override
    /// is covering it.
    ///
    /// This is why the base is resolved apart from the overrides rather than as
    /// the last link of one chain. An override that is not a number is passed over
    /// in silence, because the value the widget is left following is fine — but
    /// when the *declaration* is the broken one, silence loses the only message
    /// this has to give. A state active from the first frame would bury it for the
    /// life of the process, since the report dedupes per widget and property.
    #[cfg(debug_assertions)]
    #[test]
    fn a_bad_declaration_is_reported_even_under_a_good_override() {
        use crate::reactive::diagnostics::report_count;

        let hot = create_signal(true);
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            Text::new("x")
                .font_size(f32::NAN)
                .state(hot, |s: TextStyle| s.font_size(20.0)),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let before = report_count();
        let (_, size) = frame(&mut tree, root);
        assert!(
            (size - 20.0).abs() < 0.01,
            "the override is what the text draws with, got {size}"
        );
        assert_eq!(
            report_count() - before,
            1,
            "and the declaration underneath it was still wrong"
        );
    }

    /// An override's colour must not wake a layout, for the same reason a plain
    /// one must not.
    ///
    /// This is the test that decides where a declaration is *read* — see
    /// `ResolvedTextStyle`, which is where the reasoning lives. It fails against
    /// the shorter fix, the one that tests each candidate as it walks.
    #[test]
    fn an_override_colour_wakes_no_layout_either() {
        let hot = create_signal(true);
        let hue = create_signal(Color::RED);
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            Text::new("x").state(hot, move |s: TextStyle| s.color(hue)),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        frame(&mut tree, root);
        jobs::clear_pending_jobs();

        hue.set(Color::BLUE);
        let woken = jobs::queued_job_types(root);
        assert!(
            !woken.contains(&JobType::Layout),
            "an override's colour must not re-measure the text: {woken:?}"
        );
        assert!(
            woken.contains(&JobType::Paint),
            "and it does have to repaint it: {woken:?}"
        );
    }

    /// Every override setter, not just the one an earlier test happened to
    /// reach.
    ///
    /// `TextStyle`'s setters are six separate two-line bodies, and each can be
    /// emptied on its own — `cargo mutants` did exactly that to four of them
    /// after a first pass covered only the size. The family and the weight are
    /// read off the painted command; the stroke and the shadow off the reach
    /// they publish, which is the only place a decoration shows up in something
    /// a test can name.
    #[test]
    fn every_override_setter_supplies_its_property() {
        let hot = create_signal(true);
        let mut tree = Tree::new();
        let root = tree.register(Box::new(Text::new("x").state(hot, |s: TextStyle| {
            s.font_family(FontFamily::Monospace)
                .font_weight(FontWeight::BOLD)
                .text_stroke(TextStroke::new(3.0, Color::WHITE))
                .text_shadow(TextShadow::new(0.0, 0.0, 9.0, Color::WHITE))
        })));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let styled = painted_style(&mut tree, root);
        assert_eq!(
            styled.0,
            FontFamily::Monospace,
            "the family override applies"
        );
        assert_eq!(styled.1, FontWeight::BOLD, "and the weight");
        // A stroke and a shadow reach paint as the slop the text publishes,
        // never as a field of the command, so that is where they are asked
        // about.
        let decorated = tree.paint_overflow(root);
        assert!(
            decorated >= 3.0,
            "the stroke and shadow overrides have to reach the published reach, \
             got {decorated}"
        );

        hot.set(false);
        let plain = painted_style(&mut tree, root);
        assert_ne!(
            plain.0,
            FontFamily::Monospace,
            "and all of them come back off"
        );
        assert_ne!(plain.1, FontWeight::BOLD);
        assert!(
            tree.paint_overflow(root) < decorated,
            "including the two only the reach can see"
        );
    }

    /// The family and weight of the first painted text command.
    fn painted_style(tree: &mut Tree, root: WidgetId) -> (FontFamily, FontWeight) {
        measured(tree, root, 800.0);
        let mut node = RenderNode::new(root.as_u64());
        tree.paint_widget(root, &mut node);
        fn find(node: &RenderNode) -> Option<(FontFamily, FontWeight)> {
            for cmd in &node.commands {
                if let DrawCommand::Text {
                    font_family,
                    font_weight,
                    ..
                } = &**cmd
                {
                    return Some((*font_family, *font_weight));
                }
            }
            node.children.iter().find_map(|c| find(c))
        }
        find(&node).expect("a text command")
    }

    /// Lay out and paint, returning the first text command's colour and size.
    ///
    /// The queued jobs are processed first, which is what turns a signal write
    /// into `needs_layout` on the widgets that read it — and, because
    /// `mark_needs_layout` walks to the relayout boundary, on the ancestors
    /// that have to descend to reach them. Skip it and every container takes
    /// its unchanged-constraints early-out, so the text is never asked to lay
    /// out again and the test measures the cache instead of the resolution.
    fn frame(tree: &mut Tree, root: WidgetId) -> (Color, f32) {
        measured(tree, root, 800.0);
        let mut node = RenderNode::new(root.as_u64());
        tree.paint_widget(root, &mut node);

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

    /// The stroke carried by the paint's frost command, if there is one.
    fn frost_stroke(widget: impl Widget + 'static) -> Option<TextStroke> {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(widget));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        tree.layout_widget(root, Constraints::new(0.0, 0.0, 800.0, 600.0));
        let mut node = RenderNode::new(root.as_u64());
        tree.paint_widget(root, &mut node);
        node.commands.iter().find_map(|cmd| match &**cmd {
            DrawCommand::TextBackdropBlur { stroke, .. } => *stroke,
            _ => None,
        })
    }

    /// Every command a paint produced, in order, as short names.
    fn commands(widget: impl Widget + 'static) -> Vec<&'static str> {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(widget));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        tree.layout_widget(root, Constraints::new(0.0, 0.0, 800.0, 600.0));
        let mut node = RenderNode::new(root.as_u64());
        tree.paint_widget(root, &mut node);
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
        let shadowed = commands(text("hi").backdrop_blur(8.0).text_shadow(TextShadow::new(
            0.0,
            2.0,
            4.0,
            Color::BLACK,
        )));
        assert_eq!(shadowed[0], "frost");
        assert!(
            shadowed[1..].iter().all(|cmd| *cmd == "text"),
            "a shadow is more glyphs, and they go over the frost too: {shadowed:?}"
        );
    }

    /// The stroke of a frosted text is not more glyphs. Copies under the fill
    /// ring the letter *and* fill it, which over frost is an opaque letter
    /// where the picture should be — so it rides on the frost command instead
    /// and is drawn from the same coverage mask.
    #[test]
    fn a_frosted_text_takes_its_stroke_as_a_contour() {
        let stroke = TextStroke::new(2.0, Color::BLACK);
        assert_eq!(
            commands(text("hi").backdrop_blur(8.0).text_stroke(stroke)),
            vec!["frost", "text"],
            "one frost, one fill, and no copies in between"
        );

        assert_eq!(
            frost_stroke(text("hi").backdrop_blur(8.0).text_stroke(stroke)).map(|s| s.width),
            Some(2.0)
        );
        assert!(
            frost_stroke(text("hi").backdrop_blur(8.0)).is_none(),
            "and nothing is invented for a text that declared none"
        );
    }

    /// Without frost the cheap stroke is still the right one: under an opaque
    /// fill the copies are invisible, and they cost no rasterization.
    #[test]
    fn an_unfrosted_text_keeps_the_sampled_stroke() {
        let drawn = commands(text("hi").text_stroke(TextStroke::new(1.0, Color::BLACK)));
        assert!(
            drawn.len() > 2 && drawn.iter().all(|cmd| *cmd == "text"),
            "copies of the glyphs, no frost command: {drawn:?}"
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
            tree.layout_widget(root, Constraints::new(0.0, 0.0, 800.0, 600.0));
            let mut node = RenderNode::new(root.as_u64());
            tree.paint_widget(root, &mut node);
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

    /// A declaration on the text is the nearest one there is, per property:
    /// The declaration is reactive: the signal is read where the style is
    /// resolved, inside the text's own scope.
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

    /// A shadow that resolves to nothing visible must not expand into a ring of
    /// text copies. Same gate the stroke has, and the same one the container's
    /// shadow got — a transparent shadow is spellable deliberately, and in
    /// passing while an animated colour leaves transparent.
    #[test]
    fn a_fully_transparent_text_shadow_draws_nothing() {
        let invisible = TextShadow::new(2.0, 2.0, 4.0, Color::TRANSPARENT);
        assert_eq!(
            commands(text("hi").text_shadow(invisible)),
            vec!["text"],
            "the glyphs, and no copies behind them"
        );

        let visible = TextShadow::new(2.0, 2.0, 4.0, Color::rgba(0.0, 0.0, 0.0, 0.5));
        assert!(
            commands(text("hi").text_shadow(visible)).len() > 1,
            "and a shadow that can be seen still draws"
        );
    }

    // -----------------------------------------------------------------------
    // A signal that stops being a number
    // -----------------------------------------------------------------------

    /// A text through the frames a recovery claim needs: a bad number, then a
    /// finite one, then long enough for any transition to have arrived.
    ///
    /// The pair is handed back rather than asserted on, so each property reads
    /// its own half, and it is an `Option` because a text measured at NaN has no
    /// bounds to paint into — the glyphs do not merely come out the wrong size,
    /// they are gone.
    fn after_a_bad_number(
        widget: impl Widget + 'static,
        poison: impl FnOnce(),
        recover: impl FnOnce(),
    ) -> Option<(Color, f32)> {
        let ms = std::time::Duration::from_millis;
        let mut tree = Tree::new();
        let root = tree.register(Box::new(container().child(widget)));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let t0 = std::time::Instant::now();
        all_text(&mut tree, root, t0);

        poison();
        // Two frames, so a bad number is not merely the target but through
        // `lerp` into the displayed value — which `begin_segment` then keeps as
        // the start of every segment after it.
        all_text(&mut tree, root, t0 + ms(1));
        all_text(&mut tree, root, t0 + ms(50));

        recover();
        all_text(&mut tree, root, t0 + ms(100));
        all_text(&mut tree, root, t0 + ms(1000)).first().copied()
    }

    /// The size a text draws with is the one its signal asks for, even after the
    /// signal has spent a frame not being a number.
    #[test]
    fn a_font_size_follows_its_signal_again_once_the_signal_recovers() {
        let size = create_signal(10.0f32);
        let painted = after_a_bad_number(
            Text::new("x").font_size((move || size.get()).transition(200.0)),
            || size.set(f32::NAN),
            || size.set(30.0),
        )
        .map(|(_, size)| size);

        assert!(
            painted.is_some_and(|size| (size - 30.0).abs() < 0.5),
            "a size given one bad number never followed its signal again: the \
             glyphs are drawn at {painted:?} rather than 30"
        );
    }

    /// The size's neighbour, and the same defect in the same place.
    ///
    /// `TextAnims` holds exactly two motions, and both are pointed at a value
    /// read out of the same resolved style — so a door on one of them is not the
    /// rule the door exists to state.
    #[test]
    fn a_text_colour_follows_its_signal_again_once_the_signal_recovers() {
        let hue = create_signal(Color::rgb(0.0, 0.0, 0.0));
        let painted = after_a_bad_number(
            Text::new("x").color((move || hue.get()).transition(200.0)),
            || hue.set(Color::rgba(f32::NAN, 0.0, 0.0, 1.0)),
            // Red, and not white: white is what the door hands out for a colour
            // nobody declared, so recovering to it would pass whether the signal
            // was followed or merely fallen back on.
            || hue.set(Color::rgb(1.0, 0.0, 0.0)),
        )
        .map(|(color, _)| color);

        assert!(
            painted.is_some_and(|color| color.r > 0.99 && color.g < 0.01),
            "a colour given one bad number never followed its signal again: \
             got {painted:?}"
        );
    }

    /// A colour with no motion is resolved at paint and nowhere else, so that
    /// read is a second way past the door.
    ///
    /// Nothing downstream of it guards — the channels reach the shader as they
    /// are — and the tests above cannot see it: each declares a transition, which
    /// takes the animated branch of `get_animated_value` and leaves the closure
    /// beside it unwatched.
    #[test]
    fn a_colour_with_no_motion_is_painted_through_the_door_too() {
        let hue = create_signal(Color::rgba(f32::NAN, 0.0, 0.0, 1.0));
        let mut tree = Tree::new();
        let root = tree.register(Box::new(Text::new("x").color(hue)));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let (painted, _) = frame(&mut tree, root);
        assert!(
            painted.channels().iter().all(|c| c.is_finite()),
            "a colour nobody can compute reached the shader: {painted:?}"
        );
    }

    /// Both animated text properties go through the door, not the one somebody
    /// remembered.
    ///
    /// Asked of the resolver rather than of a symptom further down, for the
    /// reason the container's enumeration gives: a symptom test passes for the
    /// wrong reason all the time — the measurer already refuses a size it
    /// cannot shape — and what the door promises is about the value it hands
    /// out.
    ///
    /// *Animated*, and not every declared one, is the whole of what is claimed:
    /// `TextStyle`'s stroke and shadow are numbers too and are read raw. They
    /// are the category this issue's scope note put gradients and ripple colour
    /// in — no `AnimationState` stands behind either, so a bad number is gone
    /// from them the frame the signal recovers, and nothing keeps it.
    #[test]
    fn every_animated_text_property_is_resolved_to_a_finite_value() {
        let nan = f32::NAN;
        let declared = TextStyle::default()
            .font_size(nan)
            .color(Color::rgba(nan, 0.0, 0.0, 1.0));
        let mut style = crate::widgets::text_style::ResolvedTextStyle::default();
        style.push_own(&declared);

        // Any id: the resolvers do not consult the tree, and the diagnostic only
        // prints the number.
        let id = WidgetId::from_u64(1);

        assert!(
            style.font_size(id).is_finite(),
            "font_size resolved to a value that is not finite"
        );
        assert!(
            style.color(id).channels().iter().all(|c| c.is_finite()),
            "color resolved to a value that is not finite"
        );
    }

    /// And it says so, once.
    ///
    /// A property that quietly stops following its signal is the silence as much
    /// as the value, so the diagnostic is the third thing the fix owes. Once per
    /// property rather than once per frame, which is the dedupe in
    /// `diagnostics::non_finite`.
    ///
    /// The signal is written every frame, and that is what makes this test about
    /// the dedupe: `Text::layout` early-outs when nothing has dirtied it, so a
    /// loop that only paints resolves the size once and would pass with the
    /// dedupe deleted. NaN is equal to nothing including itself, so writing the
    /// same bad number again is always a change.
    #[cfg(debug_assertions)]
    #[test]
    fn a_font_size_that_is_not_a_number_is_reported_once() {
        use crate::reactive::diagnostics::report_count;

        let size = create_signal(f32::NAN);
        let mut tree = Tree::new();
        let root = tree.register(Box::new(Text::new("x").font_size(move || size.get())));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let before = report_count();
        for _ in 0..5 {
            size.set(f32::NAN);
            frame(&mut tree, root);
        }
        assert_eq!(
            report_count() - before,
            1,
            "five resolutions, and it should have said so once"
        );
    }
}
