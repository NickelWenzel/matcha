//! Owns the shaped text buffer that the widget draws and decorates.

use std::cell::RefCell;

use iced::advanced::graphics::text;
use iced::advanced::text::editor::{self, Editor as _};

/// The text content of a code editor.
///
/// Unlike iced's `text_editor::Content`, which hides its editor behind a
/// private field, this owns the [`text::Editor`] outright — the shaped buffer
/// inside it is the only source of the glyph geometry that diagnostics, inlay
/// hints, and the gutter are placed against.
#[derive(Debug)]
// The widget is a sibling module, so it needs `pub(super)` to reach the editor at all; a pair
// of accessors would put two more names on a public type to do the same job. The count of edits
// is `pub(super)` for the same reason, and is read through `revision`.
pub struct Content(pub(super) RefCell<text::Editor>, pub(super) u64);

impl Default for Content {
    fn default() -> Self {
        Self::new()
    }
}

/// Reshapes the text from scratch.
///
/// [`text::Editor`] is an `Arc` that mutates through
/// `Arc::try_unwrap`, so a second strong reference to the same editor would
/// panic on the next edit. Cloning therefore has to cost a full reshape, as it
/// does in iced.
impl Clone for Content {
    fn clone(&self) -> Self {
        Self::with_text(&self.text())
    }
}

impl Content {
    /// Creates an empty [`Content`].
    pub fn new() -> Self {
        Self::with_text("")
    }

    /// Creates a [`Content`] holding the given text.
    pub fn with_text(text: &str) -> Self {
        Self(RefCell::new(text::Editor::with_text(text)), 0)
    }

    /// Applies an [`Action`](editor::Action) to the contents.
    pub fn perform(&mut self, action: editor::Action) {
        // Only an edit counts. The widget publishes every action for the application to feed
        // back here, so scrolling, clicking and dragging all arrive; counting those would move
        // the revision on every mouse-move of a drag-select, over text that never changed.
        let is_edit = action.is_edit();

        self.0.borrow_mut().perform(action);

        if is_edit {
            self.1 += 1;
        }
    }

    /// Moves the cursor to the given position.
    pub fn move_to(&mut self, cursor: editor::Cursor) {
        self.0.borrow_mut().move_to(cursor);
    }

    /// Returns the current cursor position.
    pub fn cursor(&self) -> editor::Cursor {
        self.0.borrow().cursor()
    }

    /// Returns the number of lines.
    pub fn line_count(&self) -> usize {
        self.0.borrow().line_count()
    }

    /// Returns the text of the whole buffer.
    pub fn text(&self) -> String {
        self.0.borrow().text()
    }

    /// Returns the selected text, if any.
    pub fn selection(&self) -> Option<String> {
        self.0.borrow().copy()
    }

    /// Returns whether the contents are empty.
    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }

    /// Returns the text of the line at `index`.
    ///
    /// The text is owned: the borrowed [`Line`](editor::Line) the editor hands
    /// out cannot outlive the internal borrow it is read through.
    pub fn line(&self, index: usize) -> Option<String> {
        Some(self.0.borrow().line(index)?.text.to_string())
    }

    /// Returns the line ending used to separate lines, taken from the first
    /// line, or `None` when the contents have no lines at all.
    pub fn line_ending(&self) -> Option<editor::LineEnding> {
        Some(self.0.borrow().line(0)?.ending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use iced::advanced::text::Position;

    #[test]
    fn with_text_round_trips_multibyte_content() {
        let content = Content::with_text("héllo\nworld");

        assert_eq!(content.text(), "héllo\nworld");
        assert_eq!(content.line_count(), 2);
        assert!(!content.is_empty());
    }

    #[test]
    fn a_new_content_is_empty() {
        assert!(Content::new().is_empty());
    }

    #[test]
    fn line_reports_each_line_and_nothing_past_the_end() {
        let content = Content::with_text("héllo\nworld");

        assert_eq!(content.line(0).as_deref(), Some("héllo"));
        assert_eq!(content.line(1).as_deref(), Some("world"));
        assert_eq!(content.line(2), None);
    }

    #[test]
    fn line_ending_reports_the_ending_of_the_first_line() {
        assert_eq!(
            Content::with_text("a\r\nb").line_ending(),
            Some(editor::LineEnding::CrLf)
        );
    }

    #[test]
    fn a_fresh_cursor_sits_at_the_start_with_no_selection() {
        let content = Content::with_text("x");

        assert_eq!(
            content.cursor(),
            editor::Cursor {
                position: Position { line: 0, index: 0 },
                selection: None,
            }
        );
        assert_eq!(content.selection(), None);
    }

    /// Applying every action the widget can publish, an edit aside.
    #[cfg(feature = "lsp")]
    fn perform_everything_but_an_edit(content: &mut Content) {
        use iced::Point;
        use iced::advanced::mouse;

        content.perform(editor::Action::Move(editor::Motion::Right));
        content.perform(editor::Action::Select(editor::Motion::Left));
        content.perform(editor::Action::SelectWord);
        content.perform(editor::Action::SelectLine);
        content.perform(editor::Action::SelectAll);
        content.perform(editor::Action::Click(
            Point::ORIGIN,
            mouse::click::Kind::Single,
        ));
        content.perform(editor::Action::Drag(Point::new(10.0, 0.0)));
        content.perform(editor::Action::Scroll { lines: 3 });
    }

    #[cfg(feature = "lsp")]
    #[test]
    fn a_fresh_content_has_never_been_edited() {
        assert_eq!(Content::with_text("héllo").revision(), 0);
        assert_eq!(Content::new().revision(), 0);
    }

    #[cfg(feature = "lsp")]
    #[test]
    fn every_edit_moves_the_revision_and_nothing_else_does() {
        let mut content = Content::with_text("héllo");

        content.perform(editor::Action::Edit(editor::Edit::Insert('x')));
        assert_eq!(content.revision(), 1);

        // Undo and redo change the text, so they count too.
        content.perform(editor::Action::Edit(editor::Edit::Undo));
        content.perform(editor::Action::Edit(editor::Edit::Redo));
        assert_eq!(content.revision(), 3);

        // A drag-select publishes a click and a drag per mouse-move. Counting
        // those would report the text as changed while the user selects it.
        let unedited = content.revision();

        perform_everything_but_an_edit(&mut content);
        content.move_to(editor::Cursor {
            position: Position { line: 0, index: 0 },
            selection: None,
        });
        let _ = (content.text(), content.cursor(), content.line(0));

        assert_eq!(
            content.revision(),
            unedited,
            "only an edit changes the text, so only an edit may count"
        );
    }

    #[cfg(feature = "lsp")]
    #[test]
    fn a_clone_has_never_been_edited() {
        let mut content = Content::with_text("héllo");

        content.perform(editor::Action::Edit(editor::Edit::Insert('x')));

        // A revision is a count of edits to one `Content`, and a clone has had
        // none. Starting it at zero makes a revision recorded before the clone
        // read as stale, which is the direction that refuses an edit rather
        // than applying a stale one.
        assert_eq!(content.clone().revision(), 0);
        assert_eq!(content.clone().text(), content.text());
    }

    #[test]
    fn move_to_places_the_cursor_and_its_selection() {
        let mut content = Content::with_text("héllo\nworld");

        content.move_to(editor::Cursor {
            position: Position { line: 1, index: 5 },
            selection: Some(Position { line: 1, index: 0 }),
        });

        assert_eq!(
            content.cursor(),
            editor::Cursor {
                position: Position { line: 1, index: 5 },
                selection: Some(Position { line: 1, index: 0 }),
            }
        );
        assert_eq!(content.selection().as_deref(), Some("world"));
    }
}
