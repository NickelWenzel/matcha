//! The code editor widget and its supporting types.

pub mod decoration;
pub mod geometry;

mod content;
mod widget;

pub use content::Content;
pub use widget::{CodeEditor, code_editor};

// Naming `Action` is unavoidable for anyone calling `on_action`, and `Binding`/`KeyPress` for
// anyone calling `key_binding`. Re-exporting them keeps callers off `iced::advanced::*`, which
// is gated behind a feature they would otherwise have to enable just to spell their own message
// type. This mirrors what `iced::widget::text_editor` re-exports, plus `Position`, which iced
// omits but which is needed to build a `Cursor` for `Content::move_to`.
pub use iced::advanced::text::Position;
pub use iced::advanced::text::editor::{
    Action, Binding, Cursor, Edit, KeyPress, Line, LineEnding, Motion, Selection,
};
