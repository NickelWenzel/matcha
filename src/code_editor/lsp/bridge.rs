//! Converting between a language server's coordinates and the editor's.

use iced::advanced::text::editor::Editor as _;

use super::{Diagnostic, Encoding, Hint, Position, Range};
use crate::code_editor::content::Content;
use crate::code_editor::decoration::{self, TextRange};

/// Why a position did not land exactly.
///
/// Each variant carries what [`Bridge::clamp`] adjusts with, so adjusting never
/// costs a second scan of the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reason {
    /// The line is past the end of the buffer.
    Line,
    /// The column is past the end of its line, which is this many bytes long.
    Column { length: usize },
    /// The column is inside a character that starts at this byte.
    NotACharBoundary { floor: usize },
}

/// Walks one line, answering columns in the order it is asked for.
///
/// Asking in ascending order walks the line once, which is what the batch
/// conversions do. Asking out of order is answered correctly and costs a
/// restart.
struct Columns<'a> {
    text: &'a str,
    encoding: Encoding,
    /// Whether every character is one byte and one unit, whatever the encoding.
    ascii: bool,
    /// The byte the next character starts at, and the column at that byte.
    byte: usize,
    units: u32,
}

impl<'a> Columns<'a> {
    fn new(text: &'a str, encoding: Encoding) -> Self {
        Self {
            ascii: text.is_ascii(),
            text,
            encoding,
            byte: 0,
            units: 0,
        }
    }

    fn byte_at(&mut self, character: u32) -> Result<usize, Reason> {
        // Source is mostly ASCII, and `is_ascii` is one vectorised pass, so the
        // walk below is the uncommon path.
        if self.ascii {
            let index = character as usize;

            return if index <= self.text.len() {
                Ok(index)
            } else {
                Err(Reason::Column {
                    length: self.text.len(),
                })
            };
        }

        if character < self.units {
            self.byte = 0;
            self.units = 0;
        }

        while self.units < character {
            let Some(next) = self.text[self.byte..].chars().next() else {
                return Err(Reason::Column {
                    length: self.text.len(),
                });
            };
            let width = self.encoding.width(next);

            // Passing the column without landing on it means the column points
            // between the units of `next`, which UTF-16 allows for anything
            // outside the basic plane.
            if self.units + width > character {
                return Err(Reason::NotACharBoundary { floor: self.byte });
            }

            self.units += width;
            self.byte += next.len_utf8();
        }

        Ok(self.byte)
    }
}

/// Where [`Bridge::clamp`] puts a column that did not land.
fn clamped(reason: &Reason) -> usize {
    match reason {
        Reason::NotACharBoundary { floor } => *floor,
        Reason::Column { length } => *length,
        // A line is resolved before any of its columns are, so no column can
        // report one.
        Reason::Line => 0,
    }
}

/// Converts positions between a language server's coordinates and the editor's.
///
/// Built by [`Content::lsp`] from the text and the [`Encoding`] the server
/// negotiated. A bridge borrows the content, so the text cannot change while
/// one is alive and every position it converts describes the same buffer.
///
/// Converting needs the text because a server counts columns in code units and
/// the editor counts them in bytes. The two are the same only for a line of
/// ASCII.
pub struct Bridge<'a> {
    content: &'a Content,
    encoding: Encoding,
}

impl Content {
    /// Converts positions for a server that negotiated `encoding`.
    ///
    /// ```no_run
    /// # use matcha::Content;
    /// # use matcha::lsp::{Encoding, Position};
    /// # let content = Content::with_text("fn main() {}");
    /// let at = content.lsp(Encoding::Utf16).clamp(Position { line: 0, character: 3 });
    /// ```
    pub fn lsp(&self, encoding: Encoding) -> Bridge<'_> {
        Bridge {
            content: self,
            encoding,
        }
    }

    /// How many times this content has been edited.
    ///
    /// Typing, pasting, undo and redo each move it. Scrolling, clicking,
    /// dragging and moving the cursor leave it alone, as does reading the text.
    /// A clone starts again at zero, so a revision means something only against
    /// the [`Content`] it came from.
    ///
    /// Pair it with the document version sent to the server, and hand it back
    /// when the answer arrives. A server describes the document as it was when
    /// the request was made, and applying those positions to text that has
    /// since changed edits the wrong bytes.
    ///
    /// ```no_run
    /// # use matcha::Content;
    /// # let mut content = Content::with_text("fn main() {}");
    /// # let version = 0;
    /// let sent = (version, content.revision());
    /// // ... the server answers, and `sent.1` says whether the answer still fits.
    /// ```
    pub fn revision(&self) -> u64 {
        self.1
    }
}

impl Bridge<'_> {
    /// Converts a server position into the editor's, adjusting one that does
    /// not land exactly.
    ///
    /// A column past the end of its line becomes the end of that line. A column
    /// inside a character becomes the byte that character starts at. A line
    /// past the end of the buffer keeps its number and takes column 0, which
    /// the widget draws nothing for.
    ///
    /// This is the conversion decorations use. [`exact`](Self::exact) is the
    /// one edits use, where a position that does not land is an error rather
    /// than something to adjust.
    pub fn clamp(&self, from: Position) -> crate::Position {
        match self.resolve(from) {
            Ok(position) => position,
            // The line number passes through, and the column does not. Holding
            // the line keeps the position out of every visible row, so the
            // widget ignores it the way it ignores any line it does not have.
            // Moving it to the end of the buffer instead would put a squiggle
            // under whatever happens to be on the last line.
            Err(reason) => crate::Position {
                line: from.line as usize,
                index: clamped(&reason),
            },
        }
    }

    /// Converts a server position into the editor's, or `None` if it does not
    /// land on a byte of the text.
    ///
    /// `None` covers a line past the end of the buffer, a column past the end
    /// of its line, and a column inside a character.
    ///
    /// A line one past the last is not one of those: a server spells the end of
    /// the document that way, so it converts to the end of the last line and
    /// its column is ignored.
    pub fn exact(&self, from: Position) -> Option<crate::Position> {
        self.resolve(from).ok()
    }

    /// Converts an editor position into a server's, or `None` if the server
    /// cannot name it.
    ///
    /// `None` covers a line the buffer does not have, a byte past the end of
    /// its line, a byte inside a character, and a line number too large for the
    /// protocol's 32 bits.
    pub fn locate(&self, at: crate::Position) -> Option<Position> {
        let editor = self.content.0.borrow();
        let text = editor.line(at.line)?.text;

        if at.index > text.len() || !text.is_char_boundary(at.index) {
            return None;
        }

        let character = if text.is_ascii() {
            u32::try_from(at.index).ok()?
        } else {
            text[..at.index]
                .chars()
                .map(|character| self.encoding.width(character))
                .sum()
        };

        Some(Position {
            line: u32::try_from(at.line).ok()?,
            character,
        })
    }

    /// Converts a server range into the editor's, or `None` if either endpoint
    /// names a line past the end of the buffer.
    ///
    /// Both endpoints are clamped, and a range whose end precedes its start
    /// comes back with its endpoints in order.
    pub fn range(&self, from: Range) -> Option<TextRange> {
        let lines = self.content.0.borrow().line_count();

        // Both endpoints, not one: a range that starts on a line the buffer has
        // and ends past the end covers every row from the start onwards, so it
        // underlines the rest of the file.
        if from.start.line as usize > lines || from.end.line as usize > lines {
            return None;
        }

        Some(TextRange::new(self.clamp(from.start), self.clamp(from.end)))
    }

    /// Converts an editor range into a server's, or `None` if the server cannot
    /// name either endpoint.
    pub fn locate_range(&self, at: TextRange) -> Option<Range> {
        Some(Range {
            start: self.locate(at.start())?,
            end: self.locate(at.end())?,
        })
    }

    /// Converts diagnostics for [`CodeEditor::diagnostics`].
    ///
    /// The result has one entry per diagnostic, in the same order, so entry `n`
    /// answers diagnostic `n`. Nothing is dropped. That correspondence is worth
    /// relying on: the widget draws a squiggle and no text, so an application
    /// showing a message keeps these diagnostics alongside what it hands the
    /// widget, and a result that skipped one would put the two out of step.
    ///
    /// A diagnostic on a line the buffer no longer has still gets an entry, and
    /// that entry draws nothing.
    ///
    /// [`CodeEditor::diagnostics`]: crate::CodeEditor::diagnostics
    pub fn diagnostics(&self, from: &[Diagnostic]) -> Vec<decoration::diagnostic::Diagnostic> {
        let lines = self.content.0.borrow().line_count();
        let wanted: Vec<Position> = from
            .iter()
            .flat_map(|diagnostic| [diagnostic.range.start, diagnostic.range.end])
            .collect();
        let at = self.clamp_all(&wanted);

        from.iter()
            .enumerate()
            .map(|(index, diagnostic)| {
                let (start, end) = (diagnostic.range.start.line, diagnostic.range.end.line);

                let range = if start.max(end) as usize > lines {
                    // Both endpoints onto the line the buffer does not have, so
                    // the widget's own filter finds no row to draw on. Clamping
                    // them separately would leave a live start and an end at
                    // the bottom of the file, and underline everything between.
                    let past = crate::Position {
                        line: start.max(end) as usize,
                        index: 0,
                    };

                    TextRange::new(past, past)
                } else {
                    TextRange::new(at[index * 2], at[index * 2 + 1])
                };

                decoration::diagnostic::Diagnostic {
                    range,
                    severity: diagnostic.severity,
                }
            })
            .collect()
    }

    /// Converts inlay hints for [`CodeEditor::inlay_hints`].
    ///
    /// A hint whose label is empty is left out, so the result can be shorter
    /// than its input. That is the one thing dropped here, and it is about the
    /// label rather than the position: a chip is sized from its label, and an
    /// empty one paints a box over the code and shifts the next chip along the
    /// row without showing anything.
    ///
    /// Order is kept and nothing is sorted. The widget orders chips by where
    /// they end up on screen, which follows wrapped lines that a sort by line
    /// and column would not.
    ///
    /// [`CodeEditor::inlay_hints`]: crate::CodeEditor::inlay_hints
    pub fn hints(&self, from: &[Hint]) -> Vec<decoration::inlay::Hint<'static>> {
        let wanted: Vec<Position> = from.iter().map(|hint| hint.position).collect();
        let at = self.clamp_all(&wanted);

        from.iter()
            .zip(at)
            .filter(|(hint, _)| !hint.label.is_empty())
            .map(|(hint, position)| decoration::inlay::Hint {
                position,
                // Owned, so the labels outlive this call and satisfy the
                // widget's borrow of them. A borrow of `from` could not: an
                // application cannot hold the hints and the widget that borrows
                // them in one struct.
                label: hint.label.clone().into(),
            })
            .collect()
    }

    /// Clamps many positions, walking each line they name once.
    ///
    /// Converting them one at a time re-reads the line from its start every
    /// time, which a few thousand diagnostics on one long line turn into
    /// hundreds of megabytes of scanning.
    fn clamp_all(&self, wanted: &[Position]) -> Vec<crate::Position> {
        let mut order: Vec<usize> = (0..wanted.len()).collect();
        order.sort_unstable_by_key(|&index| (wanted[index].line, wanted[index].character));

        let mut out = vec![crate::Position { line: 0, index: 0 }; wanted.len()];
        let editor = self.content.0.borrow();
        let lines = editor.line_count();
        let mut rest = order.as_slice();

        while let Some(&first) = rest.first() {
            let line = wanted[first].line;
            let (here, tail) = rest.split_at(rest.partition_point(|&i| wanted[i].line == line));
            rest = tail;

            let Some(text) = editor.line(line as usize).map(|text| text.text) else {
                // The line after the last one is how a server spells the end of
                // the document. Anything past that keeps its number and loses
                // its column, as it does in `clamp`.
                let past = if line as usize == lines {
                    let last = lines.saturating_sub(1);

                    crate::Position {
                        line: last,
                        index: editor.line(last).map_or(0, |text| text.text.len()),
                    }
                } else {
                    crate::Position {
                        line: line as usize,
                        index: 0,
                    }
                };

                for &index in here {
                    out[index] = past;
                }

                continue;
            };

            let mut columns = Columns::new(&text, self.encoding);

            // Ascending, because `order` sorted them that way, so the line is
            // read once however many positions name it.
            for &index in here {
                out[index] = crate::Position {
                    line: line as usize,
                    index: columns
                        .byte_at(wanted[index].character)
                        .unwrap_or_else(|reason| clamped(&reason)),
                };
            }
        }

        out
    }

    /// The one scan of the line. [`clamp`](Self::clamp) and
    /// [`exact`](Self::exact) differ only in what they do with a [`Reason`].
    pub(crate) fn resolve(&self, from: Position) -> Result<crate::Position, Reason> {
        let editor = self.content.0.borrow();
        let lines = editor.line_count();
        let line = from.line as usize;

        // A server spells the end of the document as the line after the last
        // one, so a whole-file edit arrives as `0:0` to `line_count:0`. The
        // column is meaningless on a line that does not exist.
        if line == lines {
            let last = lines.saturating_sub(1);

            return Ok(crate::Position {
                line: last,
                index: editor.line(last).map_or(0, |text| text.text.len()),
            });
        }

        let text = editor.line(line).ok_or(Reason::Line)?.text;

        Ok(crate::Position {
            line,
            index: Columns::new(&text, self.encoding).byte_at(from.character)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::code_editor::decoration::diagnostic::Severity;

    /// The multibyte text the geometry tests use: two bytes per character, then
    /// three, then a grapheme cluster of twenty-five.
    const MULTIBYTE: &str = "héllo wörld\n日本語のテキスト\n👩‍👩‍👧‍👦 family";

    const ENCODINGS: [Encoding; 3] = [Encoding::Utf8, Encoding::Utf16, Encoding::Utf32];

    fn at(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    /// What `clamp` and `exact` make of one column of a one-line buffer.
    fn both(text: &str, character: u32, encoding: Encoding) -> (usize, Option<usize>) {
        let content = Content::with_text(text);
        let bridge = content.lsp(encoding);
        let from = at(0, character);

        (
            bridge.clamp(from).index,
            bridge.exact(from).map(|position| position.index),
        )
    }

    #[test]
    fn an_ascii_column_is_already_a_byte_offset() {
        for encoding in ENCODINGS {
            assert_eq!(both("hello", 3, encoding), (3, Some(3)));
        }
    }

    #[test]
    fn a_column_past_the_end_of_a_line_clamps_but_is_not_exact() {
        assert_eq!(both("hello", 99, Encoding::Utf16), (5, None));
        assert_eq!(both("", 7, Encoding::Utf16), (0, None));

        // A server is free to send this, and the arithmetic must not overflow
        // on the way to rejecting it.
        assert_eq!(both("hello", u32::MAX, Encoding::Utf16), (5, None));
    }

    #[test]
    fn the_column_at_the_end_of_a_line_is_exact() {
        // An edit that appends to a line names this column, so refusing it
        // would refuse every append. `clamp` cannot tell the difference -- it
        // answers with the end of the line either way -- so only `exact` shows
        // whether the column was accepted or merely pulled back.
        for encoding in ENCODINGS {
            assert_eq!(both("hello", 5, encoding), (5, Some(5)));
        }

        // The same column on a line the fast path does not take.
        assert_eq!(both("héllo", 5, Encoding::Utf16), (6, Some(6)));
        assert_eq!(both("héllo", 6, Encoding::Utf8), (6, Some(6)));
    }

    #[test]
    fn a_two_byte_character_costs_one_unit_in_every_encoding_but_utf8() {
        // "héllo": h=0, é=1..3, l=3.
        assert_eq!(both("héllo", 2, Encoding::Utf16), (3, Some(3)));
        assert_eq!(both("héllo", 2, Encoding::Utf32), (3, Some(3)));
        assert_eq!(both("héllo", 3, Encoding::Utf8), (3, Some(3)));
    }

    #[test]
    fn a_column_inside_a_character_clamps_to_where_that_character_starts() {
        // "a😀b": a=0, the emoji spans bytes 1..5, b=5. In UTF-16 the emoji is
        // a surrogate pair, so column 2 falls between its halves.
        assert_eq!(
            both("a😀b", 2, Encoding::Utf16),
            (1, None),
            "rounding up would step over the emoji the column points into"
        );

        // The same column in UTF-8 falls inside the é rather than between two
        // surrogates, and is refused for the same reason.
        assert_eq!(both("héllo", 2, Encoding::Utf8), (1, None));

        // A scalar value is indivisible, so UTF-32 cannot produce the case.
        assert_eq!(both("a😀b", 2, Encoding::Utf32), (5, Some(5)));
    }

    #[test]
    fn the_line_after_the_last_one_is_the_end_of_the_document() {
        let content = Content::with_text("a\nb");
        let bridge = content.lsp(Encoding::Utf16);

        // Whatever column it carries: the line does not exist to have columns.
        for character in [0, 1, 99] {
            let end = crate::Position { line: 1, index: 1 };

            assert_eq!(bridge.clamp(at(2, character)), end);
            assert_eq!(
                bridge.exact(at(2, character)),
                Some(end),
                "a whole-file edit arrives as 0:0 to line_count:0, so refusing \
                 this would refuse every reformat"
            );
        }
    }

    #[test]
    fn a_line_past_the_end_of_the_document_keeps_its_number_and_loses_its_column() {
        let content = Content::with_text("a\nb");
        let bridge = content.lsp(Encoding::Utf16);

        assert_eq!(
            bridge.clamp(at(4000, 3)),
            crate::Position {
                line: 4000,
                index: 0
            },
            "moving it onto a line that exists would draw it over that line"
        );
        assert_eq!(bridge.exact(at(4000, 3)), None);
    }

    #[test]
    fn a_range_with_either_endpoint_past_the_end_converts_to_nothing() {
        let content = Content::with_text("alpha\nbeta\ngamma");
        let bridge = content.lsp(Encoding::Utf16);

        let live = Range {
            start: at(0, 1),
            end: at(1, 2),
        };
        assert!(bridge.range(live).is_some());

        // The start is a line the buffer has. Converting the endpoints
        // separately would cover every row from it to the end of the file.
        assert!(
            bridge
                .range(Range {
                    start: at(0, 1),
                    end: at(4000, 0),
                })
                .is_none()
        );
        assert!(
            bridge
                .range(Range {
                    start: at(4000, 0),
                    end: at(4000, 1),
                })
                .is_none()
        );
    }

    #[test]
    fn a_range_whose_end_precedes_its_start_comes_back_in_order() {
        let content = Content::with_text("alpha beta");
        let bridge = content.lsp(Encoding::Utf16);

        let range = bridge
            .range(Range {
                start: at(0, 8),
                end: at(0, 2),
            })
            .expect("both endpoints are on a line the buffer has");

        assert_eq!(range.start(), crate::Position { line: 0, index: 2 });
        assert_eq!(range.end(), crate::Position { line: 0, index: 8 });
    }

    #[test]
    fn locate_refuses_a_position_the_protocol_cannot_name() {
        let content = Content::with_text("héllo");
        let bridge = content.lsp(Encoding::Utf16);

        assert_eq!(
            bridge.locate(crate::Position { line: 0, index: 3 }),
            Some(at(0, 2))
        );
        assert_eq!(bridge.locate(crate::Position { line: 9, index: 0 }), None);
        assert_eq!(
            bridge.locate(crate::Position { line: 0, index: 99 }),
            None,
            "a byte past the end of the line names nothing"
        );
        assert_eq!(
            bridge.locate(crate::Position { line: 0, index: 2 }),
            None,
            "byte 2 is inside the é"
        );
    }

    #[test]
    fn every_byte_of_multibyte_text_survives_a_round_trip_in_every_encoding() {
        let content = Content::with_text(MULTIBYTE);

        for encoding in ENCODINGS {
            let bridge = content.lsp(encoding);

            for line in 0..content.line_count() {
                let text = content.line(line).expect("the line is in range");

                for (index, _) in text.char_indices().chain([(text.len(), ' ')]) {
                    let from = crate::Position { line, index };
                    let named = bridge.locate(from).expect("a char boundary is nameable");

                    assert_eq!(
                        bridge.clamp(named),
                        from,
                        "{encoding:?} lost {from:?} of {text:?}"
                    );
                }
            }
        }
    }

    fn diagnostic(range: Range, severity: Severity) -> Diagnostic {
        Diagnostic {
            range,
            severity,
            message: String::new(),
            source: None,
            code: None,
            code_description: None,
            tags: Vec::new(),
            related: Vec::new(),
            data: None,
        }
    }

    fn hint(line: u32, character: u32, label: &str) -> Hint {
        Hint {
            position: at(line, character),
            label: label.to_owned(),
            kind: None,
            padding_left: false,
            padding_right: false,
            tooltip: None,
            text_edits: Vec::new(),
            data: None,
        }
    }

    /// The one property that makes a second, faster path safe to have.
    #[test]
    fn converting_a_batch_answers_exactly_what_converting_one_at_a_time_does() {
        let content = Content::with_text(MULTIBYTE);

        // Every column of every line, a few past the end of each, the line a
        // server uses for the end of the document, and one well past it.
        let mut wanted = Vec::new();
        for line in 0..=4 {
            for character in 0..30 {
                wanted.push(at(line, character));
            }
        }
        wanted.push(at(0, u32::MAX));

        for encoding in ENCODINGS {
            let bridge = content.lsp(encoding);
            let one_at_a_time: Vec<_> = wanted.iter().map(|&p| bridge.clamp(p)).collect();

            assert_eq!(bridge.clamp_all(&wanted), one_at_a_time, "{encoding:?}");

            // The batch sorts internally, so the order it is handed must not
            // change any answer.
            let mut backwards = wanted.clone();
            backwards.reverse();

            let mut expected = one_at_a_time.clone();
            expected.reverse();

            assert_eq!(bridge.clamp_all(&backwards), expected, "{encoding:?}");
        }
    }

    #[test]
    fn a_line_is_read_again_when_its_columns_are_asked_for_out_of_order() {
        let mut columns = Columns::new("a😀b", Encoding::Utf16);

        assert_eq!(columns.byte_at(3), Ok(5));
        assert_eq!(
            columns.byte_at(1),
            Ok(1),
            "walking backwards restarts rather than answering from the middle"
        );
        assert_eq!(columns.byte_at(3), Ok(5));
    }

    #[test]
    fn a_later_column_never_resolves_to_an_earlier_byte() {
        let text = MULTIBYTE.lines().next().expect("the fixture has a line");
        let mut columns = Columns::new(text, Encoding::Utf16);
        let mut last = 0;

        for character in 0..12 {
            let index = columns.byte_at(character).unwrap_or(text.len());

            assert!(index >= last, "column {character} went backwards");
            last = index;
        }
    }

    #[test]
    fn every_diagnostic_gets_an_entry_and_keeps_its_place() {
        let content = Content::with_text(
            "alpha
beta
gamma",
        );
        let bridge = content.lsp(Encoding::Utf16);

        let from = [
            diagnostic(
                Range {
                    start: at(0, 1),
                    end: at(0, 3),
                },
                Severity::Warning,
            ),
            // On a line the buffer does not have.
            diagnostic(
                Range {
                    start: at(4000, 0),
                    end: at(4000, 2),
                },
                Severity::Error,
            ),
            diagnostic(
                Range {
                    start: at(2, 0),
                    end: at(2, 5),
                },
                Severity::Hint,
            ),
        ];

        let converted = bridge.diagnostics(&from);

        assert_eq!(
            converted.len(),
            from.len(),
            "an application keeps its own diagnostics alongside these, and \
             matches them up by position in the list"
        );
        assert_eq!(converted[0].severity, Severity::Warning);
        assert_eq!(converted[1].severity, Severity::Error);
        assert_eq!(converted[2].severity, Severity::Hint);
    }

    #[test]
    fn a_diagnostic_that_outruns_the_buffer_covers_no_line_that_exists() {
        let content = Content::with_text(
            "alpha
beta
gamma",
        );
        let bridge = content.lsp(Encoding::Utf16);

        // A live start and a stale end. Clamping the two separately would put
        // the end at the bottom of the file and underline everything between.
        let converted = bridge.diagnostics(&[diagnostic(
            Range {
                start: at(0, 1),
                end: at(4000, 0),
            },
            Severity::Error,
        )]);

        let range = converted[0].range;

        assert_eq!(range.start(), range.end());
        assert!(
            range.start().line >= content.line_count(),
            "both endpoints have to sit off the end, or the widget draws it"
        );
    }

    #[test]
    fn a_hint_without_a_label_is_left_out_and_the_rest_keep_their_order() {
        let content = Content::with_text(
            "alpha
beta",
        );
        let bridge = content.lsp(Encoding::Utf16);

        // Deliberately not in position order: the widget sorts by where a chip
        // lands on screen, which follows wrapping, so sorting here would only
        // break the correspondence with the labels.
        let converted = bridge.hints(&[hint(1, 2, "second"), hint(0, 1, "first"), hint(0, 3, "")]);

        assert_eq!(
            converted.len(),
            2,
            "the empty label is the only one dropped"
        );
        assert_eq!(converted[0].label, "second");
        assert_eq!(converted[1].label, "first");
    }

    #[test]
    fn a_hint_on_a_line_the_buffer_lost_is_still_converted() {
        let content = Content::with_text("alpha");
        let bridge = content.lsp(Encoding::Utf16);

        let converted = bridge.hints(&[hint(4000, 0, "stale")]);

        assert_eq!(converted.len(), 1, "only an empty label is dropped");
        assert!(converted[0].position.line >= content.line_count());
    }

    #[test]
    fn a_range_round_trips_through_the_servers_coordinates() {
        let content = Content::with_text(MULTIBYTE);
        let bridge = content.lsp(Encoding::Utf16);

        let from = TextRange::new(
            crate::Position { line: 0, index: 1 },
            crate::Position { line: 2, index: 25 },
        );

        let named = bridge.locate_range(from).expect("both endpoints are bytes");

        assert_eq!(bridge.range(named), Some(from));
    }
}
