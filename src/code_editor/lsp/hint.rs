//! Labels a language server wants shown inside a line.

use super::{Position, Replacement};

/// A label a language server wants shown inside a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    /// Where to anchor it.
    pub position: Position,
    /// The label.
    ///
    /// A server may send this in parts, each of which can carry a location of
    /// its own. The parts are joined and those locations dropped, so a type
    /// hint is text rather than something to follow.
    pub label: String,
    /// What the label annotates.
    pub kind: Option<Kind>,
    /// Whether the server asked for a space before the label.
    ///
    /// Carried for sending back, and it does not change what is drawn: the chip
    /// already stands clear of the character it annotates.
    pub padding_left: bool,
    /// Whether the server asked for a space after the label.
    pub padding_right: bool,
    /// A longer description of the label.
    ///
    /// matcha has nowhere to show this. It is here for the application.
    pub tooltip: Option<String>,
    /// The edits that turn the label into text of its own.
    ///
    /// This is what accepting a hint means: a `: i32` chip becomes a real type
    /// annotation. Hand them to [`Content::apply`](crate::Content::apply).
    pub text_edits: Vec<Replacement>,
    /// Whatever the server attached, as the JSON text it arrived as.
    ///
    /// Send it back unchanged to ask the server to fill in a hint it sent
    /// without a tooltip or without edits.
    pub data: Option<String>,
}

/// What a hint annotates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The type of an expression or a binding.
    Type,
    /// The name of a parameter at a call site.
    Parameter,
}
