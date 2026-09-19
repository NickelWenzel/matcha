//! Where decorations go: logical text positions mapped to screen geometry.
//!
//! Every coordinate here is relative to the **text origin** — the widget's
//! bounds shrunk by its padding — which is the space the editor reports its own
//! selection in. The widget adds the origin translation when it draws.
//!
//! These functions read the shaped buffer, so they are only meaningful once the
//! visible window has been shaped: call them from `draw`, never from `layout`.
//! A buffer that is only partly shaped yields fewer rows than the viewport
//! holds, which every function here tolerates by simply reporting less.

use iced::advanced::graphics::text::cosmic_text;
use iced::{Point, Rectangle};

use crate::Position;
use crate::decoration::TextRange;

/// The width of a fragment that covers no glyphs, in logical pixels.
///
/// Wide enough for one period of a squiggle, so the marker reads as a marker.
const MIN_FRAGMENT_WIDTH: f32 = 4.0;

/// A visible fragment of a text range, in text-origin-relative coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fragment {
    /// The fragment's box.
    pub bounds: Rectangle,
    /// The text baseline within that box, used to place underlines.
    pub baseline: f32,
}

/// Returns one [`Fragment`] per visible visual row the range covers.
///
/// A range that is scrolled out of view, or that names a line or byte index the
/// buffer does not have, yields no fragments rather than panicking.
///
/// `hint_factor` is the *editor's* — `editor.hint_factor().unwrap_or(1.0)` —
/// because the buffer's coordinate space is scaled by that factor, and not by
/// the renderer's.
pub fn range_fragments(
    buffer: &cosmic_text::Buffer,
    hint_factor: f32,
    range: TextRange,
) -> Vec<Fragment> {
    let scroll = buffer.scroll();
    let start = cosmic_text::Cursor::new(range.start().line, range.start().index);
    let end = cosmic_text::Cursor::new(range.end().line, range.end().index);
    let inverse = 1.0 / hint_factor;

    buffer
        .layout_runs()
        // `highlight` has no line-bounds check of its own: for a run outside `[start.line,
        // end.line]` both halves of its predicate are vacuously true — in both directions — so
        // every grapheme reports as selected and the run comes back as one full-width span.
        // Without this filter, one diagnostic squiggles every visible line.
        .filter(|run| run.line_i >= start.line && run.line_i <= end.line)
        .flat_map(|run| {
            let mut spans: Vec<(f32, f32)> = run.highlight(start, end).collect();

            if spans.is_empty() {
                // `highlight` drops spans of zero width, so a range on a blank line and a
                // zero-width range — what an LSP "insert here" looks like — would otherwise
                // draw nothing at all.
                let anchor = if run.glyphs.is_empty() {
                    // A blank line has no glyph to anchor to, and its own cursor answers for
                    // no other line, so a blank line inside a multi-line range can only be
                    // marked at its left edge.
                    Some(0.0)
                } else {
                    // Anchoring at the column the range starts at, rather than at the left
                    // edge, is what keeps a zero-width range pointing at its own token.
                    // `None` means the range starts on another line or another visual row of
                    // this one, where there is nothing to mark.
                    run.cursor_position(&start)
                };

                // The minimum width is logical, while the spans are in buffer space; the
                // shared scaling below converts both.
                spans.extend(anchor.map(|x| (x, MIN_FRAGMENT_WIDTH * hint_factor)));
            }

            let (top, height, baseline) = (run.line_top, run.line_height, run.line_y);

            spans.into_iter().map(move |(x, width)| Fragment {
                // The iterator has already applied the vertical scroll, but the renderer only
                // applies the horizontal one at draw time, so overlays subtract it themselves.
                // It is a buffer-space distance, hence subtracted before the hint scaling.
                bounds: Rectangle {
                    x: x - scroll.horizontal,
                    y: top,
                    width,
                    height,
                } * inverse,
                baseline: baseline * inverse,
            })
        })
        .collect()
}

/// Returns the top-left anchor of a position, if it is currently visible.
///
/// Affinity is ignored: at a wrap boundary, the index that ends a visual row
/// anchors at the end of that row rather than at the start of the next one.
pub fn position_anchor(
    buffer: &cosmic_text::Buffer,
    hint_factor: f32,
    position: Position,
) -> Option<Point> {
    let scroll = buffer.scroll();
    let cursor = cosmic_text::Cursor::new(position.line, position.index);

    buffer
        .cursor_position(&cursor)
        .map(|(x, top)| Point::new((x - scroll.horizontal) / hint_factor, top / hint_factor))
}

/// Returns `(line index, top)` for the first visual row of every visible
/// logical line — the rows a gutter puts its line numbers on.
pub fn visible_line_rows(
    buffer: &cosmic_text::Buffer,
    hint_factor: f32,
) -> impl Iterator<Item = (usize, f32)> + '_ {
    buffer.layout_runs().filter_map(move |run| {
        // A structural test, not a dedupe on `line_i`: when a wrapped line straddles the top of
        // the viewport, the iterator skips the rows above it and yields a continuation row
        // first, which a dedupe would mislabel as that line's first row. A row's glyphs are
        // stored in visual order, which is reverse-logical for a right-to-left row, so the
        // *minimum* start is the row's logical start and the first glyph's need not be.
        let is_first_row =
            run.glyphs.is_empty() || run.glyphs.iter().map(|glyph| glyph.start).min() == Some(0);

        is_first_row.then(|| (run.line_i, run.line_top / hint_factor))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use iced::advanced::graphics;
    use iced::advanced::text;
    use iced::advanced::text::editor::Editor as _;
    use iced::{Font, Pixels, Size};

    use crate::{Action, Motion};

    const LINE_HEIGHT: f32 = 20.0;
    const VIEWPORT_HEIGHT: f32 = 400.0;

    /// A paragraph long enough to wrap several times at the widths used below.
    const PARAGRAPH: &str = "alpha bravo charlie delta echo foxtrot golf hotel india \
                             juliett kilo lima mike november oscar papa quebec romeo";

    /// Shapes `content` the way the widget's `layout` does, without a window.
    ///
    /// The bundled Fira Sans is the sans-serif family whenever the `fira-sans`
    /// feature is on, so [`Font::DEFAULT`] keeps shaping off the host's fonts.
    fn shaped(content: &str, width: f32, wrapping: text::Wrapping) -> graphics::text::Editor {
        let mut editor = graphics::text::Editor::with_text(content);

        editor.update(
            Size::new(width, VIEWPORT_HEIGHT),
            Font::DEFAULT,
            Pixels(14.0),
            text::LineHeight::Absolute(Pixels(LINE_HEIGHT)),
            wrapping,
            text::Alignment::Default,
            None,
            &mut text::parser::PlainText,
        );

        editor
    }

    fn range(start: (usize, usize), end: (usize, usize)) -> TextRange {
        TextRange::new(
            Position {
                line: start.0,
                index: start.1,
            },
            Position {
                line: end.0,
                index: end.1,
            },
        )
    }

    fn anchor(editor: &graphics::text::Editor, line: usize, index: usize) -> Option<Point> {
        position_anchor(editor.buffer(), 1.0, Position { line, index })
    }

    fn fragments(editor: &graphics::text::Editor, range: TextRange) -> Vec<Fragment> {
        range_fragments(editor.buffer(), 1.0, range)
    }

    #[test]
    fn a_range_on_one_line_yields_fragments_only_on_that_line() {
        let editor = shaped("alpha\nbravo\ncharlie\ndelta", 400.0, text::Wrapping::None);

        // The regression this guards is a squiggle on every *other* visible line, so the
        // other lines have to be visible for the test to mean anything.
        assert_eq!(visible_line_rows(editor.buffer(), 1.0).count(), 4);

        let fragments = fragments(&editor, range((1, 0), (1, 5)));

        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].bounds.y, anchor(&editor, 1, 0).unwrap().y);
        assert!(fragments[0].bounds.width > 0.0);
    }

    #[test]
    fn a_range_spanning_three_lines_yields_a_fragment_on_each() {
        let editor = shaped("alpha\nbravo\ncharlie\ndelta", 400.0, text::Wrapping::None);

        let fragments = fragments(&editor, range((0, 2), (2, 3)));

        assert_eq!(fragments.len(), 3);
        assert_eq!(
            fragments
                .iter()
                .map(|fragment| fragment.bounds.y)
                .collect::<Vec<_>>(),
            (0..3)
                .map(|line| anchor(&editor, line, 0).unwrap().y)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_reversed_range_yields_the_same_fragments_as_the_forward_range() {
        let editor = shaped("alpha\nbravo\ncharlie\ndelta", 400.0, text::Wrapping::None);

        assert_eq!(
            fragments(&editor, range((0, 2), (2, 3))),
            fragments(&editor, range((2, 3), (0, 2)))
        );
    }

    #[test]
    fn a_range_crossing_a_wrap_yields_a_fragment_per_visual_row() {
        let editor = shaped(PARAGRAPH, 160.0, text::Wrapping::Word);

        let rows = editor.buffer().layout_runs().count();
        assert!(
            rows > 1,
            "the paragraph has to wrap for this to test anything"
        );

        let fragments = fragments(&editor, range((0, 0), (0, PARAGRAPH.len())));

        assert_eq!(fragments.len(), rows);

        let tops = fragments
            .iter()
            .map(|fragment| fragment.bounds.y)
            .collect::<Vec<_>>();

        assert!(tops.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn a_range_on_a_blank_line_is_marked_at_its_left_edge() {
        let editor = shaped("alpha\n\ncharlie", 400.0, text::Wrapping::None);

        let fragments = fragments(&editor, range((1, 0), (1, 0)));

        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].bounds.x, 0.0);
        assert_eq!(fragments[0].bounds.width, MIN_FRAGMENT_WIDTH);
        assert_eq!(fragments[0].bounds.y, anchor(&editor, 1, 0).unwrap().y);
    }

    #[test]
    fn a_zero_width_range_is_marked_at_the_column_it_points_at() {
        let editor = shaped("alpha bravo", 400.0, text::Wrapping::None);

        let fragments = fragments(&editor, range((0, 6), (0, 6)));

        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].bounds.width, MIN_FRAGMENT_WIDTH);
        assert_eq!(fragments[0].bounds.x, anchor(&editor, 0, 6).unwrap().x);
        assert!(fragments[0].bounds.x > 0.0, "column 6 is not the left edge");
    }

    #[test]
    fn a_blank_line_inside_a_range_gets_its_own_fragment() {
        let editor = shaped("alpha\n\ncharlie", 400.0, text::Wrapping::None);

        let fragments = fragments(&editor, range((0, 0), (2, 7)));

        assert_eq!(fragments.len(), 3);
        assert_eq!(fragments[1].bounds.x, 0.0);
        assert_eq!(fragments[1].bounds.width, MIN_FRAGMENT_WIDTH);
        assert_eq!(fragments[1].bounds.y, anchor(&editor, 1, 0).unwrap().y);
    }

    #[test]
    fn a_range_starting_on_a_later_visual_row_leaves_the_rows_above_it_alone() {
        let editor = shaped(PARAGRAPH, 160.0, text::Wrapping::Word);

        let second_row = editor
            .buffer()
            .layout_runs()
            .nth(1)
            .and_then(|run| run.glyphs.iter().map(|glyph| glyph.start).min())
            .expect("the paragraph should wrap onto a second row");

        let first_row_top = anchor(&editor, 0, 0).unwrap().y;

        for fragment in fragments(&editor, range((0, second_row), (0, second_row))) {
            assert_ne!(fragment.bounds.y, first_row_top);
        }
    }

    #[test]
    fn multibyte_clusters_yield_positive_width_fragments() {
        let editor = shaped(
            "héllo wörld\n日本語のテキスト\n👩‍👩‍👧‍👦 family",
            400.0,
            text::Wrapping::None,
        );

        // "héllo" is 6 bytes, three CJK characters are 9, and the family emoji is one
        // 25-byte grapheme cluster.
        for covered in [
            range((0, 0), (0, 6)),
            range((1, 0), (1, 9)),
            range((2, 0), (2, 25)),
        ] {
            let fragments = fragments(&editor, covered);

            assert_eq!(fragments.len(), 1);
            assert!(fragments[0].bounds.width > 0.0);
        }

        let three = fragments(&editor, range((1, 0), (1, 9)))[0].bounds.width;
        let eight = fragments(&editor, range((1, 0), (1, 24)))[0].bounds.width;

        assert!(three < eight);
    }

    #[test]
    fn an_rtl_line_is_placed_from_its_glyph_extremes() {
        // Hebrew lays out right to left, so a row's glyphs can be stored in reverse logical
        // order and only the extremes of their byte indices say where the row starts.
        let editor = shaped(
            "שלום עולם רחב מאוד גדול ויפה מאוד כאן שם",
            120.0,
            text::Wrapping::Word,
        );

        let rows = editor.buffer().layout_runs().count();

        assert!(rows > 1, "the line has to wrap for this to test anything");
        assert!(editor.buffer().layout_runs().all(|run| run.rtl));

        // Still one gutter row for the whole logical line.
        assert_eq!(
            visible_line_rows(editor.buffer(), 1.0).collect::<Vec<_>>(),
            vec![(0, 0.0)]
        );

        // The first word sits at the right-hand end of the first row, not at x == 0.
        let first_word = fragments(&editor, range((0, 0), (0, 8)));

        assert_eq!(first_word.len(), 1);
        assert!(first_word[0].bounds.width > 0.0);
        assert!(first_word[0].bounds.x > 0.0);
    }

    #[test]
    fn a_range_at_the_start_or_end_of_a_line_has_width() {
        let editor = shaped("alpha bravo", 400.0, text::Wrapping::None);

        let first = fragments(&editor, range((0, 0), (0, 1)));
        let last = fragments(&editor, range((0, 10), (0, 11)));

        assert_eq!(first.len(), 1);
        assert_eq!(last.len(), 1);
        assert!(first[0].bounds.width > 0.0);
        assert!(last[0].bounds.width > 0.0);
        assert!(first[0].bounds.x < last[0].bounds.x);
    }

    #[test]
    fn an_anchor_at_a_wrap_boundary_resolves_without_affinity() {
        let editor = shaped(PARAGRAPH, 160.0, text::Wrapping::Word);

        let mut runs = editor.buffer().layout_runs();
        let first = runs.next().expect("the buffer should have a first row");
        let second = runs.next().expect("the paragraph should wrap");

        let first_end = first
            .glyphs
            .iter()
            .map(|glyph| glyph.end)
            .max()
            .expect("the first row should have glyphs");
        let second_start = second
            .glyphs
            .iter()
            .map(|glyph| glyph.start)
            .min()
            .expect("the second row should have glyphs");
        let (first_top, second_top, first_width) = (first.line_top, second.line_top, first.line_w);

        // The index that ends the first row anchors at that row's end, even though its
        // affinity would put it at the start of the second.
        let end_of_first = anchor(&editor, 0, first_end).expect("the boundary should be visible");

        assert_eq!(end_of_first.y, first_top);
        assert!((end_of_first.x - first_width).abs() < 0.01);

        // The index that starts the second row anchors there, at its left edge.
        let start_of_second =
            anchor(&editor, 0, second_start).expect("the boundary should be visible");

        assert_eq!(start_of_second.y, second_top);
        assert_eq!(start_of_second.x, 0.0);
    }

    #[test]
    fn horizontal_scroll_shifts_fragments_left_by_the_scroll_amount() {
        let mut editor = shaped(PARAGRAPH, 160.0, text::Wrapping::None);

        // A range away from the left edge: at x == 0 every scaling order agrees, which would
        // make the hint-factor assertions below vacuous.
        let word = range((0, 6), (0, 11));
        let before = fragments(&editor, word);

        // Nothing sets the horizontal scroll directly: it follows the caret, and each motion
        // moves it by at most the caret's own width.
        for _ in 0..40 {
            editor.perform(Action::Move(Motion::End));
        }

        let scrolled = editor.buffer().scroll().horizontal;
        assert!(scrolled > 0.0, "the caret should have scrolled the line");

        let after = fragments(&editor, word);

        assert_eq!(after.len(), before.len());
        assert_eq!(after[0].bounds.x, before[0].bounds.x - scrolled);
        assert_eq!(after[0].bounds.y, before[0].bounds.y);
        assert_eq!(after[0].bounds.width, before[0].bounds.width);

        // The scroll is a buffer-space distance, so it comes off before the hint scaling: the
        // two orderings only agree when one of them is zero.
        let position = Position { line: 0, index: 6 };
        let hinted = range_fragments(editor.buffer(), 2.0, word);

        assert_eq!(hinted[0].bounds.x * 2.0, after[0].bounds.x);
        assert_eq!(
            position_anchor(editor.buffer(), 2.0, position).unwrap().x * 2.0,
            position_anchor(editor.buffer(), 1.0, position).unwrap().x
        );
    }

    #[test]
    fn vertical_scroll_moves_fragments_up_and_drops_the_lines_above_the_viewport() {
        let content: String = (0..40).map(|line| format!("line {line}\n")).collect();
        let mut editor = shaped(&content, 400.0, text::Wrapping::None);

        let before = fragments(&editor, range((15, 0), (15, 4)));
        assert_eq!(before.len(), 1);
        assert_eq!(fragments(&editor, range((2, 0), (2, 4))).len(), 1);

        let lines = 10;
        editor.perform(Action::Scroll { lines });

        let after = fragments(&editor, range((15, 0), (15, 4)));

        assert_eq!(after.len(), 1);
        assert_eq!(
            after[0].bounds.y,
            before[0].bounds.y - lines as f32 * LINE_HEIGHT
        );
        assert!(fragments(&editor, range((2, 0), (2, 4))).is_empty());
    }

    #[test]
    fn visible_line_rows_reports_each_visible_line_once_in_order() {
        let editor = shaped("alpha\n\ncharlie\ndelta", 400.0, text::Wrapping::None);

        let rows = visible_line_rows(editor.buffer(), 1.0).collect::<Vec<_>>();

        assert_eq!(
            rows,
            (0..4)
                .map(|line| (line, anchor(&editor, line, 0).unwrap().y))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_wrapped_line_is_reported_on_its_first_row_only() {
        let editor = shaped(PARAGRAPH, 160.0, text::Wrapping::Word);

        let rows = visible_line_rows(editor.buffer(), 1.0).collect::<Vec<_>>();

        assert!(editor.buffer().layout_runs().count() > 1);
        assert_eq!(rows, vec![(0, 0.0)]);
    }

    #[test]
    fn a_wrapped_line_straddling_the_viewport_top_is_not_mislabelled() {
        let tail: String = (0..40).map(|line| format!("\nline {line}")).collect();
        let mut editor = shaped(&format!("{PARAGRAPH}{tail}"), 160.0, text::Wrapping::Word);

        let before = fragments(&editor, range((1, 0), (1, 4)));

        editor.perform(Action::Scroll { lines: 1 });

        // The scroll has to land inside the first line's rows rather than past them, or there
        // is no straddle to mislabel.
        assert_eq!(editor.buffer().scroll().vertical, LINE_HEIGHT);

        {
            // So the first row the iterator yields is a continuation of line 0.
            let first = editor
                .buffer()
                .layout_runs()
                .next()
                .expect("the buffer should still have visible rows");

            assert_eq!(first.line_i, 0);
            assert_ne!(first.glyphs.iter().map(|glyph| glyph.start).min(), Some(0));
        }

        let rows = visible_line_rows(editor.buffer(), 1.0).collect::<Vec<_>>();

        // A dedupe on `line_i` would have reported that continuation row as line 0's first.
        assert!(rows.iter().all(|(line, _)| *line != 0));
        assert_eq!(rows.first().map(|(line, _)| *line), Some(1));

        // The row iterator has already applied the vertical scroll; subtracting it again here
        // would move everything up by twice as much.
        let after = fragments(&editor, range((1, 0), (1, 4)));

        assert_eq!(after[0].bounds.y, before[0].bounds.y - LINE_HEIGHT);
        assert_eq!(rows.first().map(|(_, top)| *top), Some(after[0].bounds.y));
    }

    #[test]
    fn a_line_past_the_end_of_the_buffer_yields_nothing() {
        let editor = shaped("alpha\nbravo", 400.0, text::Wrapping::None);

        assert!(fragments(&editor, range((99, 0), (99, 3))).is_empty());
        assert_eq!(anchor(&editor, 99, 0), None);
    }

    #[test]
    fn a_byte_index_past_the_end_of_its_line_yields_nothing() {
        let editor = shaped("alpha\nbravo", 400.0, text::Wrapping::None);

        assert!(fragments(&editor, range((0, 10), (0, 20))).is_empty());
        assert_eq!(anchor(&editor, 0, 10), None);
    }

    #[test]
    fn an_index_off_a_char_boundary_marks_the_cluster_it_lands_in() {
        // Byte 2 is inside the two-byte "é" that spans bytes 1 to 3.
        let editor = shaped("héllo", 400.0, text::Wrapping::None);

        let fragments = fragments(&editor, range((0, 2), (0, 2)));

        assert_eq!(fragments.len(), 1);
        assert!(fragments[0].bounds.width > 0.0);
        assert_eq!(fragments[0].bounds.x, anchor(&editor, 0, 1).unwrap().x);
        assert!(anchor(&editor, 0, 2).is_some());
    }

    #[test]
    fn an_anchor_scrolled_out_of_view_is_none() {
        let content: String = (0..40).map(|line| format!("line {line}\n")).collect();
        let mut editor = shaped(&content, 400.0, text::Wrapping::None);

        assert!(anchor(&editor, 0, 0).is_some());

        editor.perform(Action::Scroll { lines: 10 });

        assert_eq!(anchor(&editor, 0, 0), None);
        assert!(anchor(&editor, 12, 0).is_some());
    }

    #[test]
    fn a_hint_factor_scales_every_coordinate() {
        let editor = shaped("alpha\n\ncharlie", 400.0, text::Wrapping::None);
        let covered = range((0, 0), (0, 5));

        let plain = range_fragments(editor.buffer(), 1.0, covered);
        let hinted = range_fragments(editor.buffer(), 2.0, covered);

        assert_eq!(hinted.len(), plain.len());
        assert_eq!(hinted[0].bounds * 2.0, plain[0].bounds);
        assert_eq!(hinted[0].baseline * 2.0, plain[0].baseline);

        // The minimum width is logical, so it survives the scaling unchanged.
        let blank = range_fragments(editor.buffer(), 2.0, range((1, 0), (1, 0)));

        assert_eq!(blank[0].bounds.width, MIN_FRAGMENT_WIDTH);
    }
}
