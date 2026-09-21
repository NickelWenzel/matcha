//! Why a batch of edits was refused.

use std::fmt;

use super::Position;

/// Why [`Content::apply`](crate::Content::apply) refused a batch of edits.
///
/// Every variant but [`Stale`](Error::Stale) names the edit it is about by
/// position in the list that was passed in, because a batch from a code action
/// can be dozens long and "one of them is wrong" is not something an
/// application can report or recover from.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The buffer was edited after the revision the caller passed.
    Stale {
        /// The revision the edits were computed against.
        expected: u64,
        /// The revision the buffer is at now.
        actual: u64,
    },
    /// An edit named a line past the end of the buffer.
    LineOutOfBounds {
        /// Which edit, by position in the list.
        edit: usize,
        /// The line it named.
        line: u32,
        /// How many lines the buffer has.
        lines: usize,
    },
    /// An edit named a column past the end of its line.
    ///
    /// Decorations clamp this and edits do not: clamping turns "replace columns
    /// 10 to 20" into an insert of nothing at the end of the line, which is not
    /// what the server asked for and leaves no sign that anything went wrong.
    ColumnOutOfBounds {
        /// Which edit, by position in the list.
        edit: usize,
        /// The column it named.
        character: u32,
        /// How many bytes that line holds.
        line_len: usize,
    },
    /// An edit named a column inside a character.
    NotACharBoundary {
        /// Which edit, by position in the list.
        edit: usize,
        /// The position it named.
        position: Position,
    },
    /// Two edits cover some of the same text.
    Overlapping {
        /// Which edit, by position in the list.
        edit: usize,
        /// The edit it overlaps, by position in the list.
        other: usize,
    },
    /// An edit's range ends before it starts.
    ///
    /// Refused rather than repaired, because both repairs change what was
    /// asked for: swapping the endpoints deletes text the server did not name,
    /// and collapsing them turns a replacement into an insertion.
    /// [`Range::collapsed`](crate::lsp::Range::collapsed) is the repair servers
    /// expect, for an application that wants to apply it.
    ReversedRange {
        /// Which edit, by position in the list.
        edit: usize,
    },
    /// An edit was written in snippet syntax.
    Unsupported {
        /// Which edit, by position in the list.
        edit: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Stale { expected, actual } => write!(
                f,
                "the edits were computed against revision {expected} and the buffer is at {actual}"
            ),
            Error::LineOutOfBounds { edit, line, lines } => write!(
                f,
                "edit {edit} names line {line} of a buffer with {lines} lines"
            ),
            Error::ColumnOutOfBounds {
                edit,
                character,
                line_len,
            } => write!(
                f,
                "edit {edit} names column {character} of a line {line_len} bytes long"
            ),
            Error::NotACharBoundary { edit, position } => write!(
                f,
                "edit {edit} names column {} of line {}, which is inside a character",
                position.character, position.line
            ),
            Error::Overlapping { edit, other } => {
                write!(f, "edit {edit} covers text that edit {other} also covers")
            }
            Error::ReversedRange { edit } => {
                write!(f, "edit {edit} ends before it starts")
            }
            Error::Unsupported { edit } => {
                write!(f, "edit {edit} is a snippet, which matcha cannot apply")
            }
        }
    }
}

impl std::error::Error for Error {}
