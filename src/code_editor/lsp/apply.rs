//! Applying a server's edits to the text.

use std::sync::Arc;

use iced::advanced::text::editor::{self, LineEnding};

use super::bridge::{Bridge, Reason};
use super::{Change, Encoding, Error, Position, document};
use crate::code_editor::content::Content;

/// An edit with its range resolved against the text and its line endings
/// matched to the buffer's.
struct Planned {
    /// Where it sat in the list that was passed in, for naming it in an error.
    edit: usize,
    start: crate::Position,
    end: crate::Position,
    text: String,
}

impl Content {
    /// Applies one document's edits.
    ///
    /// All of them or none. Every edit is checked before any is made, so a
    /// refusal leaves the text exactly as it was. This is the opposite of what
    /// decorations do, and deliberately: a squiggle in the wrong place is gone
    /// by the next keystroke, and half an applied code action is a file the
    /// user has to repair by hand.
    ///
    /// `expected` is a [`revision`](Content::revision) taken when the request
    /// went out. The edits describe the text as it was then, so a buffer that
    /// has moved since is refused rather than edited in the wrong places.
    /// The new revision comes back, ready to pair with the next `didChange`.
    ///
    /// Line endings in the inserted text are matched to the buffer's. A server
    /// that knows the file is DOS already sends `\r\n`, so this most often
    /// changes nothing, and it is what stops the one that does not from
    /// leaving a file with both.
    ///
    /// The caret is not where it was. It ends up at the last edit applied, and
    /// the view scrolls to it.
    ///
    /// # Errors
    ///
    /// Every [`Error`] but [`Stale`](Error::Stale) names the edit it is about
    /// by position in `edit.edits`.
    pub fn apply(
        &mut self,
        edit: &document::Edit,
        encoding: Encoding,
        expected: u64,
    ) -> Result<u64, Error> {
        if self.1 != expected {
            return Err(Error::Stale {
                expected,
                actual: self.1,
            });
        }

        let mut plan = Vec::with_capacity(edit.edits.len());

        // Scoped, because the bridge borrows this content and committing needs
        // it back. `RefCell::borrow_mut` takes `&self`, so a bridge still alive
        // below would not fail to compile -- it would panic at run time.
        {
            let lines = self.line_count();
            let ending = match self.line_ending() {
                // A buffer of one line with nothing after it says nothing about
                // the file's convention. Its ending reads as the empty string,
                // so rejoining against it would delete every line break in the
                // inserted text, and assuming `\n` instead would rewrite a DOS
                // file's endings. With no evidence, leave the text alone.
                None | Some(LineEnding::None) => None,
                Some(ending) => Some(ending),
            };
            let bridge = self.lsp(encoding);

            for (index, change) in edit.edits.iter().enumerate() {
                let replacement = match change {
                    Change::Replace(replacement) => replacement,
                    Change::Snippet(_) => return Err(Error::Unsupported { edit: index }),
                };

                let start = resolved(&bridge, replacement.range.start, index, lines)?;
                let end = resolved(&bridge, replacement.range.end, index, lines)?;

                if start > end {
                    return Err(Error::ReversedRange { edit: index });
                }

                plan.push(Planned {
                    edit: index,
                    start,
                    end,
                    text: match ending {
                        Some(ending) => rejoined(&replacement.new_text, ending),
                        None => replacement.new_text.clone(),
                    },
                });
            }
        }

        // By start, then by end, and stably. Sorting by start alone puts an
        // insertion after a replacement that begins at the same place, which
        // then runs against text the insertion has already moved. Equal keys
        // keep the order they arrived in, which is the order the protocol says
        // several insertions at one position appear in.
        plan.sort_by_key(|step| (step.start, step.end));

        let mut covered = crate::Position { line: 0, index: 0 };
        let mut owner = 0;

        for step in &plan {
            // Strict. Several edits may insert at one position, and each of
            // those starts where it ends, so testing for equality too would
            // refuse the one case the protocol spells out.
            if step.start < covered {
                return Err(Error::Overlapping {
                    edit: step.edit,
                    other: owner,
                });
            }

            // Sorted by start and then by end, and no range runs backwards,
            // so this only ever moves forward.
            covered = step.end;
            owner = step.edit;
        }

        // Last one first, so that every edit still ahead of the caret is
        // described in coordinates the earlier ones have not moved. It also
        // leaves the topmost edit applied last, which is the one the
        // highlighter re-reads from.
        for step in plan.into_iter().rev() {
            self.move_to(editor::Cursor {
                position: step.start,
                selection: Some(step.end),
            });
            self.perform(editor::Action::Edit(editor::Edit::Paste(Arc::new(
                step.text,
            ))));
        }

        Ok(self.1)
    }
}

/// Converts one endpoint, naming the edit it came from if it will not convert.
fn resolved(
    bridge: &Bridge<'_>,
    from: Position,
    edit: usize,
    lines: usize,
) -> Result<crate::Position, Error> {
    bridge.resolve(from).map_err(|reason| match reason {
        Reason::Line => Error::LineOutOfBounds {
            edit,
            line: from.line,
            lines,
        },
        Reason::Column { length } => Error::ColumnOutOfBounds {
            edit,
            character: from.character,
            line_len: length,
        },
        Reason::NotACharBoundary { .. } => Error::NotACharBoundary {
            edit,
            position: from,
        },
    })
}

/// `text` with its line endings replaced by `ending`.
///
/// The protocol ends a line with `\r\n`, `\n` or `\r`, so all three are
/// replaced. Replacing rather than inserting is what makes this safe to run on
/// text that already ends its lines the way the buffer does, which is the
/// common case: a server that knows the file is DOS sends `\r\n` already.
fn rejoined(text: &str, ending: LineEnding) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(index) = rest.find(['\r', '\n']) {
        out.push_str(&rest[..index]);
        out.push_str(ending.as_str());

        rest = if rest[index..].starts_with("\r\n") {
            &rest[index + 2..]
        } else {
            &rest[index + 1..]
        };
    }

    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::lsp::{Range, Replacement, Snippet};

    fn at(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    fn replace(start: (u32, u32), end: (u32, u32), text: &str) -> Change {
        Change::Replace(Replacement {
            range: Range {
                start: at(start.0, start.1),
                end: at(end.0, end.1),
            },
            new_text: text.to_owned(),
            annotation_id: None,
        })
    }

    fn document(changes: Vec<Change>) -> document::Edit {
        document::Edit {
            uri: "file:///x.rs".to_owned(),
            version: None,
            edits: changes,
        }
    }

    fn applied(text: &str, changes: Vec<Change>) -> (String, Result<u64, Error>) {
        let mut content = Content::with_text(text);
        let outcome = content.apply(&document(changes), Encoding::Utf16, 0);

        (content.text(), outcome)
    }

    /// The text after a batch that has to succeed.
    fn text_after(text: &str, changes: Vec<Change>) -> String {
        let (after, outcome) = applied(text, changes);

        outcome.expect("the batch should have applied");
        after
    }

    /// The error from a batch that has to be refused, having checked that
    /// refusing it left the text alone. Every refusal test goes through here,
    /// because an error that still edited the buffer is the failure this phase
    /// exists to prevent.
    fn refused(text: &str, changes: Vec<Change>) -> Error {
        let (after, outcome) = applied(text, changes);

        assert_eq!(after, text, "a refused batch must change nothing");
        outcome.expect_err("the batch should have been refused")
    }

    #[test]
    fn one_replacement_lands() {
        assert_eq!(
            text_after("let x = 1;", vec![replace((0, 4), (0, 5), "answer")]),
            "let answer = 1;"
        );
    }

    #[test]
    fn edits_land_wherever_they_are_in_the_list() {
        let changes = vec![
            replace((0, 4), (0, 5), "xray"),
            replace((1, 4), (1, 5), "yankee"),
            replace((2, 4), (2, 5), "zulu"),
        ];

        assert_eq!(
            text_after("let x = 1;\nlet y = 2;\nlet z = 3;", changes),
            "let xray = 1;\nlet yankee = 2;\nlet zulu = 3;"
        );
    }

    #[test]
    fn replacing_with_nothing_deletes_the_range() {
        assert_eq!(
            text_after("let x = 1;", vec![replace((0, 0), (0, 4), "")]),
            "x = 1;"
        );
    }

    #[test]
    fn inserted_text_can_carry_line_breaks_of_its_own() {
        let mut content = Content::with_text("a\nb");

        content
            .apply(
                &document(vec![replace((0, 1), (0, 1), "\nMIDDLE\n")]),
                Encoding::Utf16,
                0,
            )
            .expect("the batch should have applied");

        assert_eq!(content.text(), "a\nMIDDLE\n\nb");
        assert_eq!(content.line_count(), 4);
    }

    #[test]
    fn a_range_across_a_line_break_joins_the_lines() {
        assert_eq!(
            text_after("alpha\nbeta\ngamma", vec![replace((0, 3), (2, 1), "X")]),
            "alpXamma"
        );
    }

    #[test]
    fn the_break_between_two_lines_is_a_range_like_any_other() {
        assert_eq!(
            text_after("one\ntwo", vec![replace((0, 3), (1, 0), "")]),
            "onetwo"
        );
    }

    #[test]
    fn a_multibyte_range_is_measured_in_the_servers_units() {
        // "héllo wörld": the ö is the eighth character and the ninth byte.
        assert_eq!(
            text_after("héllo wörld", vec![replace((0, 7), (0, 8), "o")]),
            "héllo world"
        );
    }

    #[test]
    fn several_insertions_at_one_position_arrive_in_the_order_they_were_sent() {
        let changes = vec![
            replace((0, 1), (0, 1), "A"),
            replace((0, 1), (0, 1), "B"),
            replace((0, 1), (0, 1), "C"),
        ];

        assert_eq!(
            text_after("XY", changes),
            "XABCY",
            "sorting so that equal positions lose their order reverses them"
        );
    }

    #[test]
    fn an_insertion_and_a_replacement_at_one_position_both_land() {
        // The protocol allows any number of insertions followed by one
        // replacement at the same place, and says the list need not be sorted.
        // Ordering by where an edit starts and ignoring where it ends puts the
        // insertion second, against text the replacement has already moved.
        let canonical = vec![replace((0, 0), (0, 2), "Q"), replace((0, 0), (0, 0), "A")];
        let reversed = vec![replace((0, 0), (0, 0), "A"), replace((0, 0), (0, 2), "Q")];

        assert_eq!(text_after("XYZ", canonical), "AQZ");
        assert_eq!(text_after("XYZ", reversed), "AQZ");
    }

    #[test]
    fn inserted_line_breaks_match_the_buffers() {
        assert_eq!(
            text_after("a\r\nb\r\nc", vec![replace((1, 0), (1, 1), "X\nY")]),
            "a\r\nX\r\nY\r\nc",
            "a server that sends bare newlines must not leave the file with both"
        );
    }

    #[test]
    fn text_that_already_matches_the_buffer_is_left_as_it_is() {
        // The common case: a server that knows the file is DOS sends \r\n
        // already. Replacing a newline by appending to it would give \r\r\n.
        assert_eq!(
            text_after("a\r\nb\r\nc", vec![replace((1, 0), (1, 1), "X\r\nY")]),
            "a\r\nX\r\nY\r\nc"
        );
    }

    #[test]
    fn a_buffer_that_shows_no_convention_has_its_text_inserted_as_it_stands() {
        // `Content::new` and every single-line buffer report no line ending,
        // which reads as the empty string. Rejoining against that deletes every
        // line break in the inserted text.
        let mut content = Content::new();

        content
            .apply(
                &document(vec![replace((0, 0), (0, 0), "a\nb")]),
                Encoding::Utf16,
                0,
            )
            .expect("the batch should have applied");

        assert_eq!(content.text(), "a\nb");

        // The case that catches a guess at the convention rather than no
        // guess: assuming `\n` here would rewrite a DOS file's endings, and
        // nothing afterwards would show that it had happened.
        let mut dos = Content::new();

        dos.apply(
            &document(vec![replace((0, 0), (0, 0), "a\r\nb")]),
            Encoding::Utf16,
            0,
        )
        .expect("the batch should have applied");

        assert_eq!(dos.text(), "a\r\nb");

        assert_eq!(
            text_after("one line", vec![replace((0, 8), (0, 8), "\nsecond")]),
            "one line\nsecond"
        );
    }

    #[test]
    fn a_range_that_ends_before_it_starts_is_refused() {
        assert_eq!(
            refused("alpha beta", vec![replace((0, 8), (0, 2), "x")]),
            Error::ReversedRange { edit: 0 }
        );
    }

    #[test]
    fn a_column_past_the_end_of_its_line_is_refused() {
        // Decorations clamp this. An edit must not: clamping turns a
        // replacement into an insertion of nothing at the end of the line.
        assert_eq!(
            refused("alpha", vec![replace((0, 2), (0, 40), "x")]),
            Error::ColumnOutOfBounds {
                edit: 0,
                character: 40,
                line_len: 5,
            }
        );
    }

    #[test]
    fn a_column_inside_a_character_is_refused() {
        assert_eq!(
            refused("a😀b", vec![replace((0, 2), (0, 3), "x")]),
            Error::NotACharBoundary {
                edit: 0,
                position: at(0, 2),
            }
        );
    }

    #[test]
    fn a_line_past_the_end_of_the_buffer_is_refused() {
        assert_eq!(
            refused("alpha\nbeta", vec![replace((4000, 0), (4000, 1), "x")]),
            Error::LineOutOfBounds {
                edit: 0,
                line: 4000,
                lines: 2,
            }
        );
    }

    #[test]
    fn the_line_after_the_last_one_appends_to_the_document() {
        // How a server spells the end of the document, and what every
        // whole-file reformat is expressed as.
        assert_eq!(
            text_after("alpha\nbeta", vec![replace((2, 0), (2, 0), "!")]),
            "alpha\nbeta!"
        );
    }

    #[test]
    fn overlapping_edits_are_refused_and_both_are_named() {
        let changes = vec![replace((0, 0), (0, 5), "x"), replace((0, 3), (0, 8), "y")];

        assert_eq!(
            refused("alpha beta", changes),
            Error::Overlapping { edit: 1, other: 0 }
        );
    }

    #[test]
    fn a_snippet_refuses_the_whole_batch() {
        let changes = vec![
            replace((0, 0), (0, 1), "x"),
            Change::Snippet(Snippet {
                range: Range {
                    start: at(0, 2),
                    end: at(0, 3),
                },
                value: "${1:name}$0".to_owned(),
                annotation_id: None,
            }),
        ];

        assert_eq!(
            refused("alpha", changes),
            Error::Unsupported { edit: 1 },
            "applying the rest would leave the edit half made"
        );
    }

    #[test]
    fn a_buffer_edited_since_the_request_is_refused() {
        let mut content = Content::with_text("alpha");

        content.perform(editor::Action::Edit(editor::Edit::Insert('x')));

        let outcome = content.apply(
            &document(vec![replace((0, 0), (0, 1), "y")]),
            Encoding::Utf16,
            0,
        );

        assert_eq!(
            outcome,
            Err(Error::Stale {
                expected: 0,
                actual: 1
            })
        );
        assert_eq!(content.text(), "xalpha", "the edit must not have been made");
    }

    #[test]
    fn a_batch_of_nothing_changes_nothing() {
        let mut content = Content::with_text("alpha");

        assert_eq!(
            content.apply(&document(Vec::new()), Encoding::Utf16, 0),
            Ok(0)
        );
        assert_eq!(content.text(), "alpha");
        assert_eq!(
            content.revision(),
            0,
            "nothing was edited, so there is nothing to undo either"
        );
    }

    #[test]
    fn the_revision_moves_once_for_every_edit_made() {
        let mut content = Content::with_text("let x = 1;\nlet y = 2;\nlet z = 3;");

        let after = content
            .apply(
                &document(vec![
                    replace((0, 4), (0, 5), "a"),
                    replace((1, 4), (1, 5), "b"),
                    replace((2, 4), (2, 5), "c"),
                ]),
                Encoding::Utf16,
                0,
            )
            .expect("the batch should have applied");

        assert_eq!(after, 3);
        assert_eq!(content.revision(), after);
    }

    #[test]
    fn a_boundary_inside_a_grapheme_cluster_neither_panics_nor_corrupts() {
        // The family emoji is one cluster of seven characters and twenty-five
        // bytes. Columns 0 to 2 are the first of them, which is a character
        // boundary inside a cluster: legal to slice at, and a mangled cluster
        // afterwards is the server's business rather than a panic here.
        let after = text_after("👩‍👩‍👧‍👦 family", vec![replace((0, 0), (0, 2), "X")]);

        assert!(after.starts_with('X'));
        assert!(after.ends_with(" family"));
    }
}
