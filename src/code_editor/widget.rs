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
use iced::advanced::text::{self, highlighter, parser};
use iced::advanced::widget::{self, Widget};
use iced::alignment;
use iced::widget::text_editor;
use iced::window;
use iced::{Element, Event, Font, Length, Padding, Pixels, Rectangle, Size};

use crate::code_editor::Content;

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

struct State<Parser: text::Parser> {
    editor: editor::State,
    parser: RefCell<Parser>,
    parser_settings: Parser::Settings,
    last_theme: RefCell<Option<String>>,
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
}

impl<Parser, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for CodeEditor<'_, Parser, Message, Theme, Renderer>
where
    Parser: text::Parser,
    Theme: text_editor::Catalog,
    Renderer: text::Renderer<Editor = graphics::text::Editor>,
{
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State<Parser>>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State {
            editor: editor::State::new(),
            parser: RefCell::new(Parser::new(&self.parser_settings)),
            parser_settings: self.parser_settings.clone(),
            last_theme: RefCell::new(None),
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
        let state = tree.state.downcast_mut::<State<Parser>>();

        if state.parser_settings != self.parser_settings {
            state.parser.borrow_mut().update(&self.parser_settings);

            state.parser_settings = self.parser_settings.clone();
        }

        let limits = limits
            .width(self.width)
            .height(self.height)
            .shrink(self.padding);

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

        layout::Node::new(bounds.expand(self.padding))
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let Some(on_edit) = self.on_edit.as_ref() else {
            return;
        };

        let state = tree.state.downcast_mut::<State<Parser>>();
        let is_redraw = matches!(event, Event::Window(window::Event::RedrawRequested(_now)));

        let content = self.content.0.borrow();

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
            self.padding,
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
                    .input_method(&*content, layout.bounds().shrink(self.padding).position()),
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
        let state = tree.state.downcast_ref::<State<Parser>>();

        let font = self.font.unwrap_or_else(|| renderer.font());

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

        let text_bounds = bounds.shrink(self.padding);

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
    }

    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let is_disabled = self.on_edit.is_none();

        if cursor.is_over(layout.bounds()) {
            if is_disabled {
                mouse::Interaction::NotAllowed
            } else {
                mouse::Interaction::Text
            }
        } else {
            mouse::Interaction::default()
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
        let state = tree.state.downcast_mut::<State<Parser>>();

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

    use iced::Point;
    use iced_test::simulator;

    #[derive(Debug, Clone)]
    enum Message {
        Edit(editor::Action),
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
}
