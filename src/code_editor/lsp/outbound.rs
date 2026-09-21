//! Why a value could not be sent back to a language server.

use std::fmt;

/// Why a value could not be converted into a language server's own types.
///
/// Converting the other way cannot fail: everything a server sends has
/// somewhere to go here. Converting back can, because the types it has to
/// build are narrower than the ones here are.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A URI the server's crate would not accept.
    ///
    /// What counts as one differs across the versions matcha supports: a
    /// Windows path that parses under 0.95 of `lsp-types` is refused by 0.97.
    Uri(String),
    /// Text that was meant to be a JSON value and would not read as one.
    ///
    /// The `data` a server attaches is carried as the text it arrived as and
    /// never read, so this only arises for text an application wrote itself.
    Json(String),
    /// A snippet, which `lsp-types` has no shape for.
    ///
    /// `gen-lsp-types` does, so the same value converts there.
    Snippet,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Uri(uri) => write!(f, "{uri} is not a URI this server's types accept"),
            Error::Json(text) => write!(f, "{text} is not a JSON value"),
            Error::Snippet => write!(f, "lsp-types has no shape for a snippet edit"),
        }
    }
}

impl std::error::Error for Error {}
