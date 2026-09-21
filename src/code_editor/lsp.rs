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

mod bridge;
mod encoding;
mod position;
mod replacement;

pub use bridge::Bridge;
pub use encoding::Encoding;
pub use position::{Position, Range};
pub use replacement::Replacement;
