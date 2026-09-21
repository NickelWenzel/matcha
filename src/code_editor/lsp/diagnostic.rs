//! Problems a language server reported.

use crate::code_editor::decoration::diagnostic::Severity;

use super::Range;

/// A problem a language server reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Where the problem is.
    pub range: Range,
    /// How severe it is.
    ///
    /// A server may leave this out, and may send a number outside the four the
    /// protocol names. Both become [`Severity::Error`]: these severities differ
    /// only in the colour of a squiggle, so reporting an error as a warning is
    /// the worse of the two mistakes.
    pub severity: Severity,
    /// What the server said about it.
    ///
    /// The widget draws a squiggle and no text, so this is here for the
    /// application to show beside the code or on hover.
    pub message: String,
    /// Which tool reported it, such as `rustc` or `clippy`.
    ///
    /// This is how a reader tells three sources apart in one gutter, and part
    /// of what a server matches on when a diagnostic is sent back to it.
    pub source: Option<String>,
    /// The error code, as the server spelled it.
    pub code: Option<Code>,
    /// A page documenting the code.
    pub code_description: Option<String>,
    /// What the server wants done with the marked code.
    pub tags: Vec<Tag>,
    /// Other places implicated in the problem.
    pub related: Vec<Related>,
    /// Whatever the server attached, as the JSON text it arrived as.
    ///
    /// Nothing here reads it, which is why this module needs no JSON parser.
    /// Send it back unchanged when asking for code actions: a server that put
    /// its own identifiers here cannot resolve a fix without them, and
    /// rust-analyzer is one that does.
    pub data: Option<String>,
}

/// An error code, which a server may number or name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Code {
    /// A numeric code.
    Number(i32),
    /// A named code, such as `unused_variables`.
    Text(String),
}

/// What a server wants done with the code a diagnostic marks.
///
/// Carried and never drawn. Dimming and striking through are effects on the
/// text itself, and the widget draws a diagnostic as a squiggle under text it
/// does not otherwise touch. An application that wants either applies it in its
/// own interface, or sends the diagnostic back with this intact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    /// The code is unreachable or unused.
    Unnecessary,
    /// The code is deprecated.
    Deprecated,
}

/// Another place implicated in a diagnostic.
///
/// Kept whole, including the document it names. matcha has no notion of a file,
/// so it cannot tell a location in this buffer from one in another, and the
/// application already knows which document it opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Related {
    /// The document, as the server spelled it.
    pub uri: String,
    /// Where in that document.
    pub range: Range,
    /// What the server said about it.
    pub message: String,
}
