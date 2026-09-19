//! A code editor widget for [iced], with diagnostic underlines, inlay-hint
//! overlays, and a line-number gutter.
//!
//! [iced]: https://github.com/iced-rs/iced

#![warn(missing_docs)]

pub mod code_editor;

pub use code_editor::{
    Action, Binding, Content, Cursor, Edit, KeyPress, Line, LineEnding, Motion, Position,
    Selection,
};
