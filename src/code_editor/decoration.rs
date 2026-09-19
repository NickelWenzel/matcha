//! The things a code editor draws on top of its text.

pub mod diagnostic;
pub mod inlay;

use crate::Position;

/// A half-open range of text, in UTF-8 byte positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    start: Position,
    end: Position,
}

impl TextRange {
    /// Creates a [`TextRange`] covering both positions, in either order.
    pub fn new(a: Position, b: Position) -> Self {
        // An end before its start does not make `cosmic_text`'s `highlight` return nothing; it
        // makes it return a span covering the whole run. Ordering here, behind private fields,
        // is what keeps that unreachable.
        if a <= b {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }

    /// The earlier endpoint.
    pub fn start(self) -> Position {
        self.start
    }

    /// The later endpoint.
    pub fn end(self) -> Position {
        self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_orders_its_endpoints() {
        let first = Position { line: 1, index: 2 };
        let second = Position { line: 3, index: 0 };

        let forward = TextRange::new(first, second);
        let backward = TextRange::new(second, first);

        assert_eq!(forward, backward);
        assert_eq!(forward.start(), first);
        assert_eq!(forward.end(), second);
    }

    #[test]
    fn a_range_orders_by_index_within_a_line() {
        let range = TextRange::new(
            Position { line: 4, index: 9 },
            Position { line: 4, index: 1 },
        );

        assert_eq!(range.start(), Position { line: 4, index: 1 });
        assert_eq!(range.end(), Position { line: 4, index: 9 });
    }
}
