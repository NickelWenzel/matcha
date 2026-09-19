//! Underlines that mark a problem with a range of text.

use iced::advanced::renderer;
use iced::{Color, Rectangle, color};

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

/// The look a severity gets when no `diagnostic_style` is given: the colors
/// editors have converged on, over a wave shallow enough to sit under a line of
/// code without reaching the one below it.
impl From<Severity> for Style {
    fn from(severity: Severity) -> Self {
        Self {
            color: match severity {
                Severity::Error => color!(0xe5484d),
                Severity::Warning => color!(0xcca700),
                Severity::Information => color!(0x3794ff),
                Severity::Hint => color!(0x8a8a8a),
            },
            thickness: 1.0,
            amplitude: 2.0,
            // Two pixels of rise over two of run. A longer period reads as a dashed line
            // rather than a wave, and the period is also what sets the quad count, so it
            // is not free to shrink either.
            wavelength: 4.0,
        }
    }
}

/// Draws a wavy underline beneath one visible fragment of a diagnostic's range.
///
/// `fragment` and `baseline` come from [`geometry::range_fragments`] with the
/// text origin already added, so both are in the renderer's own coordinates.
///
/// [`geometry::range_fragments`]: crate::geometry::range_fragments
pub(crate) fn draw_squiggle<Renderer: renderer::Renderer>(
    renderer: &mut Renderer,
    fragment: Rectangle,
    baseline: f32,
    clip_bounds: Rectangle,
    style: Style,
) {
    // A sub-pixel stroke gamma-blends into mud instead of thinning, so the wave is drawn a
    // whole number of pixels thick — the rounding cosmic-text applies to its own underlines.
    let thickness = style.thickness.max(1.0).ceil();

    // Half a period is the wave's run. A period shorter than the stroke has no room to rise
    // and fall within it, and a zero one would divide by zero below and draw nothing at all.
    let run = (style.wavelength / 2.0).max(thickness);

    // The wave hangs one stroke under the baseline. Measuring from the baseline rather than
    // from the bottom of the line box is what keeps it glued to the glyphs: the box grows
    // with the line height and the glyphs sitting on the baseline do not.
    let top = baseline + thickness;
    let right = fragment.x + fragment.width;

    // The top edge of the stroke, as a triangle wave of the distance into the fragment.
    let rise = |x: f32| {
        let phase = (x - fragment.x) % (run * 2.0);

        style.amplitude
            * (if phase < run {
                phase
            } else {
                run * 2.0 - phase
            })
            / run
    };

    let mut x = fragment.x;

    while x < right {
        // One quad per stroke-width of travel: the wave is rasterized at the resolution of
        // its own stroke, and a finer step would only stack quads on top of each other.
        let next = (x + thickness).min(right);
        let (from, to) = (rise(x), rise(next));

        // Each quad covers the whole of the stroke over its own slice of the fragment, so
        // consecutive quads meet at the height they share and the wave comes out unbroken.
        let segment = Rectangle {
            x,
            y: top + from.min(to),
            width: next - x,
            height: (from - to).abs() + thickness,
        };

        // Intersect and skip rather than push a layer: a layer is a whole second batch of
        // primitives for what a rectangle intersection settles, and it is how `State::draw`
        // clips its own selection quads.
        if let Some(bounds) = clip_bounds.intersection(&segment) {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    // `Quad::default()` snaps to the pixel grid whenever iced is built with
                    // `crisp`, which is one of its default features. Snapping every segment
                    // would round the whole wave onto one row, leaving a dashed line.
                    snap: false,
                    ..renderer::Quad::default()
                },
                style.color,
            );
        }

        x = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_severity_gets_its_own_color() {
        let severities = [
            Severity::Error,
            Severity::Warning,
            Severity::Information,
            Severity::Hint,
        ];

        for (index, severity) in severities.iter().enumerate() {
            for other in &severities[index + 1..] {
                assert_ne!(
                    Style::from(*severity).color,
                    Style::from(*other).color,
                    "{severity:?} and {other:?} should be told apart at a glance"
                );
            }
        }
    }
}
