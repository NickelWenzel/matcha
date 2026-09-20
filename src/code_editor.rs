//! The code editor widget and its supporting types.

pub mod decoration;
pub mod gutter;

// Not public: every function here takes the `cosmic_text::Buffer` behind a `Content`, and
// `Content` keeps its editor `pub(super)`, so no caller outside the crate can obtain one. It
// was `pub` only to keep `dead_code` quiet while it had no callers; the widget now calls all
// three. Widening it again means committing to a public `Content::buffer()`, which would put a
// git-pinned fork's types in this crate's API.
pub(crate) mod geometry;

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
