//! Labels drawn among the text without displacing it.

use std::borrow::Cow;

use iced::{Color, Vector};

use crate::Position;

/// A text overlay anchored to a position, drawn without affecting layout.
#[derive(Debug, Clone)]
pub struct Hint<'a> {
    /// Where to anchor the label.
    pub position: Position,
    /// The label text.
    pub label: Cow<'a, str>,
}

/// How to draw an inlay hint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// The label color.
    pub color: Color,
    /// Label size as a fraction of the editor's text size.
    pub size_scale: f32,
    /// Offset from the anchor, in logical pixels.
    pub offset: Vector,
}
