//! The line-number gutter: how much room it takes, and where its numbers land.
//!
//! The widget folds the width below into its left padding, so these two
//! functions and the editor's own padding arithmetic are the whole of the
//! gutter — there is no second buffer and no separate hit-testing.

use iced::advanced::text::{self, paragraph};
use iced::{Color, Point, Rectangle, Size};

/// How to draw the line-number gutter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// The color of the line numbers.
    pub color: Color,
    /// The gap between the numbers and the text, in logical pixels.
    pub spacing: f32,
}

/// Returns the room the gutter takes to the left of the text: the widest line
/// number, plus [`Style::spacing`].
///
/// The digit count comes from the *total* line count, never from the numbers
/// currently on screen. The width is folded into the editor's left padding, so
/// one that grew the moment a four-digit number scrolled into view would move
/// the text origin out from under the mouse and desync hit-testing from
/// rendering — the bug cosmic-edit fixed in `8e7dbaa`.
///
/// `numbers` is the caller's measurement cache; it reshapes only when the digit
/// count or the text attributes change.
pub(crate) fn width<Paragraph: text::Paragraph>(
    style: Style,
    numbers: &mut paragraph::Plain<Paragraph>,
    line_count: usize,
    text: text::Text<()>,
) -> f32 {
    let digits = line_count.max(1).ilog10() as usize + 1;

    // A run of zeros rather than the highest number itself: the fonts a code editor is set
    // in give every digit the same advance, and a string that depends on nothing but the
    // digit count leaves the cache alone through every edit that does not add a line.
    let widest = "0".repeat(digits);

    let _ = numbers.update(text.with_content(widest.as_str()));

    numbers.min_bounds().width + style.spacing
}

/// Draws a line number on each of the given rows.
///
/// `rows` are the `(line, top)` pairs of [`visible_line_rows`], so the
/// continuation rows of a wrapped line are already absent and get no number.
/// `bounds` is the gutter's own rectangle and `top` is measured from its top
/// edge: the rows arrive adjusted for the vertical scroll and never for the
/// horizontal one, which is what keeps the numbers still while the text slides
/// sideways.
///
/// [`visible_line_rows`]: crate::code_editor::geometry::visible_line_rows
pub(crate) fn draw<Renderer: text::Renderer>(
    style: Style,
    renderer: &mut Renderer,
    bounds: Rectangle,
    viewport: Rectangle,
    rows: impl Iterator<Item = (usize, f32)>,
    text: text::Text<()>,
) {
    let Some(clip_bounds) = viewport.intersection(&bounds) else {
        return;
    };

    let line_height = text.line_height.to_absolute(text.size);

    // Both backends apply `Alignment::Right` by subtracting the shaped width from the
    // position, so what they want is the right edge of the numbers. Holding it back by the
    // spacing puts the gap between the numbers and the text instead of to their left.
    let right = bounds.x + bounds.width - style.spacing;

    for (line, top) in rows {
        renderer.fill_text(
            text::Text {
                bounds: Size::new(bounds.width, line_height.into()),
                ..text.with_content((line + 1).to_string())
            },
            Point::new(right, bounds.y + top),
            style.color,
            clip_bounds,
        );
    }
}
