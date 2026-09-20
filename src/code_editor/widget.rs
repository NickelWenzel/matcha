//! The widget itself: a multi-line editor with the full editing behaviour of
//! `iced::widget::TextEditor`.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::ops::DerefMut;

use iced::advanced::Shell;
use iced::advanced::clipboard;
use iced::advanced::graphics;
use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::text::editor::{self, Editor as _};
use iced::advanced::text::{self, highlighter, paragraph, parser};
use iced::advanced::widget::{self, Widget};
use iced::alignment;
use iced::widget::text_editor;
use iced::window;
use iced::{Element, Event, Font, Length, Padding, Pixels, Point, Rectangle, Size, Vector};

use crate::code_editor::decoration::{diagnostic, inlay};
use crate::code_editor::{Content, geometry, gutter};

/// A multi-line code editor.
///
/// It edits, selects, wraps, scrolls, and highlights exactly like
/// `iced::widget::TextEditor`, but owns its shaped text so that line numbers,
/// diagnostics, and inlay hints can be drawn against real glyph geometry.
///
/// # Example
/// ```no_run
/// # use iced::Element;
/// use matcha::{Action, Content, code_editor};
///
/// struct State {
///     content: Content,
/// }
///
/// #[derive(Debug, Clone)]
/// enum Message {
///     Edit(Action),
/// }
///
/// fn view(state: &State) -> Element<'_, Message> {
///     code_editor(&state.content)
///         .placeholder("Type something here...")
///         .on_action(Message::Edit)
///         .into()
/// }
///
/// fn update(state: &mut State, message: Message) {
///     match message {
///         Message::Edit(action) => {
///             state.content.perform(action);
///         }
///     }
/// }
/// ```
pub struct CodeEditor<'a, Parser, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Parser: text::Parser,
    Theme: text_editor::Catalog,
    Renderer: text::Renderer<Editor = graphics::text::Editor>,
{
    id: Option<widget::Id>,
    content: &'a Content,
    placeholder: Option<text::Fragment<'a>>,
    font: Option<Font>,
    text_size: Option<Pixels>,
    line_height: Option<text::LineHeight>,
    width: Length,
    height: Length,
    padding: Padding,
    wrapping: text::Wrapping,
    gutter: Option<gutter::Style>,
    diagnostics: &'a [diagnostic::Diagnostic],
    diagnostic_style: Box<dyn Fn(diagnostic::Severity) -> diagnostic::Style + 'a>,
    inlay_hints: &'a [inlay::Hint<'a>],
    // `None` until a caller asks for a look, because the default one needs the theme's
    // dimmed color and the resolved text size, and neither is known until `draw`.
    inlay_style: Option<inlay::Style>,
    class: Theme::Class<'a>,
    // A `type` alias would only move this signature somewhere the reader has to chase it; iced
    // turns the lint off for the whole workspace for the same reason.
    #[allow(clippy::type_complexity)]
    key_binding: Option<Box<dyn Fn(editor::KeyPress) -> Option<editor::Binding<Message>> + 'a>>,
    on_edit: Option<Box<dyn Fn(editor::Action) -> Message + 'a>>,
    parser_settings: Parser::Settings,
    highlighter: Option<Box<dyn text::Highlighter<Parser::Output, Theme> + 'a>>,
    last_status: Option<text_editor::Status>,
    // iced pins the renderer through `Content<Renderer>`; our `Content` is concrete, so the
    // `Editor = graphics::text::Editor` bound — the thing that makes the shaped buffer
    // reachable — has nothing else to hang on.
    renderer: PhantomData<Renderer>,
}

/// Creates a [`CodeEditor`] for the given [`Content`].
pub fn code_editor<'a, Message, Theme, Renderer>(
    content: &'a Content,
) -> CodeEditor<'a, parser::PlainText, Message, Theme, Renderer>
where
    Theme: text_editor::Catalog + 'a,
    Renderer: text::Renderer<Editor = graphics::text::Editor>,
{
    CodeEditor::new(content)
}

struct State<Parser: text::Parser, Paragraph: text::Paragraph> {
    editor: editor::State,
    parser: RefCell<Parser>,
    parser_settings: Parser::Settings,
    last_theme: RefCell<Option<String>>,
    // The gutter's width is needed in `draw`, which only ever gets a shared `&Tree`.
    widest_number: RefCell<paragraph::Plain<Paragraph>>,
    // One per hint, so a chip can be sized to the label it hides.
    labels: RefCell<Vec<paragraph::Plain<Paragraph>>>,
}

impl<'a, Message, Theme, Renderer> CodeEditor<'a, parser::PlainText, Message, Theme, Renderer>
where
    Theme: text_editor::Catalog,
    Renderer: text::Renderer<Editor = graphics::text::Editor>,
{
    /// Creates a new [`CodeEditor`] with the given [`Content`].
    pub fn new(content: &'a Content) -> Self {
        Self {
            id: None,
            content,
            placeholder: None,
            font: None,
            text_size: None,
            line_height: None,
            width: Length::Fill,
            height: Length::Fit,
            padding: Padding::new(5.0),
            wrapping: text::Wrapping::default(),
            gutter: None,
            diagnostics: &[],
            diagnostic_style: Box::new(diagnostic::Style::from),
            inlay_hints: &[],
            inlay_style: None,
            class: <Theme as text_editor::Catalog>::default(),
            key_binding: None,
            on_edit: None,
            parser_settings: (),
            highlighter: None,
            last_status: None,
            renderer: PhantomData,
        }
    }
}

impl<'a, Parser, Message, Theme, Renderer> CodeEditor<'a, Parser, Message, Theme, Renderer>
where
    Parser: text::Parser,
    Theme: text_editor::Catalog,
    Renderer: text::Renderer<Editor = graphics::text::Editor>,
{
    /// Sets the [`Id`](widget::Id) of the [`CodeEditor`].
    pub fn id(mut self, id: impl Into<widget::Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the placeholder of the [`CodeEditor`].
    pub fn placeholder(mut self, placeholder: impl text::IntoFragment<'a>) -> Self {
        self.placeholder = Some(placeholder.into_fragment());
        self
    }

    /// Sets the width of the [`CodeEditor`].
    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        self.width = Length::from(width.into());
        self
    }

    /// Sets the height of the [`CodeEditor`].
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Sets the message that should be produced when some action is performed in
    /// the [`CodeEditor`].
    ///
    /// If this method is not called, the [`CodeEditor`] will be disabled.
    pub fn on_action(mut self, on_edit: impl Fn(editor::Action) -> Message + 'a) -> Self {
        self.on_edit = Some(Box::new(on_edit));
        self
    }

    /// Sets the [`Font`] of the [`CodeEditor`].
    pub fn font(mut self, font: impl Into<Font>) -> Self {
        self.font = Some(font.into());
        self
    }

    /// Sets the text size of the [`CodeEditor`].
    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.text_size = Some(size.into());
        self
    }

    /// Sets the [`text::LineHeight`] of the [`CodeEditor`].
    pub fn line_height(mut self, line_height: impl Into<text::LineHeight>) -> Self {
        self.line_height = Some(line_height.into());
        self
    }

    /// Sets the [`Padding`] of the [`CodeEditor`].
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Sets the [`text::Wrapping`] strategy of the [`CodeEditor`].
    pub fn wrapping(mut self, wrapping: text::Wrapping) -> Self {
        self.wrapping = wrapping;
        self
    }

    /// Draws a line-number gutter to the left of the text.
    ///
    /// The gutter is absent unless this is called, and absent costs nothing:
    /// its width is then zero and every site that accounts for it is a no-op.
    pub fn gutter(mut self, style: gutter::Style) -> Self {
        self.gutter = Some(style);
        self
    }

    /// Underlines the given [`Diagnostic`](diagnostic::Diagnostic)s.
    ///
    /// The diagnostics are borrowed rather than owned or cached: they are
    /// application state that is replaced wholesale on every round-trip with
    /// whatever produces them, and the widget is rebuilt from that state every
    /// frame anyway.
    ///
    /// They are read only while drawing. Nothing the editor decides — what it
    /// shapes, where a click lands, where the caret is — depends on them.
    pub fn diagnostics(mut self, diagnostics: &'a [diagnostic::Diagnostic]) -> Self {
        self.diagnostics = diagnostics;
        self
    }

    /// Sets how each [`Severity`](diagnostic::Severity) is drawn.
    ///
    /// Defaults to [`diagnostic::Style::from`], which gives each severity the
    /// color editors have converged on.
    pub fn diagnostic_style(
        mut self,
        style: impl Fn(diagnostic::Severity) -> diagnostic::Style + 'a,
    ) -> Self {
        self.diagnostic_style = Box::new(style);
        self
    }

    /// Overlays the given [`Hint`](inlay::Hint)s on the text.
    ///
    /// The hints are borrowed for the same reason the diagnostics are: they are
    /// application state, replaced wholesale on every round-trip with whatever
    /// produces them.
    ///
    /// They are *overlays*, not text. Each label rides on an opaque chip drawn
    /// in a layer above the code, so a hint anchored inside a line hides the
    /// code there — which is what makes it legible, and why hints read best
    /// shown momentarily. This is also the control for showing them: pass
    /// `&[]`, from a toggle or a held key, and the editor draws none. A chip
    /// still reserves no room, reflows no line, moves no caret, and takes no
    /// part in hit-testing, so a click through one lands on the character
    /// underneath.
    pub fn inlay_hints(mut self, hints: &'a [inlay::Hint<'a>]) -> Self {
        self.inlay_hints = hints;
        self
    }

    /// Sets how the hints are drawn.
    ///
    /// Defaults to [`inlay::Style::new`] over the color the theme dims text to,
    /// the background the editor itself is filled with, and the editor's own
    /// text size.
    pub fn inlay_style(mut self, style: inlay::Style) -> Self {
        self.inlay_style = Some(style);
        self
    }

    /// Highlights the [`CodeEditor`] with the given [`text::Parser`] and
    /// [`text::Highlighter`].
    pub fn highlight_with<P: text::Parser>(
        self,
        settings: P::Settings,
        highlighter: impl text::Highlighter<P::Output, Theme> + 'a,
    ) -> CodeEditor<'a, P, Message, Theme, Renderer> {
        CodeEditor {
            id: self.id,
            content: self.content,
            placeholder: self.placeholder,
            font: self.font,
            text_size: self.text_size,
            line_height: self.line_height,
            width: self.width,
            height: self.height,
            padding: self.padding,
            wrapping: self.wrapping,
            gutter: self.gutter,
            diagnostics: self.diagnostics,
            diagnostic_style: self.diagnostic_style,
            inlay_hints: self.inlay_hints,
            inlay_style: self.inlay_style,
            class: self.class,
            key_binding: self.key_binding,
            on_edit: self.on_edit,
            parser_settings: settings,
            highlighter: Some(Box::new(highlighter)),
            last_status: self.last_status,
            renderer: PhantomData,
        }
    }

    /// Sets the closure to produce key bindings on key presses.
    ///
    /// See [`editor::Binding`] for the list of available bindings.
    pub fn key_binding(
        mut self,
        key_binding: impl Fn(editor::KeyPress) -> Option<editor::Binding<Message>> + 'a,
    ) -> Self {
        self.key_binding = Some(Box::new(key_binding));
        self
    }

    /// Sets the style of the [`CodeEditor`].
    #[must_use]
    pub fn style(
        mut self,
        style: impl Fn(&Theme, text_editor::Status) -> text_editor::Style + 'a,
    ) -> Self
    where
        Theme::Class<'a>: From<text_editor::StyleFn<'a, Theme>>,
    {
        self.class = (Box::new(style) as text_editor::StyleFn<'a, Theme>).into();
        self
    }

    /// The padding the editor itself sees: the widget's own, with the gutter's
    /// width folded into the left edge.
    ///
    /// `editor::State::update` derives every editor-relative coordinate it
    /// produces as `cursor - (padding.left, padding.top)`, so this one
    /// substitution is the whole of the gutter's hit-testing. It has to replace
    /// `self.padding` everywhere the editor's area is computed — mixing the two
    /// leaves the widget node narrower than its container and the text escapes
    /// the clip rect.
    fn text_padding(&self, gutter_width: f32) -> Padding {
        Padding {
            left: self.padding.left + gutter_width,
            ..self.padding
        }
    }

    /// The room the gutter takes to the left of the text, or `0.0` when there
    /// is no gutter.
    ///
    /// `line_count` is taken from the caller because `layout` and `draw` hold
    /// the contents borrowed while they need it.
    fn gutter_width(
        &self,
        line_count: usize,
        state: &State<Parser, Renderer::Paragraph>,
        renderer: &Renderer,
    ) -> f32 {
        let Some(style) = self.gutter else {
            return 0.0;
        };

        gutter::width(
            style,
            state.widest_number.borrow_mut().deref_mut(),
            line_count,
            self.gutter_text(renderer),
        )
    }

    /// The text attributes every line number is measured and drawn with.
    fn gutter_text(&self, renderer: &Renderer) -> text::Text<()> {
        let size = self.text_size.unwrap_or_else(|| renderer.text_size());
        let line_height = self.line_height.unwrap_or_else(|| renderer.line_height());

        text::Text {
            content: (),
            // A number is one row that never wraps, so only the height bounds anything —
            // and an unbounded width is what keeps measuring the gutter independent of
            // the gutter width being measured.
            bounds: Size::new(f32::INFINITY, line_height.to_absolute(size).into()),
            size,
            line_height,
            font: self.font.unwrap_or_else(|| renderer.font()),
            align_x: text::Alignment::Right,
            align_y: alignment::Vertical::Top,
            // Line numbers are ASCII digits: no fallback fonts, no clusters to resolve.
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
            ellipsis: text::Ellipsis::None,
            hint_factor: renderer.hint_factor(),
        }
    }
}

impl<Parser, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for CodeEditor<'_, Parser, Message, Theme, Renderer>
where
    Parser: text::Parser,
    Theme: text_editor::Catalog,
    Renderer: text::Renderer<Editor = graphics::text::Editor>,
{
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State<Parser, Renderer::Paragraph>>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::<Parser, Renderer::Paragraph> {
            editor: editor::State::new(),
            parser: RefCell::new(Parser::new(&self.parser_settings)),
            parser_settings: self.parser_settings.clone(),
            last_theme: RefCell::new(None),
            widest_number: RefCell::new(paragraph::Plain::default()),
            labels: RefCell::new(Vec::new()),
        })
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let mut content = self.content.0.borrow_mut();
        let state = tree
            .state
            .downcast_mut::<State<Parser, Renderer::Paragraph>>();

        if state.parser_settings != self.parser_settings {
            state.parser.borrow_mut().update(&self.parser_settings);

            state.parser_settings = self.parser_settings.clone();
        }

        // The gutter's room comes out of the text area, and the reshape that implies is
        // free: `content.update` below is handed the narrowed bounds on this very pass.
        let text_padding =
            self.text_padding(self.gutter_width(content.line_count(), state, renderer));

        let limits = limits
            .width(self.width)
            .height(self.height)
            .shrink(text_padding);

        content.update(
            limits.bounds(),
            self.font.unwrap_or_else(|| renderer.font()),
            self.text_size.unwrap_or_else(|| renderer.text_size()),
            self.line_height.unwrap_or_else(|| renderer.line_height()),
            self.wrapping,
            text::Alignment::Default,
            renderer.hint_factor(),
            state.parser.borrow_mut().deref_mut(),
        );

        let bounds = limits.resolve(self.width, self.height, content.min_bounds());

        layout::Node::new(bounds.expand(text_padding))
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let Some(on_edit) = self.on_edit.as_ref() else {
            return;
        };

        let state = tree
            .state
            .downcast_mut::<State<Parser, Renderer::Paragraph>>();
        let is_redraw = matches!(event, Event::Window(window::Event::RedrawRequested(_now)));

        let content = self.content.0.borrow();

        let text_padding =
            self.text_padding(self.gutter_width(content.line_count(), state, renderer));

        // A press left of the text origin reaches `Action::Click` as a negative x, which it
        // adds `scroll.horizontal` to: harmless while the line is unscrolled, where
        // cosmic-text clamps to the line start, but a jump to an arbitrary column once the
        // scroll exceeds the gutter. Pushing the cursor onto the origin caps it at the
        // leftmost column instead. Only a cursor already inside the widget is moved, so a
        // drag that leaves the widget still extends the selection as it did before.
        let cursor = match cursor.position_in(layout.bounds()) {
            Some(position) if position.x < text_padding.left => {
                cursor + Vector::new(text_padding.left - position.x, 0.0)
            }
            _ => cursor,
        };

        fn apply_update<Message>(
            update: editor::Update<Message>,
            shell: &mut Shell<'_, Message>,
            on_edit: &impl Fn(editor::Action) -> Message,
        ) {
            match update {
                editor::Update::Action(action) => {
                    shell.publish(on_edit(action));
                }
                editor::Update::Release => {}
                editor::Update::Custom(message) => {
                    shell.publish(message);
                }
                editor::Update::Sequence(updates) => {
                    for update in updates {
                        apply_update(update, shell, on_edit);
                    }
                }
                editor::Update::Copy(content) => {
                    shell.write_clipboard(clipboard::Content::Text(content));
                }
                editor::Update::Paste => {
                    shell.read_clipboard(clipboard::Kind::Text);
                }
                editor::Update::RedrawAt(at) => {
                    shell.request_redraw_at(at);
                }
                editor::Update::Focus | editor::Update::Unfocus | editor::Update::InputMethod => {
                    shell.request_redraw();
                }
            }
        }

        if let Some(update) = state.editor.update(
            &*content,
            event,
            layout.bounds(),
            text_padding,
            cursor,
            self.key_binding
                .as_deref()
                .unwrap_or(&editor::Binding::from_key_press as _),
        ) {
            apply_update(update, shell, on_edit);
        }

        let status = {
            let is_disabled = self.on_edit.is_none();
            let is_hovered = cursor.is_over(layout.bounds());

            if is_disabled {
                text_editor::Status::Disabled
            } else if state.editor.is_focused() {
                text_editor::Status::Focused { is_hovered }
            } else if is_hovered {
                text_editor::Status::Hovered
            } else {
                text_editor::Status::Active
            }
        };

        if is_redraw {
            self.last_status = Some(status);

            shell.request_input_method(
                &state
                    .editor
                    .input_method(&*content, layout.bounds().shrink(text_padding).position()),
            );
        } else if self
            .last_status
            .is_some_and(|last_status| status != last_status)
        {
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _defaults: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        let mut content = self.content.0.borrow_mut();
        let state = tree
            .state
            .downcast_ref::<State<Parser, Renderer::Paragraph>>();

        let font = self.font.unwrap_or_else(|| renderer.font());
        let gutter_width = self.gutter_width(content.line_count(), state, renderer);

        let theme_name = theme.name();

        // Token colors are baked into the shaped text, so a theme switch has to invalidate the
        // parser or the old colors survive until the next edit.
        if state
            .last_theme
            .borrow()
            .as_ref()
            .is_none_or(|last_theme| last_theme != theme_name)
        {
            state.parser.borrow_mut().change_line(0);
            let _ = state.last_theme.borrow_mut().replace(theme_name.to_owned());
        }

        content.highlight(font, state.parser.borrow_mut().deref_mut(), |output| {
            let Some(highlighter) = &self.highlighter else {
                return highlighter::Style::default();
            };

            highlighter.highlight(output, theme)
        });

        // `last_status` is only ever written on a redraw, and `update` bails out before reaching
        // it when the editor is disabled — so a disabled editor draws as `Active`. Mirrored from
        // iced deliberately: a `CodeEditor` and a `TextEditor` should look the same.
        let style = theme.style(
            &self.class,
            self.last_status.unwrap_or(text_editor::Status::Active),
        );

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: style.border,
                ..renderer::Quad::default()
            },
            style.background,
        );

        let text_bounds = bounds.shrink(self.text_padding(gutter_width));

        if content.is_empty()
            && let Some(placeholder) = &self.placeholder
        {
            renderer.fill_text(
                text::Text {
                    content: placeholder.clone().into_owned(),
                    bounds: text_bounds.size(),
                    size: self.text_size.unwrap_or_else(|| renderer.text_size()),
                    line_height: self.line_height.unwrap_or_else(|| renderer.line_height()),
                    font,
                    align_x: text::Alignment::Default,
                    align_y: alignment::Vertical::Top,
                    shaping: text::Shaping::Advanced,
                    wrapping: self.wrapping,
                    ellipsis: text::Ellipsis::None,
                    hint_factor: renderer.hint_factor(),
                },
                text_bounds.position(),
                style.placeholder,
                text_bounds,
            );
        }

        state.editor.draw(
            &*content,
            renderer,
            text_bounds.position(),
            *viewport,
            editor::Style {
                value: style.value,
                selection: style.selection,
            },
        );

        // Everything below reads the shaped buffer, whose coordinate space is scaled by the
        // *editor's* hint factor — not the renderer's, the one text is drawn with.
        let hint_factor = content.hint_factor().unwrap_or(1.0);

        if let Some(gutter_style) = self.gutter {
            // The rows come out of the shaped buffer, which `highlight` above has just
            // brought up to date; `layout_runs` stops at the first unshaped line, so a
            // partly shaped buffer simply numbers fewer rows.
            let text = self.gutter_text(renderer);

            gutter::draw(
                gutter_style,
                renderer,
                Rectangle {
                    x: bounds.x + self.padding.left,
                    y: text_bounds.y,
                    width: gutter_width,
                    height: text_bounds.height,
                },
                *viewport,
                geometry::visible_line_rows(content.buffer(), hint_factor),
                text,
            );
        }

        // The same clip the glyphs themselves are drawn under, so an underline is never
        // visible where its text is not — and in particular never reaches the gutter.
        let Some(clip_bounds) =
            viewport.intersection(&Rectangle::new(text_bounds.position(), content.bounds()))
        else {
            return;
        };

        let translation = text_bounds.position() - Point::ORIGIN;

        // Decorations are placed against the shaped buffer, so they can only be drawn after
        // `highlight` above: `layout_runs` stops at the first unshaped line, and shaping the
        // visible window is what `highlight` finishes. Within a layer, call order does not
        // decide z-order — both backends draw every quad in a layer before any of its
        // text — so these waves land beneath the glyphs they mark however late they are
        // issued, which is where an underline belongs. Getting *above* the glyphs takes a
        // layer of its own, which is what the hint pass below pushes one for.
        for diagnostic in self.diagnostics {
            let style = (self.diagnostic_style)(diagnostic.severity);

            for fragment in
                geometry::range_fragments(content.buffer(), hint_factor, diagnostic.range)
            {
                diagnostic::draw_squiggle(
                    renderer,
                    fragment.bounds + translation,
                    fragment.baseline + translation.y,
                    clip_bounds,
                    style,
                );
            }
        }

        let text_size = self.text_size.unwrap_or_else(|| renderer.text_size());
        let inlay_style = self
            .inlay_style
            .unwrap_or_else(|| inlay::Style::new(style.placeholder, style.background, text_size));

        // Resolved against the code's size and not the label's: a relative line height is a
        // multiple of the text it belongs to, and shrinking the label's line box with the
        // label would lift it off the row it annotates.
        let line_height = self
            .line_height
            .unwrap_or_else(|| renderer.line_height())
            .to_absolute(text_size);

        let label = text::Text {
            content: (),
            // A hint is one row that never wraps, so only the height bounds anything, and
            // matching the code's line height is what sits the label on the code's own row.
            bounds: Size::new(f32::INFINITY, line_height.into()),
            size: text_size * inlay_style.size_scale,
            line_height: text::LineHeight::Absolute(line_height),
            font,
            align_x: text::Alignment::Default,
            align_y: alignment::Vertical::Top,
            // Labels carry arrows, type names, and whatever script the thing producing them
            // writes in; `Basic` would mangle all three.
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
            ellipsis: text::Ellipsis::None,
            // The renderer's factor, not the editor's: this is text being shaped now, not a
            // measurement of text the editor already shaped.
            hint_factor: renderer.hint_factor(),
        };

        let mut labels = state.labels.borrow_mut();

        // Keyed by position rather than by content: a reordered list re-shapes a few
        // labels, which is cheap and far simpler than keying by what they say.
        labels.resize_with(self.inlay_hints.len(), Default::default);

        // Measure and anchor before placing anything. A chip is exactly as wide as the
        // text it hides, which means shaping the label first — and a hint the buffer
        // cannot place, scrolled out of view or past the end of its line, is simply
        // absent from here on.
        let mut placed: Vec<(usize, Point, Size)> = Vec::new();

        for (index, hint) in self.inlay_hints.iter().enumerate() {
            let Some(anchor) =
                geometry::position_anchor(content.buffer(), hint_factor, hint.position)
            else {
                continue;
            };

            let _ = labels[index].update(label.with_content(hint.label.as_ref()));

            placed.push((index, anchor, labels[index].min_bounds()));
        }

        // Row first, then column. Two anchors on one row are two copies of a single
        // `line_top` rather than two measurements, so comparing them exactly is sound.
        placed.sort_by(|(_, a, _), (_, b, _)| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));

        // Kept in text-origin space, like the squiggles above; `translation` is added at
        // draw time.
        let mut chips: Vec<(Rectangle, Point, usize)> = Vec::new();
        let mut row = f32::NAN;
        let mut right = f32::NEG_INFINITY;

        for (index, anchor, size) in placed {
            // `NAN` compares equal to nothing, so the first hint always opens a row.
            if anchor.y != row {
                row = anchor.y;
                right = f32::NEG_INFINITY;
            }

            // Two opaque boxes overlapping is far worse than two labels overlapping, so a
            // chip is nudged right until it clears the one before it on its row. The
            // clamp is on the box and the label is derived from it: clamping the label
            // and subtracting the padding would let each box start `padding.left` inside
            // its predecessor, which is the overlap this exists to prevent. A chip pushed
            // past the clip renders nothing, so the rule degenerates at the tail of a
            // long cascade rather than being strictly better than dropping.
            let left = (anchor.x + inlay_style.offset.x - inlay_style.padding.left).max(right);

            let chip = Rectangle {
                x: left,
                y: anchor.y + inlay_style.offset.y - inlay_style.padding.top,
                width: size.width + inlay_style.padding.x(),
                height: size.height + inlay_style.padding.y(),
            };

            right = chip.x + chip.width;

            chips.push((
                chip,
                Point::new(
                    left + inlay_style.padding.left,
                    anchor.y + inlay_style.offset.y,
                ),
                index,
            ));
        }

        // Nothing to show means no layer: an empty batch still costs one. The guard is on
        // the chips and not on the hints because a hint that resolves to no anchor
        // produces neither.
        if !chips.is_empty() {
            // Hints are opaque overlays. Nothing above this knows they exist — they
            // reserve no room, reflow nothing, and are not hit-tested — so a chip simply
            // hides the code it is anchored inside, which is what makes a label legible
            // there. It takes a layer to do it: within one layer both backends draw every
            // quad before any text, so a chip issued here would land *beneath* the code
            // and hide nothing. A new layer composites entirely above the one before it.
            // One for the whole pass, not one per hint: a layer is a separate primitive
            // batch.
            renderer.with_layer(clip_bounds, |renderer| {
                for (chip, position, index) in chips {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: chip + translation,
                            border: inlay_style.border,
                            ..renderer::Quad::default()
                        },
                        inlay_style.background,
                    );

                    renderer.fill_text(
                        label.with_content(self.inlay_hints[index].label.to_string()),
                        position + translation,
                        inlay_style.color,
                        clip_bounds,
                    );
                }
            });
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let Some(position) = cursor.position_in(layout.bounds()) else {
            return mouse::Interaction::default();
        };

        if self.on_edit.is_none() {
            return mouse::Interaction::NotAllowed;
        }

        let state = tree
            .state
            .downcast_ref::<State<Parser, Renderer::Paragraph>>();
        let text_padding =
            self.text_padding(self.gutter_width(self.content.line_count(), state, renderer));

        // An I-beam over the gutter would promise a text interaction the gutter does not
        // have — clicks there only ever reach the leftmost column.
        if self.gutter.is_some() && position.x < text_padding.left {
            mouse::Interaction::default()
        } else {
            mouse::Interaction::Text
        }
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        _viewport: &Rectangle,
        _renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        let state = tree
            .state
            .downcast_mut::<State<Parser, Renderer::Paragraph>>();

        operation.focusable(self.id.as_ref(), layout.bounds(), &mut state.editor);
        operation.text_input(
            self.id.as_ref(),
            layout.bounds(),
            &mut *self.content.0.borrow_mut(),
        );
    }
}

impl<'a, Parser, Message, Theme, Renderer> From<CodeEditor<'a, Parser, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Parser: text::Parser,
    Message: 'a,
    Theme: text_editor::Catalog + 'a,
    // iced infers this from its `&'a Content<Renderer>` field; the `PhantomData` that stands in
    // for it here carries no implied bound, so it has to be spelled out.
    Renderer: text::Renderer<Editor = graphics::text::Editor> + 'a,
{
    fn from(code_editor: CodeEditor<'a, Parser, Message, Theme, Renderer>) -> Self {
        Self::new(code_editor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeSet;
    use std::ops::Range;

    use iced::advanced::image;
    use iced::border::Radius;
    use iced::{Background, Border, Color, Point, Transformation};
    use iced_test::simulator;

    use crate::decoration::TextRange;
    use crate::{Action, Motion, Position};

    #[derive(Debug, Clone)]
    enum Message {
        Edit(Action),
    }

    const GUTTER: gutter::Style = gutter::Style {
        color: Color::BLACK,
        spacing: 8.0,
    };

    /// The text size the recorded editors below are drawn at.
    ///
    /// Written down rather than left to the renderer's default so a label's own
    /// size can be checked against a number a test chose.
    const TEXT_SIZE: f32 = 14.0;

    /// The line height the recorded editors below are drawn at, as a multiple
    /// of [`TEXT_SIZE`].
    ///
    /// Relative rather than absolute, so that a label resolving it against its
    /// own smaller size instead of the code's comes out a different box.
    const LINE_HEIGHT_SCALE: f32 = 1.5;

    /// A hint look whose every field is one no default would produce, so an
    /// assertion against it cannot pass by accident.
    const HINT: inlay::Style = inlay::Style {
        color: Color::from_rgb(1.0, 0.0, 1.0),
        background: Background::Color(Color::from_rgb(0.0, 1.0, 0.0)),
        border: Border {
            color: Color::from_rgb(0.0, 0.0, 1.0),
            width: 3.0,
            radius: Radius {
                top_left: 5.0,
                top_right: 5.0,
                bottom_right: 5.0,
                bottom_left: 5.0,
            },
        },
        size_scale: 0.5,
        offset: Vector::new(3.0, -7.0),
        padding: Padding::new(2.0),
    };

    /// The viewport the recorded draws below happen in.
    ///
    /// Tall enough that nothing a test writes is cut off the bottom, and narrow
    /// enough that a long line overruns the right edge.
    const SIZE: Size = Size::new(400.0, 600.0);

    /// The default look of an error, which is what [`drawn`] paints unless a
    /// test asks for something else.
    fn squiggle() -> diagnostic::Style {
        diagnostic::Style::from(diagnostic::Severity::Error)
    }

    /// A hint anchored at `index` of `line`.
    fn hint(line: usize, index: usize, label: &str) -> inlay::Hint<'_> {
        inlay::Hint {
            position: Position { line, index },
            label: label.into(),
        }
    }

    /// A diagnostic over `bytes` of `line`.
    fn mark(
        line: usize,
        bytes: Range<usize>,
        severity: diagnostic::Severity,
    ) -> diagnostic::Diagnostic {
        diagnostic::Diagnostic {
            range: TextRange::new(
                Position {
                    line,
                    index: bytes.start,
                },
                Position {
                    line,
                    index: bytes.end,
                },
            ),
            severity,
        }
    }

    /// Builds the widget the gutter tests drive.
    ///
    /// No padding of its own, so the gutter starts at the widget's left edge
    /// and the probes below need no magic offset; no wrapping, so a click's
    /// column is a function of its x alone and a line can scroll sideways.
    fn editor(
        content: &Content,
        gutter: Option<gutter::Style>,
    ) -> CodeEditor<'_, parser::PlainText, Message> {
        let editor = code_editor(content)
            .padding(0.0)
            .wrapping(text::Wrapping::None)
            .on_action(Message::Edit);

        match gutter {
            Some(style) => editor.gutter(style),
            None => editor,
        }
    }

    /// Lays the widget out and reports the width the editor was left with — the
    /// widget's own minus the gutter, which is the only handle a test has on
    /// the gutter's size.
    fn text_width(content: &Content, gutter: Option<gutter::Style>) -> f32 {
        let _ui = simulator(editor(content, gutter));

        content.0.borrow().bounds().width
    }

    /// Runs `events` with the cursor at `at`, applies whatever the widget
    /// published back onto `content`, and returns it.
    ///
    /// The actions are worth looking at as well as the state they produce: they
    /// carry the editor-relative point the widget resolved, which is precisely
    /// where the gutter is or is not paid for.
    fn simulate(
        content: &mut Content,
        gutter: Option<gutter::Style>,
        events: impl IntoIterator<Item = Event>,
        at: Point,
    ) -> Vec<Action> {
        // The element borrows the content, so the simulator has to be gone before the
        // resulting actions can be performed on it.
        let actions: Vec<Action> = {
            let mut ui = simulator(editor(content, gutter));

            ui.point_at(at);
            let _ = ui.simulate(events);

            ui.into_messages()
                .map(|Message::Edit(action)| action)
                .collect()
        };

        for action in &actions {
            content.perform(action.clone());
        }

        actions
    }

    #[test]
    fn typing_into_a_focused_editor_reaches_the_content() {
        let mut content = Content::new();

        // The element borrows the content, so the simulator has to be gone before the
        // resulting actions can be performed on it.
        let messages: Vec<Message> = {
            let mut ui = simulator(
                code_editor::<Message, iced::Theme, iced::Renderer>(&content)
                    .on_action(Message::Edit),
            );

            ui.point_at(Point::new(20.0, 20.0));
            let _ = ui.simulate(simulator::click());
            let _ = ui.typewrite("héllo");

            ui.into_messages().collect()
        };

        for Message::Edit(action) in messages {
            content.perform(action);
        }

        assert_eq!(content.text(), "héllo");
    }

    #[test]
    fn an_editor_without_on_action_publishes_nothing() {
        let content = Content::new();

        let mut ui = simulator(code_editor::<Message, iced::Theme, iced::Renderer>(
            &content,
        ));

        ui.point_at(Point::new(20.0, 20.0));
        let _ = ui.simulate(simulator::click());
        let _ = ui.typewrite("héllo");

        assert_eq!(ui.into_messages().count(), 0);
        assert!(content.is_empty());
    }

    #[test]
    fn a_click_lands_on_the_same_character_with_the_gutter_as_without() {
        let source = "alpha bravo charlie delta echo foxtrot golf hotel india juliett";

        let mut plain = Content::with_text(source);
        let mut numbered = Content::with_text(source);

        let gutter_width = text_width(&plain, None) - text_width(&numbered, Some(GUTTER));

        assert!(
            gutter_width > 0.0,
            "the gutter has to take room from the text"
        );

        // Every probe sits right of both text origins, so the gutter clamp cannot hide a
        // mismatch by collapsing the two clicks onto column zero.
        for x in [1.0, 46.0, 260.0] {
            let at = Point::new(x, 8.0);

            let without = simulate(&mut plain, None, simulator::click(), at);
            let with = simulate(
                &mut numbered,
                Some(GUTTER),
                simulator::click(),
                at + Vector::new(gutter_width, 0.0),
            );

            let ([Action::Click(with, _)], [Action::Click(without, _)]) =
                (with.as_slice(), without.as_slice())
            else {
                panic!("a click should publish exactly one action");
            };

            // The editor is handed the same point either way, so the gutter is paid for
            // entirely in padding. The two agree only to the rounding of the width this
            // test recovered by subtracting one `f32` from another.
            assert!(
                with.distance(*without) < 0.01,
                "a click {x} past the text origin resolved to {with:?}, not {without:?}"
            );

            // And the caret it leaves behind is on the same character.
            assert_eq!(
                numbered.cursor(),
                plain.cursor(),
                "a click {x} past the text origin"
            );
        }
    }

    #[test]
    fn the_gutter_takes_its_room_from_the_text_and_not_from_the_widget() {
        let content = Content::with_text("alpha\nbravo");

        let node = |gutter| {
            let mut ui = simulator(editor(&content, gutter).id("code-editor"));

            ui.find(iced_test::selector::id("code-editor"))
                .expect("the editor carries that id")
                .bounds()
        };

        // `layout` shrinks its limits by the gutter and has to expand the node by the
        // same amount. Expanding by the bare padding instead would leave the node that
        // much narrower than its container, and the editor's text would then run past the
        // node's right edge where `State::draw`'s clip no longer reaches it.
        assert_eq!(node(Some(GUTTER)), node(None));
    }

    #[test]
    fn the_gutter_width_follows_the_line_count_and_not_the_numbers_on_screen() {
        let source: String = (1..=200).map(|line| format!("line {line}\n")).collect();
        let mut content = Content::with_text(&source);

        let at_top = text_width(&content, Some(GUTTER));

        simulate(
            &mut content,
            Some(GUTTER),
            simulator::scroll(mouse::ScrollDelta::Lines { x: 0.0, y: -30.0 }),
            Point::new(100.0, 100.0),
        );

        assert!(
            content.0.borrow().buffer().scroll().line > 99,
            "the scroll has to reach three-digit numbers for this to test anything"
        );

        assert_eq!(text_width(&content, Some(GUTTER)), at_top);
    }

    #[test]
    fn the_gutter_widens_once_the_line_count_reaches_three_digits() {
        let lines = |count: usize| {
            (1..=count)
                .map(|line| format!("line {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let narrow = Content::with_text(&lines(99));
        let wide = Content::with_text(&lines(100));

        assert_eq!(narrow.line_count(), 99);
        assert_eq!(wide.line_count(), 100);

        assert!(
            text_width(&wide, Some(GUTTER)) < text_width(&narrow, Some(GUTTER)),
            "the hundredth line should cost the text a digit's worth of width"
        );
    }

    #[test]
    fn a_gutter_click_does_not_jump_the_caret_on_a_line_scrolled_sideways() {
        let source = (1..=120)
            .map(|line| {
                format!(
                    "{line}: alpha bravo charlie delta echo foxtrot golf hotel india juliett \
                     kilo lima mike november oscar papa quebec romeo sierra tango uniform \
                     victor whiskey xray yankee zulu"
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let gutter_width = {
            let probe = Content::with_text(&source);

            text_width(&probe, None) - text_width(&probe, Some(GUTTER))
        };

        let scrolled = || {
            let mut content = Content::with_text(&source);

            // One layout so the editor has bounds to scroll within. Nothing but caret
            // motion moves the horizontal scroll, and each motion moves it by at most the
            // caret's own width.
            let _ = text_width(&content, Some(GUTTER));

            for _ in 0..150 {
                content.perform(Action::Move(Motion::End));
            }

            content
        };

        let mut in_gutter = scrolled();
        let mut at_origin = scrolled();

        assert!(
            in_gutter.0.borrow().buffer().scroll().horizontal > gutter_width,
            "the scroll has to exceed the gutter for an unclamped click to land past the origin"
        );

        simulate(
            &mut in_gutter,
            Some(GUTTER),
            simulator::click(),
            Point::new(1.0, 8.0),
        );
        simulate(
            &mut at_origin,
            Some(GUTTER),
            simulator::click(),
            Point::new(gutter_width, 8.0),
        );

        assert_eq!(
            in_gutter.cursor(),
            at_origin.cursor(),
            "a gutter click should resolve to the leftmost column, not to one further in"
        );
    }

    #[test]
    fn a_wrapped_line_straddling_the_viewport_top_keeps_its_number_off_the_continuation_row() {
        let paragraph = ["alpha bravo charlie delta echo foxtrot golf hotel india juliett"; 8];
        let tail: String = (1..80).map(|line| format!("\nline {line}")).collect();
        let mut content = Content::with_text(&format!("{}{tail}", paragraph.join(" ")));

        // Word wrapping, unlike the rest of these, so that line 0 spans several rows.
        let messages: Vec<Message> = {
            let mut ui = simulator(
                code_editor::<Message, iced::Theme, iced::Renderer>(&content)
                    .padding(0.0)
                    .wrapping(text::Wrapping::Word)
                    .gutter(GUTTER)
                    .on_action(Message::Edit),
            );

            ui.point_at(Point::new(100.0, 100.0));
            // A quarter of a line rounds up to exactly one, which lands inside line 0's
            // rows rather than past them.
            let _ = ui.scroll(mouse::ScrollDelta::Lines { x: 0.0, y: -0.25 });

            ui.into_messages().collect()
        };

        for Message::Edit(action) in messages {
            content.perform(action);
        }

        let editor = content.0.borrow();

        {
            let first = editor
                .buffer()
                .layout_runs()
                .next()
                .expect("the buffer should still have visible rows");

            assert_eq!(first.line_i, 0, "line 0 should straddle the viewport top");
            assert_ne!(first.glyphs.iter().map(|glyph| glyph.start).min(), Some(0));
        }

        let hint_factor = editor.hint_factor().unwrap_or(1.0);
        let rows = geometry::visible_line_rows(editor.buffer(), hint_factor);

        assert!(
            rows.map(|(line, _)| line).all(|line| line != 0),
            "the continuation row must not carry line 0's number"
        );
    }

    /// A `fill_text` call, as the widget issued it.
    #[derive(Debug, Clone)]
    struct Filled {
        text: text::Text,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
        layer: usize,
    }

    /// A renderer that records what it is asked to fill and draws nothing.
    ///
    /// Squiggles are quads, a hint is a quad and a label, and a `Simulator`
    /// hands back no pixels a test can read, so the draw calls themselves are
    /// the only place to look.
    ///
    /// Every call carries the layer it landed in, because a chip drawn in the
    /// base layer paints beneath the code and hides nothing — and looks exactly
    /// like a correct one from here.
    #[derive(Debug, Default)]
    struct Probe {
        quads: Vec<(renderer::Quad, Background, usize)>,
        texts: Vec<Filled>,
        // Where the editor drew its own text. A stub would leave the code recorded
        // nowhere, and "the chip is above the code" is then inexpressible.
        editor: Vec<(Point, Color, usize)>,
        // How deep the recorder is now, and how many layers it has been asked to start.
        layer: usize,
        layers: usize,
    }

    impl Probe {
        /// The quads filled in `color`.
        ///
        /// The widget's only other quads are its background and its caret,
        /// neither of which is ever a diagnostic's color.
        fn squiggles(&self, color: Color) -> Vec<renderer::Quad> {
            self.quads
                .iter()
                .filter(|(_, background, _)| *background == Background::Color(color))
                .map(|(quad, _, _)| *quad)
                .collect()
        }

        /// The quads drawn deeper than the editor's own text, in the order they
        /// were issued.
        ///
        /// A chip's default fill is the very [`Background`] the widget's frame
        /// quad is painted with, so how deep it landed is the only thing that
        /// tells the two apart.
        fn chips(&self) -> Vec<(renderer::Quad, Background)> {
            let code = self
                .editor
                .iter()
                .map(|(_, _, layer)| *layer)
                .max()
                .expect("the editor should have drawn its own text");

            self.quads
                .iter()
                .filter(|(_, _, layer)| *layer > code)
                .map(|(quad, background, _)| (*quad, *background))
                .collect()
        }

        /// The text filled, in the order it was issued — which for hints is
        /// left to right along each row, not the order they were supplied in.
        fn labels(&self) -> Vec<&str> {
            self.texts
                .iter()
                .map(|filled| filled.text.content.as_str())
                .collect()
        }
    }

    impl renderer::Renderer for Probe {
        fn start_layer(&mut self, _bounds: Rectangle) {
            self.layer += 1;
            self.layers += 1;
        }

        fn end_layer(&mut self) {
            // A bare `-= 1` panics in debug on an unbalanced pop, which would report a
            // renderer's bug as a crash in whatever test happened to be running.
            self.layer = self.layer.saturating_sub(1);
        }

        fn start_transformation(&mut self, _transformation: Transformation) {}

        fn end_transformation(&mut self) {}

        fn fill_quad(&mut self, quad: renderer::Quad, background: impl Into<Background>) {
            self.quads.push((quad, background.into(), self.layer));
        }

        fn allocate_image(
            &self,
            _handle: &image::Handle,
            _callback: impl FnOnce(Result<image::Allocation, image::Error>) + Send + 'static,
        ) {
        }

        fn hint(&mut self, _scale: renderer::Scale) {}

        fn scale(&self) -> Option<renderer::Scale> {
            None
        }

        fn reset(&mut self, _new_bounds: Rectangle) {}

        fn settings(&self) -> renderer::Settings {
            renderer::Settings::default()
        }
    }

    impl text::Renderer for Probe {
        // The editor is the whole point: the widget's bound pins it to the graphics one, and
        // the buffer inside it is what the fragments are measured against.
        type Paragraph = graphics::text::Paragraph;
        type Editor = graphics::text::Editor;

        const ICON_FONT: Font = Font::DEFAULT;
        const CHECKMARK_ICON: char = '✓';
        const ARROW_DOWN_ICON: char = '▼';
        const SCROLL_UP_ICON: char = '^';
        const SCROLL_DOWN_ICON: char = 'v';
        const SCROLL_LEFT_ICON: char = '<';
        const SCROLL_RIGHT_ICON: char = '>';
        const ICED_LOGO: char = '*';

        fn fill_paragraph(
            &mut self,
            _text: &Self::Paragraph,
            _position: Point,
            _color: Color,
            _clip_bounds: Rectangle,
        ) {
        }

        fn fill_editor(
            &mut self,
            _editor: &Self::Editor,
            position: Point,
            color: Color,
            _clip_bounds: Rectangle,
        ) {
            self.editor.push((position, color, self.layer));
        }

        fn fill_text(
            &mut self,
            text: text::Text,
            position: Point,
            color: Color,
            clip_bounds: Rectangle,
        ) {
            self.texts.push(Filled {
                text,
                position,
                color,
                clip_bounds,
                layer: self.layer,
            });
        }
    }

    /// Lays `editor` out in a [`SIZE`] viewport and draws it, returning
    /// everything the draw recorded.
    fn record(mut editor: CodeEditor<'_, parser::PlainText, Message, iced::Theme, Probe>) -> Probe {
        let mut renderer = Probe::default();
        let mut tree = widget::Tree::new(&editor as &dyn Widget<Message, iced::Theme, Probe>);

        let node = editor.layout(&mut tree, &renderer, &layout::Limits::new(Size::ZERO, SIZE));

        editor.draw(
            &tree,
            &mut renderer,
            &iced::Theme::Light,
            &renderer::Style::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &Rectangle::with_size(SIZE),
        );

        renderer
    }

    /// [`record`]s an editor over `content` with the marks and look the caller
    /// asks for.
    ///
    /// No padding of its own, so the text origin is the widget's own corner and
    /// a recorded quad needs no offset to compare against the buffer's
    /// geometry; no wrapping, so a line stays on one row unless a test says
    /// otherwise.
    fn drawn(
        content: &Content,
        diagnostics: &[diagnostic::Diagnostic],
        style: Option<diagnostic::Style>,
    ) -> Probe {
        let editor = code_editor(content)
            .padding(0.0)
            .wrapping(text::Wrapping::None)
            .on_action(Message::Edit)
            .diagnostics(diagnostics);

        record(match style {
            Some(style) => editor.diagnostic_style(move |_severity| style),
            None => editor,
        })
    }

    /// The first visual row of each visible line of `content`, as [`drawn`]
    /// leaves the buffer.
    fn rows(content: &Content) -> Vec<(usize, f32)> {
        let editor = content.0.borrow();
        let hint_factor = editor.hint_factor().unwrap_or(1.0);

        geometry::visible_line_rows(editor.buffer(), hint_factor).collect()
    }

    #[test]
    fn a_diagnostic_on_one_line_squiggles_only_that_line() {
        let content = Content::with_text("alpha\nbravo\ncharlie\ndelta");
        let diagnostics = [mark(1, 0..5, diagnostic::Severity::Error)];

        let squiggles = drawn(&content, &diagnostics, None).squiggles(squiggle().color);
        let rows = rows(&content);

        // The regression this guards paints every *other* visible line, so the other lines
        // have to be on screen for the test to mean anything.
        assert_eq!(rows.len(), 4);
        assert!(!squiggles.is_empty());

        // Which row each quad landed on: the last row that starts at or above it.
        let lines: BTreeSet<usize> = squiggles
            .iter()
            .map(|quad| rows[rows.partition_point(|(_, top)| *top <= quad.bounds.y) - 1].0)
            .collect();

        assert_eq!(lines, BTreeSet::from([1]));
    }

    #[test]
    fn a_diagnostic_on_a_blank_line_is_visible() {
        let content = Content::with_text("alpha\n\ncharlie");
        let diagnostics = [mark(1, 0..0, diagnostic::Severity::Warning)];

        let color = diagnostic::Style::from(diagnostic::Severity::Warning).color;
        let squiggles = drawn(&content, &diagnostics, None).squiggles(color);

        assert!(!squiggles.is_empty(), "a blank line still has to be marked");

        let fragment = {
            let editor = content.0.borrow();
            let hint_factor = editor.hint_factor().unwrap_or(1.0);

            geometry::range_fragments(editor.buffer(), hint_factor, diagnostics[0].range)
                .pop()
                .expect("the blank line is on screen")
        };

        // A line with no glyphs can only be marked at its left edge, over the fallback width
        // the geometry hands back for a run that covers none of them.
        assert_eq!(fragment.bounds.x, 0.0);

        let rows = rows(&content);
        let (blank_top, next_top) = (rows[1].1, rows[2].1);

        assert!(
            squiggles.iter().all(|quad| {
                quad.bounds.x >= 0.0
                    && quad.bounds.x + quad.bounds.width <= fragment.bounds.width
                    && quad.bounds.y > blank_top
                    && quad.bounds.y < next_top
            }),
            "the mark belongs on the blank line and inside its fallback width"
        );
    }

    #[test]
    fn a_zero_width_diagnostic_is_visible() {
        let content = Content::with_text("alpha bravo");
        let diagnostics = [mark(0, 6..6, diagnostic::Severity::Hint)];

        let color = diagnostic::Style::from(diagnostic::Severity::Hint).color;
        let squiggles = drawn(&content, &diagnostics, None).squiggles(color);

        assert!(
            !squiggles.is_empty(),
            "an insert-here diagnostic still has to be marked"
        );

        // At the column it points at rather than at the left edge: the mark is useless if it
        // does not say where the insertion goes.
        let left = squiggles
            .iter()
            .map(|quad| quad.bounds.x)
            .fold(f32::INFINITY, f32::min);

        let anchor = {
            let editor = content.0.borrow();
            let hint_factor = editor.hint_factor().unwrap_or(1.0);

            geometry::position_anchor(editor.buffer(), hint_factor, Position { line: 0, index: 6 })
                .expect("column 6 of the first line is on screen")
        };

        assert!(anchor.x > 0.0, "column 6 is not the left edge");
        assert_eq!(left, anchor.x);
    }

    #[test]
    fn a_squiggle_is_a_wave_and_is_never_snapped_flat() {
        let content = Content::with_text("alpha bravo charlie");
        let diagnostics = [mark(0, 0..19, diagnostic::Severity::Error)];

        let squiggles = drawn(&content, &diagnostics, None).squiggles(squiggle().color);

        // Snapping every segment to the pixel grid would round the whole wave onto one row
        // and leave a dashed line, and `Quad::default()` does exactly that whenever iced is
        // built with `crisp` — which is one of its default features.
        assert!(squiggles.iter().all(|quad| !quad.snap));

        let tops: BTreeSet<u32> = squiggles
            .iter()
            .map(|quad| quad.bounds.y.to_bits())
            .collect();

        // A wave of amplitude 2 drawn a pixel at a time visits three heights.
        assert_eq!(tops.len(), 1 + squiggle().amplitude as usize);
    }

    #[test]
    fn a_squiggle_is_drawn_a_whole_number_of_pixels_thick() {
        let content = Content::with_text("alpha bravo");
        let diagnostics = [mark(0, 0..11, diagnostic::Severity::Error)];

        // A sub-pixel stroke gamma-blends into mud rather than thinning, so the stroke is
        // rounded up: never under a pixel, and never a fraction of one. Flattened to a
        // straight underline, where a quad's height is the stroke and nothing else.
        //
        // The stroke is also the step the wave is walked in, so a zero one would not draw a
        // hairline — it would never finish the fragment at all.
        for (thickness, drawn_as) in [(0.0, 1.0), (0.2, 1.0), (1.0, 1.0), (1.3, 2.0)] {
            let style = diagnostic::Style {
                thickness,
                amplitude: 0.0,
                ..squiggle()
            };

            let squiggles = drawn(&content, &diagnostics, Some(style)).squiggles(style.color);

            assert!(!squiggles.is_empty());
            assert!(
                squiggles.iter().all(|quad| quad.bounds.height == drawn_as),
                "a stroke of {thickness} should be drawn {drawn_as} thick"
            );
        }
    }

    #[test]
    fn a_wavelength_shorter_than_the_stroke_still_draws_a_line() {
        let content = Content::with_text("alpha bravo");
        let diagnostics = [mark(0, 0..11, diagnostic::Severity::Error)];

        // `wavelength` is a public field, so nothing stops a caller from setting it to zero.
        // That divides by zero in the wave, and a quad whose height is `NaN` is not dropped
        // by the clip — `Rectangle::intersection` compares with `f32::max`, which returns
        // the other operand — so the whole editor would be painted over instead.
        let style = diagnostic::Style {
            wavelength: 0.0,
            ..squiggle()
        };

        let squiggles = drawn(&content, &diagnostics, Some(style)).squiggles(style.color);
        let baseline = {
            let editor = content.0.borrow();
            let hint_factor = editor.hint_factor().unwrap_or(1.0);

            geometry::range_fragments(editor.buffer(), hint_factor, diagnostics[0].range)
                .pop()
                .expect("the whole line is on screen")
                .baseline
        };

        assert!(!squiggles.is_empty());
        assert!(
            squiggles.iter().all(|quad| {
                quad.bounds.y >= baseline
                    && quad.bounds.y + quad.bounds.height
                        <= baseline + 2.0 * style.thickness + style.amplitude
            }),
            "a degenerate wavelength still has to stay under its own line"
        );
    }

    #[test]
    fn a_squiggle_hangs_from_the_baseline_and_not_from_the_line_box() {
        let content = Content::with_text("alpha bravo");
        let diagnostics = [mark(0, 0..11, diagnostic::Severity::Error)];

        let drop_below_the_baseline = |line_height: f32| {
            let probe = record(
                code_editor(&content)
                    .padding(0.0)
                    .wrapping(text::Wrapping::None)
                    .line_height(text::LineHeight::Absolute(Pixels(line_height)))
                    .on_action(Message::Edit)
                    .diagnostics(&diagnostics),
            );

            let top = probe
                .squiggles(squiggle().color)
                .iter()
                .map(|quad| quad.bounds.y)
                .fold(f32::INFINITY, f32::min);

            let editor = content.0.borrow();
            let hint_factor = editor.hint_factor().unwrap_or(1.0);
            let fragment =
                geometry::range_fragments(editor.buffer(), hint_factor, diagnostics[0].range)
                    .pop()
                    .expect("the whole line is on screen");

            (top - fragment.baseline, fragment.bounds.height)
        };

        let (tight, tight_box) = drop_below_the_baseline(18.0);
        let (loose, loose_box) = drop_below_the_baseline(40.0);

        assert!(
            loose_box > tight_box,
            "the line box has to grow for this to test anything"
        );

        // Anchored to the baseline, the wave keeps its distance from the glyphs. Anchored to
        // the bottom of the line box it would have drifted by the whole difference.
        assert_eq!(tight, loose);
    }

    #[test]
    fn a_squiggle_sits_at_the_text_origin_and_not_at_the_widget_corner() {
        let content = Content::with_text("alpha bravo");
        let diagnostics = [mark(0, 0..11, diagnostic::Severity::Error)];

        let corner = |padding: f32| {
            let probe = record(
                code_editor(&content)
                    .padding(padding)
                    .wrapping(text::Wrapping::None)
                    .on_action(Message::Edit)
                    .diagnostics(&diagnostics),
            );

            let squiggles = probe.squiggles(squiggle().color);

            assert!(!squiggles.is_empty());

            squiggles
                .iter()
                .map(|quad| quad.bounds.position())
                .fold(Point::new(f32::INFINITY, f32::INFINITY), |left, at| {
                    Point::new(left.x.min(at.x), left.y.min(at.y))
                })
        };

        // The fragments are measured from the text origin, so the padding the editor is
        // inset by has to be added back before anything is drawn.
        let padded = corner(9.0);
        let flush = corner(0.0);

        assert_eq!(padded.x - flush.x, 9.0);
        assert_eq!(padded.y - flush.y, 9.0);
    }

    #[test]
    fn a_squiggle_stops_at_the_edge_of_the_text() {
        let line = "alpha bravo charlie delta echo foxtrot golf hotel india juliett kilo lima";
        let content = Content::with_text(line);
        let diagnostics = [mark(0, 0..line.len(), diagnostic::Severity::Error)];

        let squiggles = drawn(&content, &diagnostics, None).squiggles(squiggle().color);

        let width = {
            let editor = content.0.borrow();
            let hint_factor = editor.hint_factor().unwrap_or(1.0);

            geometry::range_fragments(editor.buffer(), hint_factor, diagnostics[0].range)[0]
                .bounds
                .width
        };

        assert!(
            width > SIZE.width,
            "the line has to overrun the viewport for the clip to do anything"
        );

        // Intersected against the clip rather than pushed into a layer, so nothing is drawn
        // past the text's own edge — and nothing reaches whatever sits beside it.
        assert!(!squiggles.is_empty());
        assert!(
            squiggles
                .iter()
                .all(|quad| quad.bounds.x + quad.bounds.width <= SIZE.width)
        );
    }

    #[test]
    fn an_editor_without_diagnostics_draws_exactly_what_it_drew_before() {
        let content = Content::with_text("alpha\nbravo\ncharlie");

        let bare = drawn(&content, &[], None);
        let marked = drawn(
            &content,
            &[mark(1, 0..5, diagnostic::Severity::Error)],
            None,
        );

        assert!(bare.squiggles(squiggle().color).is_empty());
        assert_eq!(
            marked.quads.len(),
            bare.quads.len() + marked.squiggles(squiggle().color).len(),
            "a diagnostic should add its own segments and nothing else"
        );
    }

    #[test]
    fn adding_diagnostics_changes_nothing_but_what_is_drawn() {
        let source = "alpha bravo charlie\ndelta echo\n\nfoxtrot";
        let diagnostics = [
            mark(0, 6..11, diagnostic::Severity::Error),
            mark(2, 0..0, diagnostic::Severity::Warning),
            mark(3, 3..3, diagnostic::Severity::Hint),
        ];

        let mut plain = Content::with_text(source);
        let mut marked = Content::with_text(source);

        // Inside the first line and well right of its origin, so the click has a character
        // to resolve to rather than a clamp at column zero.
        let at = Point::new(46.0, 8.0);

        // The element borrows the content, so the simulator has to be gone before the
        // resulting actions can be performed on it.
        let interact = |content: &Content, diagnostics: &[diagnostic::Diagnostic]| {
            let mut ui = simulator(
                editor(content, None)
                    .diagnostics(diagnostics)
                    .id("code-editor"),
            );

            ui.point_at(at);
            let _ = ui.simulate(simulator::click());
            let _ = ui.typewrite("x");

            let bounds = ui
                .find(iced_test::selector::id("code-editor"))
                .expect("the editor carries that id")
                .bounds();

            let actions: Vec<Action> = ui
                .into_messages()
                .map(|Message::Edit(action)| action)
                .collect();

            (bounds, actions)
        };

        let (plain_bounds, plain_actions) = interact(&plain, &[]);
        let (marked_bounds, marked_actions) = interact(&marked, &diagnostics);

        assert_eq!(marked_bounds, plain_bounds);
        assert_eq!(marked_actions.len(), plain_actions.len());

        for action in plain_actions {
            plain.perform(action);
        }

        for action in marked_actions {
            marked.perform(action);
        }

        assert_ne!(marked.text(), source, "the typing has to have landed");
        assert_eq!(marked.text(), plain.text());
        assert_eq!(marked.cursor(), plain.cursor());
    }

    #[test]
    fn a_screenful_of_diagnostics_stays_within_the_quad_budget() {
        // Twenty diagnostics, each over a word, is what a file being worked on looks like.
        let source: String = (0..20)
            .map(|line| format!("    let value{line} = compute(argument, other);\n"))
            .collect();

        let content = Content::with_text(&source);
        let diagnostics: Vec<diagnostic::Diagnostic> = (0..20)
            .map(|line| mark(line, 8..14 + line / 10, diagnostic::Severity::Error))
            .collect();

        let segments = drawn(&content, &diagnostics, None)
            .squiggles(squiggle().color)
            .len();

        let underlined: f32 = {
            let editor = content.0.borrow();
            let hint_factor = editor.hint_factor().unwrap_or(1.0);

            diagnostics
                .iter()
                .flat_map(|diagnostic| {
                    geometry::range_fragments(editor.buffer(), hint_factor, diagnostic.range)
                })
                .map(|fragment| fragment.bounds.width)
                .sum()
        };

        // The whole cost model: one quad per stroke-width of underlined text.
        assert!(
            (segments as f32 - underlined / squiggle().thickness).abs() <= diagnostics.len() as f32,
            "{segments} segments over {underlined} pixels of text"
        );

        // The budget the plan sets for a screenful. Coarsening the wavelength is the first
        // lever if this ever has to give.
        assert!(segments < 2000, "{segments} segments in one frame");
    }

    /// [`record`]s an editor over `content` carrying `hints`.
    ///
    /// No padding of its own, so the text origin is the widget's own corner and
    /// a recorded label needs no offset to compare against the buffer's
    /// geometry. With no gutter, and content for the placeholder to stay out
    /// of, every `fill_text` it records is a hint.
    fn overlaid(content: &Content, hints: &[inlay::Hint<'_>], wrapping: text::Wrapping) -> Probe {
        record(
            code_editor(content)
                .padding(0.0)
                .size(TEXT_SIZE)
                .line_height(text::LineHeight::Relative(LINE_HEIGHT_SCALE))
                .wrapping(wrapping)
                .on_action(Message::Edit)
                .inlay_hints(hints),
        )
    }

    #[test]
    fn hints_do_not_shift_source_text() {
        let source = "alpha bravo charlie\ndelta echo\n\nfoxtrot";
        let hints = [
            // Mid-line and left of where the click below lands, on a blank line, past the
            // end of the buffer, and long enough to run off the right edge: every shape
            // that would move a glyph if hints were virtual text rather than an overlay.
            hint(0, 6, ": usize"),
            hint(2, 0, "→ ()"),
            hint(
                3,
                7,
                "  // a label far wider than the line it is anchored to",
            ),
            hint(9, 0, ": nowhere"),
        ];

        let mut plain = Content::with_text(source);
        let mut annotated = Content::with_text(source);

        // Inside the first line and right of the first hint's anchor, so a hint that
        // reserved room for itself would put a different character under the cursor.
        let at = Point::new(46.0, 8.0);

        // The element borrows the content, so the simulator has to be gone before the
        // resulting actions can be performed on it.
        let interact = |content: &Content, hints: &[inlay::Hint<'_>]| {
            let mut ui = simulator(editor(content, None).inlay_hints(hints).id("code-editor"));

            ui.point_at(at);
            let _ = ui.simulate(simulator::click());
            let _ = ui.typewrite("x");

            let bounds = ui
                .find(iced_test::selector::id("code-editor"))
                .expect("the editor carries that id")
                .bounds();

            let actions: Vec<Action> = ui
                .into_messages()
                .map(|Message::Edit(action)| action)
                .collect();

            (bounds, actions)
        };

        let (plain_bounds, plain_actions) = interact(&plain, &[]);
        let (annotated_bounds, annotated_actions) = interact(&annotated, &hints);

        assert!(
            matches!(plain_actions.first(), Some(Action::Click(..))),
            "the click has to have landed for this to test anything"
        );

        // `Action::Click` carries the editor-relative point the widget resolved, so
        // comparing the actions compares the click-to-caret mapping itself and not only
        // the caret it happened to leave behind.
        assert_eq!(annotated_bounds, plain_bounds);
        assert_eq!(annotated_actions, plain_actions);

        for action in plain_actions {
            plain.perform(action);
        }

        for action in annotated_actions {
            annotated.perform(action);
        }

        assert_ne!(annotated.text(), source, "the typing has to have landed");
        assert_eq!(annotated.text(), plain.text());
        assert_eq!(annotated.cursor(), plain.cursor());
    }

    #[test]
    fn a_hint_is_drawn_at_its_anchor_measured_from_the_text_origin() {
        const PADDING: f32 = 9.0;

        let content = Content::with_text("alpha bravo");
        let hints = [hint(0, 6, ": usize")];

        let probe = record(
            code_editor(&content)
                .padding(PADDING)
                .size(TEXT_SIZE)
                .font(Font::MONOSPACE)
                .wrapping(text::Wrapping::None)
                .on_action(Message::Edit)
                .inlay_hints(&hints)
                .inlay_style(HINT),
        );

        let anchor = {
            let editor = content.0.borrow();
            let hint_factor = editor.hint_factor().unwrap_or(1.0);

            geometry::position_anchor(editor.buffer(), hint_factor, Position { line: 0, index: 6 })
                .expect("column 6 of the first line is on screen")
        };

        assert!(anchor.x > 0.0, "column 6 is not the left edge");

        let [label] = probe.texts.as_slice() else {
            panic!("one hint should be drawn once");
        };

        assert_eq!(label.text.content, ": usize");
        assert_eq!(label.color, HINT.color);
        assert_eq!(label.text.size, Pixels(TEXT_SIZE) * HINT.size_scale);

        // The editor's own font, so a label reads as an annotation of this code rather
        // than as text from somewhere else.
        assert_eq!(label.text.font, Font::MONOSPACE);

        // An anchor is a top-left corner, and both backends move the position by the
        // label's shaped size for every alignment but these two.
        assert_eq!(label.text.align_x, text::Alignment::Default);
        assert_eq!(label.text.align_y, alignment::Vertical::Top);

        // Anchors are measured from the text origin, so the padding the editor is inset by
        // has to be added back before anything is drawn.
        assert_eq!(
            label.position,
            anchor + Vector::new(PADDING, PADDING) + HINT.offset
        );

        // The same clip the glyphs themselves are drawn under, so a label is never visible
        // where its text is not.
        assert_eq!(
            label.clip_bounds,
            Rectangle::new(Point::new(PADDING, PADDING), content.0.borrow().bounds())
        );
    }

    #[test]
    fn a_hint_without_a_style_takes_its_look_from_the_theme_and_the_text_size() {
        let content = Content::with_text("alpha bravo");
        let hints = [hint(0, 6, ": usize")];

        let probe = overlaid(&content, &hints, text::Wrapping::None);

        let anchor = {
            let editor = content.0.borrow();
            let hint_factor = editor.hint_factor().unwrap_or(1.0);

            geometry::position_anchor(editor.buffer(), hint_factor, Position { line: 0, index: 6 })
                .expect("column 6 of the first line is on screen")
        };

        // `record` draws the light theme, and nothing has written a status yet.
        let style = text_editor::default(&iced::Theme::Light, text_editor::Status::Active);
        let expected = inlay::Style::new(style.placeholder, style.background, Pixels(TEXT_SIZE));

        let [label] = probe.texts.as_slice() else {
            panic!("one hint should be drawn once");
        };

        assert_ne!(
            expected.color, style.value,
            "a hint has to be told apart from the code it annotates"
        );

        assert_eq!(label.color, expected.color);
        assert_eq!(label.text.size, Pixels(TEXT_SIZE) * expected.size_scale);
        assert!(label.text.size < Pixels(TEXT_SIZE));
        assert_eq!(label.position, anchor + expected.offset);

        // The label's line box is the code's, so the smaller glyphs sit on the row they
        // annotate. Resolving the editor's relative line height against the label's own
        // size instead would shrink the box and lift them off it.
        assert_eq!(
            label.text.line_height,
            text::LineHeight::Absolute(Pixels(TEXT_SIZE * LINE_HEIGHT_SCALE))
        );
    }

    #[test]
    fn a_hint_is_drawn_on_an_opaque_chip_above_the_code() {
        let content = Content::with_text("alpha bravo");
        let hints = [hint(0, 6, ": usize")];

        let bare = overlaid(&content, &[], text::Wrapping::None);
        let annotated = overlaid(&content, &hints, text::Wrapping::None);

        assert!(bare.texts.is_empty());
        assert_eq!(annotated.labels(), [": usize"]);

        // The hint costs exactly one quad, which is its chip.
        assert_eq!(annotated.quads.len(), bare.quads.len() + 1);

        let [(_, _, code)] = annotated.editor.as_slice() else {
            panic!("the editor should fill its own text exactly once");
        };
        let [label] = annotated.texts.as_slice() else {
            panic!("one hint should be drawn once");
        };
        let [(chip, _, chip_layer)] = annotated.quads[bare.quads.len()..] else {
            panic!("the hint should add exactly one quad");
        };

        // Within one layer both backends draw every quad before any text, so a chip
        // issued beside the code would paint underneath it and the code would show
        // through. Nothing about the call itself says which happened — only how deep it
        // landed does.
        assert!(
            chip_layer > *code,
            "a chip in the editor's own layer paints beneath the code and hides nothing"
        );

        // The label rides on the chip rather than being left behind in the base layer,
        // where the code would paint over it in turn.
        assert_eq!(label.layer, chip_layer);

        // And it is under the label rather than merely somewhere nearby.
        assert!(chip.bounds.contains(label.position));
    }

    #[test]
    fn a_chip_covers_the_whole_row_it_annotates() {
        let content = Content::with_text("alpha bravo\ncharlie delta");
        let hints = [hint(1, 4, ": usize")];

        let probe = overlaid(&content, &hints, text::Wrapping::None);

        let chips = probe.chips();
        let [(chip, _)] = chips.as_slice() else {
            panic!("one hint should draw one chip");
        };

        let rows = rows(&content);
        let (_, top) = rows[1];
        let line_height = TEXT_SIZE * LINE_HEIGHT_SCALE;

        assert_eq!(rows.len(), 2, "both lines have to be on screen");
        assert!(top > 0.0, "the annotated row must not be the first one");

        // The whole premise: the code underneath is hidden rather than blended with. A
        // chip raised off its row leaves the descenders showing along the bottom and
        // clips the row above, which is why the default offset is level with the row and
        // the breathing room is horizontal.
        assert!(
            chip.bounds.y <= top && chip.bounds.y + chip.bounds.height >= top + line_height,
            "a chip spanning {}..{} does not cover the row at {top}..{}",
            chip.bounds.y,
            chip.bounds.y + chip.bounds.height,
            top + line_height
        );
    }

    #[test]
    fn a_chip_is_as_wide_as_the_label_it_hides() {
        let content = Content::with_text("alpha bravo");

        let chip = |text: &str, size_scale: f32, padding: Padding| {
            let hints = [hint(0, 6, text)];

            let probe = record(
                code_editor(&content)
                    .padding(0.0)
                    .size(TEXT_SIZE)
                    .font(Font::MONOSPACE)
                    .wrapping(text::Wrapping::None)
                    .on_action(Message::Edit)
                    .inlay_hints(&hints)
                    .inlay_style(inlay::Style {
                        size_scale,
                        padding,
                        ..HINT
                    }),
            );

            let chips = probe.chips();
            let [(chip, _)] = chips.as_slice() else {
                panic!("one hint should draw one chip");
            };

            chip.bounds
        };

        let short = chip(": u8", HINT.size_scale, Padding::ZERO);
        let long = chip(": a much longer annotation", HINT.size_scale, Padding::ZERO);

        // Sized to the label rather than to anything constant.
        assert!(short.width > 0.0);
        assert!(long.width > short.width);

        // Sized to the *shaped* label: doubling the size the glyphs are drawn at doubles
        // the room they take, so a chip measured against the code's size instead of the
        // label's would not move.
        let doubled = chip(
            ": a much longer annotation",
            HINT.size_scale * 2.0,
            Padding::ZERO,
        );

        assert!(
            (doubled.width - 2.0 * long.width).abs() < 0.01 * long.width,
            "a label at twice the size measured {} against {}",
            doubled.width,
            long.width
        );

        // And the padding is added on top of the measurement, once per side.
        let padded = chip(
            ": a much longer annotation",
            HINT.size_scale,
            Padding::new(3.0),
        );

        assert_eq!(padded.width, long.width + 6.0);
        assert_eq!(padded.height, long.height + 6.0);
    }

    #[test]
    fn a_chip_is_outlined_with_the_border_its_style_carries() {
        // A quad left at its default carries a transparent edge of no width, so a
        // border that never reaches the quad and a border nobody asked for look the
        // same from here unless the look under test differs from that default.
        assert_ne!(HINT.border, Border::default());

        let content = Content::with_text("alpha bravo");
        let hints = [hint(0, 6, ": usize")];

        let probe = record(
            code_editor(&content)
                .padding(0.0)
                .size(TEXT_SIZE)
                .wrapping(text::Wrapping::None)
                .on_action(Message::Edit)
                .inlay_hints(&hints)
                .inlay_style(HINT),
        );

        let chips = probe.chips();
        let [(chip, _)] = chips.as_slice() else {
            panic!("one hint should draw one chip");
        };

        // The whole border and not merely its presence: the width separates the chip
        // from code of a similar color behind it, and the radius is what keeps a chip
        // from reading as a block of the editor's own frame.
        assert_eq!(chip.border, HINT.border);
    }

    #[test]
    fn two_hints_on_one_row_do_not_overlap() {
        let content = Content::with_text("alpha bravo charlie delta");

        // Supplied right to left, so the row has to be laid out in its own order rather
        // than in the order the hints happened to arrive.
        let hints = [hint(0, 11, ": second"), hint(0, 5, ": a long first label")];

        let probe = overlaid(&content, &hints, text::Wrapping::None);

        assert_eq!(
            probe.labels(),
            [": a long first label", ": second"],
            "chips are placed left to right along the row"
        );

        let chips = probe.chips();
        let [(first, _), (second, _)] = chips.as_slice() else {
            panic!("both hints should draw a chip");
        };

        assert_eq!(
            first.bounds.y, second.bounds.y,
            "both hints have to share a row for this to test anything"
        );

        let anchor = |index| {
            let editor = content.0.borrow();
            let hint_factor = editor.hint_factor().unwrap_or(1.0);

            geometry::position_anchor(editor.buffer(), hint_factor, Position { line: 0, index })
                .expect("the first line is on screen")
        };

        // Left to right in anchor order, and the second anchor falls *inside* the first
        // chip, so the second box can only clear it by shifting.
        assert!(anchor(5).x < anchor(11).x);
        assert!(
            anchor(11).x < first.bounds.x + first.bounds.width,
            "the two chips have to collide for this to test anything"
        );

        // Not even by `padding.left`, which is what clamping the label instead of the box
        // would cost.
        assert!(
            second.bounds.x >= first.bounds.x + first.bounds.width,
            "a chip at {} overlaps the one ending at {}",
            second.bounds.x,
            first.bounds.x + first.bounds.width
        );
    }

    #[test]
    fn two_hints_on_different_rows_both_sit_at_their_anchors() {
        let content = Content::with_text("alpha bravo\ncharlie delta");
        let hints = [hint(0, 5, ": a long first label"), hint(1, 0, ": second")];

        let probe = overlaid(&content, &hints, text::Wrapping::None);

        let chips = probe.chips();
        let [(first, _), (second, _)] = chips.as_slice() else {
            panic!("both hints should draw a chip");
        };

        assert_ne!(first.bounds.y, second.bounds.y, "the hints are on two rows");

        let style = text_editor::default(&iced::Theme::Light, text_editor::Status::Active);
        let expected = inlay::Style::new(style.placeholder, style.background, Pixels(TEXT_SIZE));

        let anchor = {
            let editor = content.0.borrow();
            let hint_factor = editor.hint_factor().unwrap_or(1.0);

            geometry::position_anchor(editor.buffer(), hint_factor, Position { line: 1, index: 0 })
                .expect("the second line is on screen")
        };

        let unshifted = anchor.x + expected.offset.x - expected.padding.left;

        assert!(
            first.bounds.x + first.bounds.width > unshifted,
            "the rows have to be able to collide for this to test anything"
        );

        // The running right edge resets at every row, so a chip low on the screen is
        // never pushed aside by one above it.
        assert_eq!(second.bounds.x, unshifted);
    }

    #[test]
    fn a_hint_without_a_style_still_gets_an_opaque_chip() {
        let content = Content::with_text("alpha bravo");
        let hints = [hint(0, 6, ": usize")];

        let probe = overlaid(&content, &hints, text::Wrapping::None);

        let chips = probe.chips();
        let [(_, fill)] = chips.as_slice() else {
            panic!("one hint should draw one chip");
        };

        // `record` draws the light theme, and nothing has written a status yet.
        let style = text_editor::default(&iced::Theme::Light, text_editor::Status::Active);

        // The editor's own background, so a chip reads as a hole punched in the code
        // rather than as a badge stuck over it.
        assert_eq!(*fill, style.background);

        let Background::Color(color) = fill else {
            panic!("the default fill is a solid color");
        };

        assert_eq!(
            color.a, 1.0,
            "a chip that lets the code through hides nothing"
        );
    }

    #[test]
    fn an_editor_without_hints_pushes_no_layer_and_draws_no_chip() {
        let content = Content::with_text("alpha bravo");

        let bare = overlaid(&content, &[], text::Wrapping::None);

        assert_eq!(bare.layers, 0, "an empty hint pass must not cost a layer");
        assert!(bare.chips().is_empty());

        // A hint the buffer cannot place produces no chip either, which is why the guard
        // is on the chips rather than on the hints.
        let nowhere = overlaid(&content, &[hint(9, 0, ": nowhere")], text::Wrapping::None);

        assert_eq!(nowhere.layers, 0);
        assert!(nowhere.chips().is_empty());

        let annotated = overlaid(
            &content,
            &[hint(0, 2, ": a"), hint(0, 8, ": b")],
            text::Wrapping::None,
        );

        assert_eq!(annotated.chips().len(), 2);
        assert_eq!(
            annotated.layers, 1,
            "one layer for the whole pass, not one per hint"
        );
    }

    #[test]
    fn a_hint_scrolled_out_of_view_is_not_drawn() {
        let source: String = (0..200).map(|line| format!("line {line}\n")).collect();
        let mut content = Content::with_text(&source);

        let hints = [hint(0, 0, ": first"), hint(40, 0, ": later")];

        assert_eq!(
            overlaid(&content, &hints, text::Wrapping::None).labels(),
            [": first"],
            "line 40 starts below the viewport"
        );

        // The draw above gave the editor the bounds this scroll is clamped against.
        content.perform(Action::Scroll { lines: 30 });

        assert_eq!(
            overlaid(&content, &hints, text::Wrapping::None).labels(),
            [": later"],
            "a hint tracks its line out of view and back in again"
        );
    }

    #[test]
    fn a_hint_anchored_past_the_end_of_its_line_is_not_drawn() {
        let content = Content::with_text("alpha\nbravo");

        let hints = [
            hint(0, 5, "→ end"),
            hint(0, 40, ": past the line"),
            hint(99, 0, ": past the buffer"),
        ];

        // The end of a line is a place; two bytes further along is not, and neither is a
        // line the buffer does not have. Both resolve to no anchor and are dropped.
        let probe = overlaid(&content, &hints, text::Wrapping::None);

        assert_eq!(probe.labels(), ["→ end"]);

        // The two that never placed cost neither a chip nor a layer of their own.
        assert_eq!(probe.chips().len(), 1);
        assert_eq!(probe.layers, 1);
    }

    #[test]
    fn a_hint_at_a_wrap_boundary_anchors_to_the_previous_row() {
        // One unbroken token wrapped by glyph, because that is the only break that leaves
        // a byte belonging to both rows: a word wrap falls on a space, whose glyph is
        // dropped from both rows, so the byte that ends the first row and the byte that
        // starts the second are two different bytes and neither is ambiguous.
        let token = "alphabravocharliedeltaechofoxtrotgolfhotelindiajuliettkilolimamike";
        let content = Content::with_text(token);

        // One draw to shape the buffer, so the rows the boundary is read off exist.
        assert!(
            overlaid(&content, &[], text::Wrapping::Glyph)
                .texts
                .is_empty()
        );

        let (boundary, first_top, first_width, second_top) = {
            let editor = content.0.borrow();
            let mut runs = editor.buffer().layout_runs();

            let first = runs.next().expect("the buffer should have a first row");
            let second = runs.next().expect("the token has to wrap");

            let boundary = first
                .glyphs
                .iter()
                .map(|glyph| glyph.end)
                .max()
                .expect("the first row should have glyphs");

            assert_eq!(
                Some(boundary),
                second.glyphs.iter().map(|glyph| glyph.start).min(),
                "the boundary byte has to belong to both rows for this to test anything"
            );

            (boundary, first.line_top, first.line_w, second.line_top)
        };

        let hints = [hint(0, boundary, ": usize")];
        let probe = overlaid(&content, &hints, text::Wrapping::Glyph);

        let [label] = probe.texts.as_slice() else {
            panic!("one hint should be drawn once");
        };

        let style = text_editor::default(&iced::Theme::Light, text_editor::Status::Active);
        let offset =
            inlay::Style::new(style.placeholder, style.background, Pixels(TEXT_SIZE)).offset;

        // `Buffer::cursor_position` ignores `Cursor::affinity`, so of the two rows this
        // byte belongs to it takes the earlier — and the hint lands at the far right edge
        // of the row above the one a reader would expect. Accepted for now; whoever
        // teaches the anchor about affinity should find this test failing rather than a
        // silent shift.
        assert_eq!(label.position.y, first_top + offset.y);
        assert_ne!(label.position.y, second_top + offset.y);
        assert!((label.position.x - offset.x - first_width).abs() < 0.01);
    }

    #[test]
    fn a_non_ascii_hint_label_is_shaped_in_full_and_never_reflowed() {
        let content = Content::with_text("alpha bravo");
        let label = "→ Vec<String>, 日本語, 👋🏽";
        let hints = [hint(0, 6, label)];

        // A wrapping editor, so a label that inherited the editor's strategy instead of
        // stating its own would show it here.
        let probe = overlaid(&content, &hints, text::Wrapping::Word);

        let [drawn] = probe.texts.as_slice() else {
            panic!("one hint should be drawn once");
        };

        assert_eq!(drawn.text.content, label);

        // `Basic` resolves no clusters and falls back to no other font, which turns an
        // arrow, a CJK run, and a modified emoji into boxes.
        assert_eq!(drawn.text.shaping, text::Shaping::Advanced);

        // A hint is one row: reflowing it would push it onto the line below, and eliding
        // it would hide the type it exists to report.
        assert_eq!(drawn.text.wrapping, text::Wrapping::None);
        assert_eq!(drawn.text.ellipsis, text::Ellipsis::None);
    }
}
