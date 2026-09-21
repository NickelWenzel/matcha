//! Positions as a language server counts them.

/// A position in a language server's coordinates.
///
/// The `character` is counted in whatever [`Encoding`] the server negotiated —
/// UTF-16 code units unless it said otherwise — which is why this is a
/// different type from [`matcha::Position`], whose `index` is a UTF-8 byte
/// offset into the line. Converting between the two needs the text of the line,
/// so it is the bridge's job rather than a [`From`] impl.
///
/// Keeping them distinct is what makes a position that came off the wire
/// unusable as an index until it has been converted: the compiler enforces
/// what a comment would only ask for.
///
/// [`Encoding`]: super::Encoding
/// [`matcha::Position`]: crate::Position
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    /// Zero-based line number.
    ///
    /// Declared before `character` so that the derived [`Ord`] compares lines
    /// first, which is the order a batch of text edits has to be sorted in.
    pub line: u32,
    /// Zero-based offset into the line, in the negotiated encoding's code
    /// units.
    pub character: u32,
}

/// A range of text in a language server's coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Range {
    /// Where the range starts.
    pub start: Position,
    /// Where it ends, exclusive.
    pub end: Position,
}

impl Range {
    /// The range with its start moved to its end.
    ///
    /// Unconditional: it does not test whether the range is reversed, so
    /// calling it on a well-formed one collapses that range to an insertion
    /// point. The caller checks `end < start` first.
    ///
    /// This is the repair a server expects for a range whose end precedes its
    /// start — the protocol leaves the case undefined, VS Code caps the start
    /// to the end, and enough servers rely on that for it to be the de-facto
    /// answer. It is deliberately **not** a swap: swapping would delete
    /// everything between two endpoints the server meant to collapse.
    pub fn collapsed(self) -> Self {
        Self {
            start: self.end,
            end: self.end,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    #[test]
    fn a_collapsed_range_becomes_an_insert_at_its_end() {
        let range = Range {
            start: at(0, 4),
            end: at(0, 9),
        };

        let collapsed = range.collapsed();

        assert_eq!(collapsed.start, collapsed.end);
        assert_eq!(
            collapsed.start, range.end,
            "an insert at the end, never a swap: a swap would delete the range"
        );
    }

    #[test]
    fn a_position_orders_by_line_before_column() {
        assert!(
            at(1, 0) > at(0, 99),
            "a batch of edits is sorted by this ordering, so lines must win"
        );
    }
}
