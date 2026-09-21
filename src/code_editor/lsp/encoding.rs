//! How a language server counts the `character` in a position.

/// How a language server counts the `character` in a [`Position`].
///
/// Negotiated during initialization, which matcha takes no part in: the
/// application performs the handshake and passes the result in.
///
/// [`Position`]: super::Position
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    /// UTF-8 bytes, the units the editor stores.
    Utf8,
    /// UTF-16 code units.
    ///
    /// The protocol's default and the only encoding a server is obliged to
    /// support, so an unrecognised `positionEncoding` means this rather than
    /// an error.
    #[default]
    Utf16,
    /// UTF-32 code units, which is to say Unicode scalar values.
    Utf32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_encoding_is_the_protocols_own() {
        assert_eq!(Encoding::default(), Encoding::Utf16);
    }
}
