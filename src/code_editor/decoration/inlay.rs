//! Labels drawn among the text without displacing it.

use std::borrow::Cow;

use iced::{Color, Pixels, Vector};

use crate::Position;

/// The size a hint is drawn at, as a fraction of the code it annotates.
///
/// Small enough that the eye reads the code first, large enough to stay legible
/// beside it.
const SIZE_SCALE: f32 = 0.75;

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

impl Style {
    /// The look a hint gets when no `inlay_style` is given.
    ///
    /// The color is the caller's because a hint has to read as *not code*
    /// against whatever the theme paints behind it, which no fixed color does
    /// for every theme; the widget passes the color its theme dims text to. The
    /// text size is the code's, which the offset is a fraction of so that the
    /// nudge keeps its proportions at every size.
    pub fn new(color: Color, text_size: Pixels) -> Self {
        let size = text_size * SIZE_SCALE;

        Self {
            color,
            size_scale: SIZE_SCALE,
            // Clear of the anchor to the right, so the label does not start on top of the
            // glyph it annotates, and raised, so it reads as a note about the line rather
            // than as more of it.
            offset: Vector::new(2.0, -size.0 * 0.25),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_look_nudges_a_label_clear_of_the_glyph_it_annotates() {
        let style = Style::new(Color::BLACK, Pixels(16.0));

        assert!(
            style.size_scale < 1.0,
            "a hint has to read as a note about the code, not as more of it"
        );

        // Right of the anchor, so the label does not start on top of the glyph it
        // annotates, and up, so it does not sit in line with the code.
        assert!(style.offset.x > 0.0);
        assert!(style.offset.y < 0.0);
    }

    #[test]
    fn the_default_offset_keeps_its_proportions_at_every_text_size() {
        let small = Style::new(Color::BLACK, Pixels(10.0));
        let large = Style::new(Color::BLACK, Pixels(20.0));

        assert_eq!(large.offset.y, small.offset.y * 2.0);
    }
}
