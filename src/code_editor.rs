//! The code editor widget and its supporting types.

mod content;

pub use content::Content;

// Naming `Cursor` and `Position` is unavoidable for anyone calling `Content::move_to`, and
// `LineEnding` for anyone reading `Content::line_ending`. Re-exporting them keeps callers off
// `iced::advanced::*`, which is gated behind a feature they would otherwise have to enable just
// to spell types this crate already hands them. This mirrors what `iced::widget::text_editor`
// re-exports, plus `Position`, which iced omits but which is needed to build a `Cursor`.
pub use iced::advanced::text::Position;
pub use iced::advanced::text::editor::{
    Action, Binding, Cursor, Edit, KeyPress, Line, LineEnding, Motion, Selection,
};
