//! Translating language-server messages into the editor's own terms.
//!
//! matcha takes no part in the protocol itself: there is no transport here, no
//! JSON-RPC framing and no handshake. The application decodes a message and
//! hands the value over. What this module does is convert positions between the
//! encoding the server negotiated and the UTF-8 byte offsets the editor stores,
//! and turn the results into the decorations the widget already draws.
//!
//! The `lsp-types` and `gen-lsp-types` features add conversions to and from
//! those crates, so an application does not have to write the mapping itself.
//!
//! # A converted position is a snapshot
//!
//! A position that has been through the bridge is a byte offset into the text
//! as it stood at that moment. It is not an anchor. Later edits do not carry it
//! along, and nothing here updates it.
//!
//! Decorations are built for that. They are replaced whole on every round trip
//! rather than patched, so a position the next keystroke invalidates costs one
//! frame of a squiggle in the wrong place.
//!
//! An edit is not. Applying a range computed against older text changes the
//! wrong bytes, and nothing about the result looks wrong afterwards. So
//! [`Content::apply`] takes the [`revision`] the request was sent at and
//! refuses a buffer that has moved since.
//!
//! [`Content::apply`]: crate::Content::apply
//! [`revision`]: crate::Content::revision

pub mod diagnostic;
pub mod document;
pub mod hint;
pub mod workspace;

mod apply;
mod bridge;
mod encoding;
mod error;
mod position;
mod replacement;

pub use bridge::Bridge;
pub use diagnostic::Diagnostic;
pub use encoding::Encoding;
pub use error::Error;
pub use hint::Hint;
pub use position::{Position, Range};
pub use replacement::{Change, Replacement, Snippet};
