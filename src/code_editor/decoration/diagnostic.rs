//! Underlines that mark a problem with a range of text.

use iced::Color;

use super::TextRange;

/// How severe a [`Diagnostic`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Something that keeps the code from building or running.
    Error,
    /// Something that builds, but is probably a mistake.
    Warning,
    /// Something worth knowing about the code.
    Information,
    /// A suggestion, such as a possible refactor.
    Hint,
}

/// A range of text to underline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diagnostic {
    /// The range to underline.
    pub range: TextRange,
    /// How severe it is.
    pub severity: Severity,
}

/// How to draw a diagnostic underline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// The color of the wave.
    pub color: Color,
    /// Stroke thickness, in logical pixels.
    pub thickness: f32,
    /// Peak-to-trough height, in logical pixels.
    pub amplitude: f32,
    /// Horizontal period, in logical pixels.
    pub wavelength: f32,
}
