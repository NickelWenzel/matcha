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
pub struct Content(RefCell<text::Editor>);

impl Default for Content {
    fn default() -> Self {
        Self::new()
    }
}

impl Content {
    /// Creates an empty [`Content`].
    pub fn new() -> Self {
        Self::with_text("")
    }

    /// Creates a [`Content`] holding the given text.
    pub fn with_text(text: &str) -> Self {
        Self(RefCell::new(text::Editor::with_text(text)))
    }

    /// Applies an [`Action`](editor::Action) to the contents.
    pub fn perform(&mut self, action: editor::Action) {
        self.0.borrow_mut().perform(action);
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
