//! One document's worth of edits.

use super::Change;

/// The edits a language server wants made to one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// The document, as the server spelled it.
    ///
    /// matcha never reads this. It is how the application decides which buffer
    /// the edits belong to.
    pub uri: String,
    /// The version the server computed these against, if it said.
    ///
    /// [`Content::apply`](crate::Content::apply) ignores it and takes a
    /// [`revision`](crate::Content::revision) instead: matcha does not send
    /// `didChange`, so only the application knows which of its versions a
    /// buffer is at.
    pub version: Option<i32>,
    /// The edits, in the order the server listed them.
    ///
    /// Order matters when several of them insert at one position: the text
    /// they insert appears in this order.
    pub edits: Vec<Change>,
}
