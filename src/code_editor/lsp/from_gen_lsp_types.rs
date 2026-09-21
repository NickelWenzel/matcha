//! Converting from the `gen-lsp-types` crate.
//!
//! Available with the `gen-lsp-types` feature, which asks for a range of
//! versions. Two rules keep one source building across all of it, and **both
//! are the opposite of what the `lsp-types` module does**:
//!
//! **Read a URI with `to_string`.** The type here is a newtype over a `String`
//! with no `as_str` of its own and nothing to deref to, and the crate's
//! features can replace it with two other types entirely. `Display` is the one
//! thing all three have.
//!
//! **Read an integer enum through `u32`.** These are real enums with named
//! variants, and 0.11 added a catch-all for numbers the protocol has not
//! named. A match on the variants builds against 0.11 and not against 0.9.
//!
//! Two fields have no home here. `Command::tooltip` and `CodeAction::tags`
//! exist in this crate and not in `lsp-types`, so carrying them would put a
//! field in the shared types that can never survive a trip through the other
//! family.

use super::{
    Change, CodeAction, Command, Diagnostic, Encoding, Hint, Message, Offer, Position, Range,
    Replacement, Snippet, diagnostic, document, hint, workspace,
};
use crate::code_editor::decoration::diagnostic::Severity;

impl From<gen_lsp_types::Position> for Position {
    fn from(from: gen_lsp_types::Position) -> Self {
        Self {
            line: from.line,
            character: from.character,
        }
    }
}

impl From<gen_lsp_types::Range> for Range {
    fn from(from: gen_lsp_types::Range) -> Self {
        Self {
            start: from.start.into(),
            end: from.end.into(),
        }
    }
}

impl From<gen_lsp_types::PositionEncodingKind> for Encoding {
    fn from(from: gen_lsp_types::PositionEncodingKind) -> Self {
        match String::from(from).as_str() {
            "utf-8" => Encoding::Utf8,
            "utf-32" => Encoding::Utf32,
            _ => Encoding::Utf16,
        }
    }
}

impl From<gen_lsp_types::Code> for diagnostic::Code {
    fn from(from: gen_lsp_types::Code) -> Self {
        match from {
            gen_lsp_types::Code::Int(number) => diagnostic::Code::Number(number),
            gen_lsp_types::Code::String(text) => diagnostic::Code::Text(text),
        }
    }
}

impl From<gen_lsp_types::DiagnosticRelatedInformation> for diagnostic::Related {
    fn from(from: gen_lsp_types::DiagnosticRelatedInformation) -> Self {
        Self {
            uri: from.location.uri.to_string(),
            range: from.location.range.into(),
            message: from.message,
        }
    }
}

impl From<gen_lsp_types::Diagnostic> for Diagnostic {
    fn from(from: gen_lsp_types::Diagnostic) -> Self {
        Self {
            range: from.range.into(),
            severity: severity(from.severity),
            // A message is text or marked-up text here, unlike the other
            // family, where it is only ever text.
            message: match from.message {
                gen_lsp_types::Message::String(text) => text,
                gen_lsp_types::Message::MarkupContent(markup) => markup.value,
            },
            source: from.source,
            code: from.code.map(diagnostic::Code::from),
            code_description: from
                .code_description
                .map(|description| description.href.to_string()),
            tags: from
                .tags
                .unwrap_or_default()
                .into_iter()
                .filter_map(tag)
                .collect(),
            related: from
                .related_information
                .unwrap_or_default()
                .into_iter()
                .map(diagnostic::Related::from)
                .collect(),
            data: from.data.map(|data| data.to_string()),
        }
    }
}

/// The severity a diagnostic is drawn with.
///
/// Through `u32` rather than by variant. 0.11 has a variant for a number the
/// protocol has not named and 0.9 does not, so a match on the variants builds
/// against one and not the other.
fn severity(from: Option<gen_lsp_types::DiagnosticSeverity>) -> Severity {
    match from.map(u32::from) {
        Some(2) => Severity::Warning,
        Some(3) => Severity::Information,
        Some(4) => Severity::Hint,
        _ => Severity::Error,
    }
}

fn tag(from: gen_lsp_types::DiagnosticTag) -> Option<diagnostic::Tag> {
    match u32::from(from) {
        1 => Some(diagnostic::Tag::Unnecessary),
        2 => Some(diagnostic::Tag::Deprecated),
        _ => None,
    }
}

impl From<gen_lsp_types::TextEdit> for Replacement {
    fn from(from: gen_lsp_types::TextEdit) -> Self {
        Self {
            range: from.range.into(),
            new_text: from.new_text,
            annotation_id: None,
        }
    }
}

impl From<gen_lsp_types::Edit> for Change {
    fn from(from: gen_lsp_types::Edit) -> Self {
        match from {
            gen_lsp_types::Edit::TextEdit(edit) => Change::Replace(edit.into()),
            gen_lsp_types::Edit::AnnotatedTextEdit(edit) => Change::Replace(Replacement {
                annotation_id: Some(edit.annotation_id),
                ..Replacement::from(edit.text_edit)
            }),
            // This family has a shape for a snippet and the other does not.
            // Kept and refused rather than dropped: applying its text as it
            // stands would put the markers in the user's file.
            gen_lsp_types::Edit::SnippetTextEdit(edit) => Change::Snippet(Snippet {
                range: edit.range.into(),
                value: edit.snippet.value,
                annotation_id: edit.annotation_id,
            }),
        }
    }
}

impl From<gen_lsp_types::InlayHint> for Hint {
    fn from(from: gen_lsp_types::InlayHint) -> Self {
        Self {
            position: from.position.into(),
            label: match from.label {
                gen_lsp_types::Label::String(text) => text,
                gen_lsp_types::Label::InlayHintLabelPartList(parts) => {
                    parts.into_iter().map(|part| part.value).collect()
                }
            },
            kind: from.kind.map(u32::from).and_then(|kind| match kind {
                1 => Some(hint::Kind::Type),
                2 => Some(hint::Kind::Parameter),
                _ => None,
            }),
            padding_left: from.padding_left.unwrap_or(false),
            padding_right: from.padding_right.unwrap_or(false),
            tooltip: from.tooltip.map(|tooltip| match tooltip {
                gen_lsp_types::Tooltip::String(text) => text,
                gen_lsp_types::Tooltip::MarkupContent(markup) => markup.value,
            }),
            text_edits: from
                .text_edits
                .unwrap_or_default()
                .into_iter()
                .map(Replacement::from)
                .collect(),
            data: from.data.map(|data| data.to_string()),
        }
    }
}

impl From<gen_lsp_types::TextDocumentEdit> for document::Edit {
    fn from(from: gen_lsp_types::TextDocumentEdit) -> Self {
        Self {
            // A level deeper than the other family puts it.
            uri: from.text_document.text_document_identifier.uri.to_string(),
            version: from.text_document.version,
            edits: from.edits.into_iter().map(Change::from).collect(),
        }
    }
}

impl From<gen_lsp_types::ChangeAnnotation> for workspace::Annotation {
    fn from(from: gen_lsp_types::ChangeAnnotation) -> Self {
        Self {
            label: from.label,
            needs_confirmation: from.needs_confirmation.unwrap_or(false),
            description: from.description,
        }
    }
}

impl From<gen_lsp_types::WorkspaceEdit> for workspace::Edit {
    fn from(from: gen_lsp_types::WorkspaceEdit) -> Self {
        let annotations = from
            .change_annotations
            .unwrap_or_default()
            .into_iter()
            .map(|(id, annotation)| (id, annotation.into()))
            .collect();

        // One flat list here, where the other family nests four levels. The
        // rule it settles is the same: the list wins wherever both arrive.
        let steps = match from.document_changes {
            Some(changes) => changes
                .into_iter()
                .map(|change| match change {
                    gen_lsp_types::DocumentChange::TextDocumentEdit(edit) => {
                        workspace::Step::Document(edit.into())
                    }
                    gen_lsp_types::DocumentChange::CreateFile(create) => {
                        workspace::Step::Operation(workspace::Operation::Create(
                            workspace::Create {
                                uri: create.uri.to_string(),
                                overwrite: create
                                    .options
                                    .as_ref()
                                    .and_then(|options| options.overwrite)
                                    .unwrap_or(false),
                                ignore_if_exists: create
                                    .options
                                    .and_then(|options| options.ignore_if_exists)
                                    .unwrap_or(false),
                                annotation_id: create.annotation_id,
                            },
                        ))
                    }
                    gen_lsp_types::DocumentChange::RenameFile(rename) => {
                        workspace::Step::Operation(workspace::Operation::Rename(
                            workspace::Rename {
                                old_uri: rename.old_uri.to_string(),
                                new_uri: rename.new_uri.to_string(),
                                overwrite: rename
                                    .options
                                    .as_ref()
                                    .and_then(|options| options.overwrite)
                                    .unwrap_or(false),
                                ignore_if_exists: rename
                                    .options
                                    .and_then(|options| options.ignore_if_exists)
                                    .unwrap_or(false),
                                annotation_id: rename.annotation_id,
                            },
                        ))
                    }
                    gen_lsp_types::DocumentChange::DeleteFile(delete) => {
                        workspace::Step::Operation(workspace::Operation::Delete(
                            workspace::Delete {
                                uri: delete.uri.to_string(),
                                recursive: delete
                                    .options
                                    .as_ref()
                                    .and_then(|options| options.recursive)
                                    .unwrap_or(false),
                                ignore_if_not_exists: delete
                                    .options
                                    .and_then(|options| options.ignore_if_not_exists)
                                    .unwrap_or(false),
                                // On the operation here, where the other family
                                // keeps a delete's on its options.
                                annotation_id: delete.annotation_id,
                            },
                        ))
                    }
                })
                .collect(),
            None => from
                .changes
                .unwrap_or_default()
                .into_iter()
                .map(|(uri, edits)| {
                    workspace::Step::Document(document::Edit {
                        uri: uri.to_string(),
                        version: None,
                        edits: edits
                            .into_iter()
                            .map(|edit| Change::Replace(edit.into()))
                            .collect(),
                    })
                })
                .collect(),
        };

        workspace::Edit { steps, annotations }
    }
}

impl From<gen_lsp_types::Command> for Command {
    fn from(from: gen_lsp_types::Command) -> Self {
        Self {
            title: from.title,
            command: from.command,
            arguments: from
                .arguments
                .unwrap_or_default()
                .into_iter()
                .map(|argument| argument.to_string())
                .collect(),
        }
    }
}

impl From<gen_lsp_types::CodeAction> for CodeAction {
    fn from(from: gen_lsp_types::CodeAction) -> Self {
        Self {
            title: from.title,
            kind: from.kind.map(String::from),
            diagnostics: from
                .diagnostics
                .unwrap_or_default()
                .into_iter()
                .map(Diagnostic::from)
                .collect(),
            edit: from.edit.map(workspace::Edit::from),
            command: from.command.map(Command::from),
            is_preferred: from.is_preferred.unwrap_or(false),
            disabled: from.disabled.map(|disabled| disabled.reason),
            data: from.data.map(|data| data.to_string()),
        }
    }
}

impl From<gen_lsp_types::CodeActionResponse> for Offer {
    fn from(from: gen_lsp_types::CodeActionResponse) -> Self {
        match from {
            gen_lsp_types::CodeActionResponse::CodeAction(action) => Offer::Action(action.into()),
            gen_lsp_types::CodeActionResponse::Command(command) => Offer::Command(command.into()),
        }
    }
}

impl From<gen_lsp_types::PublishDiagnosticsParams> for Message {
    fn from(from: gen_lsp_types::PublishDiagnosticsParams) -> Self {
        Message::Diagnostics {
            uri: from.uri.to_string(),
            version: from.version,
            diagnostics: from.diagnostics.into_iter().map(Diagnostic::from).collect(),
        }
    }
}

impl From<gen_lsp_types::ApplyWorkspaceEditParams> for Message {
    fn from(from: gen_lsp_types::ApplyWorkspaceEditParams) -> Self {
        Message::Edit {
            label: from.label,
            edit: from.edit.into(),
        }
    }
}

impl From<Vec<gen_lsp_types::CodeActionResponse>> for Message {
    fn from(from: Vec<gen_lsp_types::CodeActionResponse>) -> Self {
        Message::Offers(from.into_iter().map(Offer::from).collect())
    }
}

impl From<Option<Vec<gen_lsp_types::InlayHint>>> for Message {
    fn from(from: Option<Vec<gen_lsp_types::InlayHint>>) -> Self {
        Message::Hints {
            hints: from.map(|hints| hints.into_iter().map(Hint::from).collect()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    fn uri(text: &str) -> gen_lsp_types::Uri {
        text.to_owned().into()
    }

    fn range() -> gen_lsp_types::Range {
        gen_lsp_types::Range {
            start: gen_lsp_types::Position {
                line: 0,
                character: 0,
            },
            end: gen_lsp_types::Position {
                line: 0,
                character: 1,
            },
        }
    }

    fn wire(severity: Option<gen_lsp_types::DiagnosticSeverity>) -> gen_lsp_types::Diagnostic {
        gen_lsp_types::Diagnostic {
            range: range(),
            severity,
            code: None,
            code_description: None,
            source: None,
            message: gen_lsp_types::Message::String("unused".to_owned()),
            tags: None,
            related_information: None,
            data: None,
        }
    }

    fn document(uri_text: &str, version: Option<i32>) -> gen_lsp_types::TextDocumentEdit {
        gen_lsp_types::TextDocumentEdit {
            text_document: gen_lsp_types::OptionalVersionedTextDocumentIdentifier {
                version,
                text_document_identifier: gen_lsp_types::TextDocumentIdentifier {
                    uri: uri(uri_text),
                },
            },
            edits: vec![gen_lsp_types::Edit::TextEdit(gen_lsp_types::TextEdit {
                range: range(),
                new_text: "x".to_owned(),
            })],
        }
    }

    #[test]
    fn a_severity_is_read_through_its_number() {
        // By number rather than by variant. 0.11 has a variant for a severity
        // the protocol has not named and 0.9 does not, so matching on the
        // variants builds against one of them and not the other. The unnamed
        // case cannot be constructed here for the same reason.
        assert_eq!(
            Diagnostic::from(wire(Some(gen_lsp_types::DiagnosticSeverity::Warning))).severity,
            Severity::Warning
        );
        assert_eq!(
            Diagnostic::from(wire(Some(gen_lsp_types::DiagnosticSeverity::Hint))).severity,
            Severity::Hint
        );
        assert_eq!(Diagnostic::from(wire(None)).severity, Severity::Error);
    }

    #[test]
    fn a_marked_up_message_is_read_as_its_text() {
        // This family carries a message as text or as marked-up text. The
        // other only ever carries text.
        let mut from = wire(None);
        from.message = gen_lsp_types::Message::MarkupContent(gen_lsp_types::MarkupContent {
            kind: gen_lsp_types::MarkupKind::Markdown,
            value: "**unused**".to_owned(),
        });

        assert_eq!(Diagnostic::from(from).message, "**unused**");
    }

    #[test]
    fn a_snippet_is_kept_as_a_snippet() {
        // This family has a shape for one and the other does not. Turning it
        // into an ordinary replacement would put the markers in the file.
        let from = gen_lsp_types::Edit::SnippetTextEdit(gen_lsp_types::SnippetTextEdit {
            range: range(),
            snippet: gen_lsp_types::StringValue {
                value: "${1:name}$0".to_owned(),
            },
            annotation_id: Some("fill".to_owned()),
        });

        let Change::Snippet(snippet) = Change::from(from) else {
            panic!("a snippet stays one");
        };

        assert_eq!(snippet.value, "${1:name}$0");
        assert_eq!(snippet.annotation_id.as_deref(), Some("fill"));
    }

    #[test]
    fn a_flat_list_of_changes_keeps_its_order() {
        let from = gen_lsp_types::WorkspaceEdit {
            changes: None,
            document_changes: Some(vec![
                gen_lsp_types::DocumentChange::CreateFile(gen_lsp_types::CreateFile {
                    uri: uri("file:///b.rs"),
                    options: None,
                    annotation_id: None,
                }),
                gen_lsp_types::DocumentChange::TextDocumentEdit(document("file:///b.rs", Some(1))),
                gen_lsp_types::DocumentChange::DeleteFile(gen_lsp_types::DeleteFile {
                    uri: uri("file:///old.rs"),
                    options: Some(gen_lsp_types::DeleteFileOptions {
                        recursive: Some(true),
                        ignore_if_not_exists: None,
                    }),
                    // On the operation here. The other family keeps a delete's
                    // annotation on its options instead.
                    annotation_id: Some("cleanup".to_owned()),
                }),
            ]),
            change_annotations: None,
        };

        let edit = workspace::Edit::from(from);

        assert_eq!(edit.steps().len(), 3);
        assert!(matches!(edit.steps()[1], workspace::Step::Document(_)));

        let workspace::Step::Operation(workspace::Operation::Delete(delete)) = &edit.steps()[2]
        else {
            panic!("the third is a delete");
        };

        assert!(delete.recursive);
        assert_eq!(delete.annotation_id.as_deref(), Some("cleanup"));
    }

    #[test]
    fn the_list_wins_over_the_map_here_too() {
        let from = gen_lsp_types::WorkspaceEdit {
            changes: Some(HashMap::from([(
                uri("file:///a.rs"),
                vec![gen_lsp_types::TextEdit {
                    range: range(),
                    new_text: "from the map".to_owned(),
                }],
            )])),
            document_changes: Some(vec![gen_lsp_types::DocumentChange::TextDocumentEdit(
                document("file:///b.rs", Some(4)),
            )]),
            change_annotations: None,
        };

        let edit = workspace::Edit::from(from);
        let edits: Vec<_> = edit.document_edits().collect();

        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].uri, "file:///b.rs");
        assert_eq!(edits[0].version, Some(4));
    }

    #[test]
    fn a_uri_is_read_through_display() {
        // There is no `as_str` on this one and nothing to deref to, which is
        // the rule the other family has exactly backwards.
        let from = gen_lsp_types::PublishDiagnosticsParams {
            uri: uri("file:///a.rs"),
            version: Some(2),
            diagnostics: vec![wire(None)],
        };

        let Message::Diagnostics { uri, version, .. } = Message::from(from) else {
            panic!("built as diagnostics");
        };

        assert_eq!(uri, "file:///a.rs");
        assert_eq!(version, Some(2));
    }

    #[test]
    fn an_encoding_the_protocol_has_not_named_reads_as_the_one_every_server_has() {
        assert_eq!(
            Encoding::from(gen_lsp_types::PositionEncodingKind::UTF8),
            Encoding::Utf8
        );
        assert_eq!(
            Encoding::from(gen_lsp_types::PositionEncodingKind::UTF32),
            Encoding::Utf32
        );
    }
}
