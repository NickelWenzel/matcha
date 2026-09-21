//! A range of text and what replaces it.

use super::Range;

/// A range of text and what replaces it.
///
/// This is what a language server calls a `TextEdit`. Its range is in the
/// server's coordinates, so it is converted against the buffer it applies to
/// rather than being read directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    /// The text to replace.
    pub range: Range,
    /// The text that replaces it.
    ///
    /// Line endings are the server's, and are matched to the buffer's when the
    /// replacement is applied.
    pub new_text: String,
    /// The change annotation this belongs to.
    ///
    /// An annotation can mark a change as needing confirmation. matcha never
    /// asks, so an application that honours one groups replacements by this
    /// before applying them.
    pub annotation_id: Option<String>,
}
