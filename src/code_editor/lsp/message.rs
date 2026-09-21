//! What a language server sent.

use super::{Diagnostic, Hint, Offer, workspace};

/// Something a language server sent.
///
/// This is what the per-crate conversions produce, for an application with one
/// path that everything arrives through. An application that already knows
/// which request it issued can skip it and convert the payload directly.
///
/// Every variant carries what identifies the message as well as what it says.
/// A payload alone is not routable: two buffers cannot both be told about
/// diagnostics that name no document.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// `textDocument/publishDiagnostics`.
    Diagnostics {
        /// Which document, as the server spelled it.
        uri: String,
        /// The version the server read, if it said.
        version: Option<i32>,
        /// What it found.
        diagnostics: Vec<Diagnostic>,
    },
    /// An answer to `textDocument/inlayHint`.
    Hints {
        /// The hints, or nothing.
        ///
        /// Nothing and an empty list differ on the wire: a server answers with
        /// nothing when it has no hints to give at all, and with an empty list
        /// when it has none in the range that was asked about.
        hints: Option<Vec<Hint>>,
    },
    /// A `workspace/applyEdit` request.
    ///
    /// A request rather than a notification: the server is waiting for an
    /// answer. matcha is not the transport and will not send one.
    Edit {
        /// What to call the change in an undo list.
        label: Option<String>,
        /// What to change.
        edit: workspace::Edit,
    },
    /// An answer to `textDocument/codeAction`.
    Offers(Vec<Offer>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_arrive_with_the_document_they_are_about() {
        let message = Message::Diagnostics {
            uri: "file:///a.rs".to_owned(),
            version: Some(7),
            diagnostics: Vec::new(),
        };

        let Message::Diagnostics { uri, version, .. } = &message else {
            panic!("built as diagnostics");
        };

        // Without these an application with two buffers cannot tell which one
        // the message is about, nor whether it still applies.
        assert_eq!(uri, "file:///a.rs");
        assert_eq!(*version, Some(7));
    }

    #[test]
    fn no_hints_and_no_hints_in_range_are_different_answers() {
        let none = Message::Hints { hints: None };
        let empty = Message::Hints {
            hints: Some(Vec::new()),
        };

        assert_ne!(none, empty);
    }

    #[test]
    fn an_edit_request_keeps_what_the_change_is_called() {
        let message = Message::Edit {
            label: Some("Rename symbol".to_owned()),
            edit: workspace::Edit {
                steps: Vec::new(),
                annotations: std::collections::HashMap::new(),
            },
        };

        let Message::Edit { label, .. } = &message else {
            panic!("built as an edit");
        };

        assert_eq!(label.as_deref(), Some("Rename symbol"));
    }
}
