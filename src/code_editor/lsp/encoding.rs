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

impl Encoding {
    /// How many code units `character` occupies in this encoding.
    pub(crate) fn width(self, character: char) -> u32 {
        match self {
            Encoding::Utf8 => character.len_utf8() as u32,
            Encoding::Utf16 => character.len_utf16() as u32,
            // One unit per scalar value, which is what a `char` is.
            Encoding::Utf32 => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_encoding_is_the_protocols_own() {
        assert_eq!(Encoding::default(), Encoding::Utf16);
    }

    #[test]
    fn each_encoding_measures_a_character_in_its_own_units() {
        // One byte, one UTF-16 unit, one scalar value.
        assert_eq!(Encoding::Utf8.width('a'), 1);
        assert_eq!(Encoding::Utf16.width('a'), 1);
        assert_eq!(Encoding::Utf32.width('a'), 1);

        // Two bytes, still one UTF-16 unit.
        assert_eq!(Encoding::Utf8.width('é'), 2);
        assert_eq!(Encoding::Utf16.width('é'), 1);
        assert_eq!(Encoding::Utf32.width('é'), 1);

        // Four bytes, and the surrogate pair that makes UTF-16 columns able to
        // point inside a character.
        assert_eq!(Encoding::Utf8.width('😀'), 4);
        assert_eq!(Encoding::Utf16.width('😀'), 2);
        assert_eq!(Encoding::Utf32.width('😀'), 1);
    }
}
