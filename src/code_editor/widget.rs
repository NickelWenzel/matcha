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
use iced::{Element, Event, Font, Length, Padding, Pixels, Rectangle, Size, Vector};

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

        if let Some(gutter_style) = self.gutter {
            // The rows come out of the shaped buffer, which `highlight` above has just
            // brought up to date; `layout_runs` stops at the first unshaped line, so a
            // partly shaped buffer simply numbers fewer rows.
            let text = self.gutter_text(renderer);
            // The buffer's coordinate space is scaled by the *editor's* factor, which is
            // not the renderer's — the one the numbers themselves are drawn with.
            let hint_factor = content.hint_factor().unwrap_or(1.0);

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

    use iced::{Color, Point};
    use iced_test::simulator;

    use crate::{Action, Motion};

    #[derive(Debug, Clone)]
    enum Message {
        Edit(Action),
    }

    const GUTTER: gutter::Style = gutter::Style {
        color: Color::BLACK,
        spacing: 8.0,
    };

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
}
