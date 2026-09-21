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

/// One entry in a document's list of edits.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// An ordinary replacement.
    Replace(Replacement),
    /// A snippet, which matcha will not apply.
    Snippet(Snippet),
}

/// A replacement written in snippet syntax.
///
/// Kept rather than dropped, and refused rather than applied. Its text is a
/// template: `$0` marks where the cursor should end up and `${1:name}` marks a
/// field to fill in. Inserting it as it stands would put those markers in the
/// user's file. An application that supports snippets expands one itself and
/// applies the result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    /// The text it would replace.
    pub range: Range,
    /// Snippet syntax, not literal text.
    pub value: String,
    /// The change annotation this belongs to.
    pub annotation_id: Option<String>,
}
