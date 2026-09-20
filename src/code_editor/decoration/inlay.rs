//! Labels on opaque chips, laid over the text without moving any of it.

use std::borrow::Cow;

use iced::{Background, Border, Color, Padding, Pixels, Vector};

use crate::Position;

/// The size a hint is drawn at, as a fraction of the code it annotates.
///
/// Small enough that the eye reads the code first, large enough to stay legible
/// beside it.
const SIZE_SCALE: f32 = 0.75;

/// A label anchored to a position, drawn on a chip that hides the code beneath
/// it and changes nothing about how that code is laid out.
#[derive(Debug, Clone)]
pub struct Hint<'a> {
    /// Where to anchor the label.
    pub position: Position,
    /// The label text.
    pub label: Cow<'a, str>,
}

/// How to draw an inlay hint: a label on a filled, outlined chip that hides the
/// code beneath it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// The label color.
    pub color: Color,
    /// What the chip behind the label is filled with.
    ///
    /// Opacity is the point. The chip is what makes a label legible inside a
    /// line of code, so a fill the code shows through undoes the whole thing.
    pub background: Background,
    /// The outline drawn around the chip, inside its bounds.
    ///
    /// A quad mixes its border into its fill rather than compositing it over,
    /// so the outermost pixel of a chip is only as opaque as this color. That
    /// costs a hairline of the occlusion, which is a fair trade for an edge:
    /// rounded corners are anti-aliased whether or not anything is drawn on
    /// them, so the boundary is soft either way.
    pub border: Border,
    /// Label size as a fraction of the editor's text size.
    pub size_scale: f32,
    /// Offset from the anchor, in logical pixels.
    pub offset: Vector,
    /// Space between the label and the edge of its chip.
    pub padding: Padding,
}

impl Style {
    /// The look a hint gets when no `inlay_style` is given.
    ///
    /// Both colors are the caller's because a hint has to read as *not code*
    /// against whatever the theme paints behind it, which no fixed pair does
    /// for every theme; the widget passes the color its theme dims text to and
    /// the background the editor itself is filled with, so that a chip reads as
    /// a hole punched in the code rather than as a badge stuck over it. The
    /// outline is taken from the label rather than asked for as a third color,
    /// because the fill is the caller's and can be anything, and the hue the
    /// label is already legible in is the one guaranteed to read against it.
    /// The text size is the code's, which the padding is a fraction of so that
    /// the chip keeps its proportions at every size.
    pub fn new(color: Color, background: Background, text_size: Pixels) -> Self {
        let size = text_size * SIZE_SCALE;

        Self {
            color,
            background,
            // Scaled well down, because a frame at the label's full strength is read
            // before the label is and the label is the point — and because the border
            // is mixed into the fill rather than laid over it, so the fainter it is the
            // less of the occlusion it costs. Rounded and one pixel wide like the
            // editor's own frame, so that a chip looks like part of the widget it sits
            // in rather than something pasted over it.
            border: Border {
                color: color.scale_alpha(0.4),
                width: 1.0,
                radius: 2.0.into(),
            },
            size_scale: SIZE_SCALE,
            // Clear of the anchor to the right, so the label does not start on top of the
            // glyph it annotates — and level with the row, because a chip stands exactly
            // one row tall and raising it would leave the descenders it has to hide
            // showing beneath.
            offset: Vector::new(2.0, 0.0),
            // Room either side of the label, so it does not touch the code it interrupts,
            // and none above or below, where the chip already covers its row exactly.
            padding: Padding::from([0.0, size.0 * 0.25]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default look at `text_size`, over a fill no theme would produce.
    fn look(text_size: f32) -> Style {
        Style::new(
            Color::BLACK,
            Background::Color(Color::WHITE),
            Pixels(text_size),
        )
    }

    #[test]
    fn the_default_look_nudges_a_label_clear_of_the_glyph_it_annotates() {
        let style = look(16.0);

        assert!(
            style.size_scale < 1.0,
            "a hint has to read as a note about the code, not as more of it"
        );

        // Right of the anchor, so the label does not start on top of the glyph it
        // annotates.
        assert!(style.offset.x > 0.0);

        // Level with it, though. A chip is exactly as tall as the row it covers, so
        // raising it would uncover the bottom of that row — where the descenders are —
        // while clipping the row above.
        assert_eq!(style.offset.y, 0.0);
    }

    #[test]
    fn the_default_chip_is_outlined_in_the_color_of_its_label() {
        let style = look(16.0);

        // A border of no width is drawn as nothing at all, which leaves a chip as the
        // bare fill it was before.
        assert!(style.border.width > 0.0);

        // The label's own hue. The fill is the caller's and can be any color, so an
        // outline in a hue of its own is a gamble on a contrast nobody checked, while
        // this one is already known to read there — the label is drawn in it.
        assert_eq!(
            [
                style.border.color.r,
                style.border.color.g,
                style.border.color.b
            ],
            [style.color.r, style.color.g, style.color.b]
        );

        // Visible, but dimmer than what it surrounds: a frame as strong as the glyphs
        // inside it is read first, and the label is the thing worth reading.
        assert!(style.border.color.a > 0.0);
        assert!(style.border.color.a < style.color.a);

        // Rounded on every corner, so a chip reads as something laid into the code
        // rather than as a block of it.
        let radius = style.border.radius;

        assert!(
            [
                radius.top_left,
                radius.top_right,
                radius.bottom_right,
                radius.bottom_left
            ]
            .iter()
            .all(|corner| *corner > 0.0)
        );
    }

    #[test]
    fn the_default_padding_keeps_its_proportions_at_every_text_size() {
        let small = look(10.0);
        let large = look(20.0);

        assert!(
            small.padding.x() > 0.0,
            "a label needs room either side of it, or the chip cuts into its glyphs"
        );
        assert_eq!(large.padding.x(), small.padding.x() * 2.0);

        // None above or below, for the same reason the offset is level with the row.
        assert_eq!(small.padding.y(), 0.0);
        assert_eq!(large.padding.y(), 0.0);
    }
}
