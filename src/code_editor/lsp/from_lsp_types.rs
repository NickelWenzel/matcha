//! Converting from the `lsp-types` crate.
//!
//! Available with the `lsp-types` feature, which asks for a range of versions
//! rather than one. Two rules keep one source building across all of it, and
//! both look like arbitrary style until the other versions are tried:
//!
//! **Read a URI with `as_str`.** The field is a `Url` at 0.95 and a `Uri` at
//! 0.96 and after, so the type can never be named here. `Display` is not
//! implemented on the later one, which rules out the formatting macros.
//!
//! **Match a severity on its associated constants.** They are a newtype over a
//! private integer, so there is nothing else to match on, and the arm for
//! anything outside them is not optional.

use super::{
    Change, CodeAction, Command, Diagnostic, Encoding, Hint, Message, Offer, Position, Range,
    Replacement, diagnostic, document, hint, outbound, workspace,
};
use crate::code_editor::decoration::diagnostic::Severity;

impl From<lsp_types::Position> for Position {
    fn from(from: lsp_types::Position) -> Self {
        Self {
            line: from.line,
            character: from.character,
        }
    }
}

impl From<lsp_types::Range> for Range {
    fn from(from: lsp_types::Range) -> Self {
        Self {
            start: from.start.into(),
            end: from.end.into(),
        }
    }
}

impl From<lsp_types::PositionEncodingKind> for Encoding {
    fn from(from: lsp_types::PositionEncodingKind) -> Self {
        match from.as_str() {
            "utf-8" => Encoding::Utf8,
            "utf-32" => Encoding::Utf32,
            // Including anything the protocol has not named. UTF-16 is what a
            // server is obliged to support, so it is the answer rather than an
            // error.
            _ => Encoding::Utf16,
        }
    }
}

impl From<lsp_types::NumberOrString> for diagnostic::Code {
    fn from(from: lsp_types::NumberOrString) -> Self {
        match from {
            lsp_types::NumberOrString::Number(number) => diagnostic::Code::Number(number),
            lsp_types::NumberOrString::String(text) => diagnostic::Code::Text(text),
        }
    }
}

impl From<lsp_types::DiagnosticRelatedInformation> for diagnostic::Related {
    fn from(from: lsp_types::DiagnosticRelatedInformation) -> Self {
        Self {
            uri: from.location.uri.as_str().to_owned(),
            range: from.location.range.into(),
            message: from.message,
        }
    }
}

impl From<lsp_types::Diagnostic> for Diagnostic {
    fn from(from: lsp_types::Diagnostic) -> Self {
        Self {
            range: from.range.into(),
            severity: severity(from.severity),
            message: from.message,
            source: from.source,
            code: from.code.map(diagnostic::Code::from),
            code_description: from
                .code_description
                .map(|description| description.href.as_str().to_owned()),
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
/// A server may leave it out, and may send a number outside the four the
/// protocol names. Both mean the same thing here: these severities differ only
/// in the colour of a squiggle, so reporting an error as something milder is
/// the worse of the two mistakes.
fn severity(from: Option<lsp_types::DiagnosticSeverity>) -> Severity {
    match from {
        Some(lsp_types::DiagnosticSeverity::WARNING) => Severity::Warning,
        Some(lsp_types::DiagnosticSeverity::INFORMATION) => Severity::Information,
        Some(lsp_types::DiagnosticSeverity::HINT) => Severity::Hint,
        _ => Severity::Error,
    }
}

/// A tag the protocol names, or nothing.
fn tag(from: lsp_types::DiagnosticTag) -> Option<diagnostic::Tag> {
    match from {
        lsp_types::DiagnosticTag::UNNECESSARY => Some(diagnostic::Tag::Unnecessary),
        lsp_types::DiagnosticTag::DEPRECATED => Some(diagnostic::Tag::Deprecated),
        _ => None,
    }
}

/// An absent flag is off, which is what every one of these means.
fn flag(from: Option<bool>) -> bool {
    from.unwrap_or(false)
}

impl From<lsp_types::TextEdit> for Replacement {
    fn from(from: lsp_types::TextEdit) -> Self {
        Self {
            range: from.range.into(),
            new_text: from.new_text,
            annotation_id: None,
        }
    }
}

impl From<lsp_types::AnnotatedTextEdit> for Replacement {
    fn from(from: lsp_types::AnnotatedTextEdit) -> Self {
        Self {
            annotation_id: Some(from.annotation_id),
            ..Replacement::from(from.text_edit)
        }
    }
}

impl From<lsp_types::OneOf<lsp_types::TextEdit, lsp_types::AnnotatedTextEdit>> for Change {
    fn from(from: lsp_types::OneOf<lsp_types::TextEdit, lsp_types::AnnotatedTextEdit>) -> Self {
        Change::Replace(match from {
            lsp_types::OneOf::Left(edit) => edit.into(),
            lsp_types::OneOf::Right(edit) => edit.into(),
        })
    }
}

impl From<lsp_types::InlayHint> for Hint {
    fn from(from: lsp_types::InlayHint) -> Self {
        Self {
            position: from.position.into(),
            label: label(from.label),
            kind: kind(from.kind),
            padding_left: from.padding_left.unwrap_or(false),
            padding_right: from.padding_right.unwrap_or(false),
            tooltip: from.tooltip.map(tooltip),
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

/// A label, with any parts joined.
fn label(from: lsp_types::InlayHintLabel) -> String {
    match from {
        lsp_types::InlayHintLabel::String(text) => text,
        lsp_types::InlayHintLabel::LabelParts(parts) => {
            parts.into_iter().map(|part| part.value).collect()
        }
    }
}

fn kind(from: Option<lsp_types::InlayHintKind>) -> Option<hint::Kind> {
    match from {
        Some(lsp_types::InlayHintKind::TYPE) => Some(hint::Kind::Type),
        Some(lsp_types::InlayHintKind::PARAMETER) => Some(hint::Kind::Parameter),
        _ => None,
    }
}

fn tooltip(from: lsp_types::InlayHintTooltip) -> String {
    match from {
        lsp_types::InlayHintTooltip::String(text) => text,
        lsp_types::InlayHintTooltip::MarkupContent(markup) => markup.value,
    }
}

impl From<lsp_types::TextDocumentEdit> for document::Edit {
    fn from(from: lsp_types::TextDocumentEdit) -> Self {
        Self {
            uri: from.text_document.uri.as_str().to_owned(),
            version: from.text_document.version,
            edits: from.edits.into_iter().map(Change::from).collect(),
        }
    }
}

impl From<lsp_types::ChangeAnnotation> for workspace::Annotation {
    fn from(from: lsp_types::ChangeAnnotation) -> Self {
        Self {
            label: from.label,
            needs_confirmation: from.needs_confirmation.unwrap_or(false),
            description: from.description,
        }
    }
}

impl From<lsp_types::ResourceOp> for workspace::Operation {
    fn from(from: lsp_types::ResourceOp) -> Self {
        match from {
            lsp_types::ResourceOp::Create(create) => {
                let options = create.options;

                workspace::Operation::Create(workspace::Create {
                    uri: create.uri.as_str().to_owned(),
                    overwrite: flag(options.as_ref().and_then(|options| options.overwrite)),
                    ignore_if_exists: flag(
                        options
                            .as_ref()
                            .and_then(|options| options.ignore_if_exists),
                    ),
                    annotation_id: create.annotation_id,
                })
            }
            lsp_types::ResourceOp::Rename(rename) => {
                let options = rename.options;

                workspace::Operation::Rename(workspace::Rename {
                    old_uri: rename.old_uri.as_str().to_owned(),
                    new_uri: rename.new_uri.as_str().to_owned(),
                    overwrite: flag(options.as_ref().and_then(|options| options.overwrite)),
                    ignore_if_exists: flag(
                        options
                            .as_ref()
                            .and_then(|options| options.ignore_if_exists),
                    ),
                    annotation_id: rename.annotation_id,
                })
            }
            lsp_types::ResourceOp::Delete(delete) => {
                let options = delete.options;

                workspace::Operation::Delete(workspace::Delete {
                    uri: delete.uri.as_str().to_owned(),
                    recursive: flag(options.as_ref().and_then(|options| options.recursive)),
                    ignore_if_not_exists: flag(
                        options
                            .as_ref()
                            .and_then(|options| options.ignore_if_not_exists),
                    ),
                    // On the options rather than on the operation, which is
                    // where create and rename keep theirs. The crate is not
                    // consistent about it and the protocol is not either.
                    annotation_id: options.and_then(|options| options.annotation_id),
                })
            }
        }
    }
}

impl From<lsp_types::WorkspaceEdit> for workspace::Edit {
    fn from(from: lsp_types::WorkspaceEdit) -> Self {
        let annotations = from
            .change_annotations
            .unwrap_or_default()
            .into_iter()
            .map(|(id, annotation)| (id, annotation.into()))
            .collect();

        // The protocol carries the same information two ways and says the list
        // wins wherever a client understands it. Settling that here, once, is
        // what lets everything downstream read one ordered sequence.
        let steps = match from.document_changes {
            Some(lsp_types::DocumentChanges::Edits(edits)) => edits
                .into_iter()
                .map(|edit| workspace::Step::Document(edit.into()))
                .collect(),
            Some(lsp_types::DocumentChanges::Operations(operations)) => operations
                .into_iter()
                .map(|operation| match operation {
                    lsp_types::DocumentChangeOperation::Edit(edit) => {
                        workspace::Step::Document(edit.into())
                    }
                    lsp_types::DocumentChangeOperation::Op(operation) => {
                        workspace::Step::Operation(operation.into())
                    }
                })
                .collect(),
            // The map form carries no versions and no order between documents,
            // which is why a server that has anything to say about either sends
            // the list instead.
            None => from
                .changes
                .unwrap_or_default()
                .into_iter()
                .map(|(uri, edits)| {
                    workspace::Step::Document(document::Edit {
                        uri: uri.as_str().to_owned(),
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

impl From<lsp_types::Command> for Command {
    fn from(from: lsp_types::Command) -> Self {
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

impl From<lsp_types::CodeAction> for CodeAction {
    fn from(from: lsp_types::CodeAction) -> Self {
        Self {
            title: from.title,
            kind: from.kind.map(|kind| kind.as_str().to_owned()),
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

impl From<lsp_types::CodeActionOrCommand> for Offer {
    fn from(from: lsp_types::CodeActionOrCommand) -> Self {
        match from {
            lsp_types::CodeActionOrCommand::CodeAction(action) => Offer::Action(action.into()),
            lsp_types::CodeActionOrCommand::Command(command) => Offer::Command(command.into()),
        }
    }
}

impl From<lsp_types::PublishDiagnosticsParams> for Message {
    fn from(from: lsp_types::PublishDiagnosticsParams) -> Self {
        Message::Diagnostics {
            uri: from.uri.as_str().to_owned(),
            version: from.version,
            diagnostics: from.diagnostics.into_iter().map(Diagnostic::from).collect(),
        }
    }
}

impl From<lsp_types::ApplyWorkspaceEditParams> for Message {
    fn from(from: lsp_types::ApplyWorkspaceEditParams) -> Self {
        Message::Edit {
            label: from.label,
            edit: from.edit.into(),
        }
    }
}

impl From<Vec<lsp_types::CodeActionOrCommand>> for Message {
    fn from(from: Vec<lsp_types::CodeActionOrCommand>) -> Self {
        Message::Offers(from.into_iter().map(Offer::from).collect())
    }
}

impl From<Option<Vec<lsp_types::InlayHint>>> for Message {
    fn from(from: Option<Vec<lsp_types::InlayHint>>) -> Self {
        Message::Hints {
            hints: from.map(|hints| hints.into_iter().map(Hint::from).collect()),
        }
    }
}

/// A URI of whatever type the field being filled asks for.
///
/// The type is a `Url` at 0.95 and a `Uri` after, so it is never written down:
/// the call site infers it from where the result is going.
fn uri<T: std::str::FromStr>(text: &str) -> Result<T, outbound::Error> {
    text.parse()
        .map_err(|_| outbound::Error::Uri(text.to_owned()))
}

/// The JSON value some text was meant to be.
fn json<T: std::str::FromStr>(from: Option<String>) -> Result<Option<T>, outbound::Error> {
    match from {
        Some(text) => match text.parse() {
            Ok(value) => Ok(Some(value)),
            Err(_) => Err(outbound::Error::Json(text)),
        },
        None => Ok(None),
    }
}

// ---- back the other way ----
//
// Fallible, and the reason is URIs. At 0.96 and after a URI is built only by
// parsing, and at 0.95 it is a `url::Url`, which is also built by parsing.
// Neither type can be named here, so both are reached by inferring the type
// from the field being filled.

impl From<Position> for lsp_types::Position {
    fn from(from: Position) -> Self {
        Self {
            line: from.line,
            character: from.character,
        }
    }
}

impl From<Range> for lsp_types::Range {
    fn from(from: Range) -> Self {
        Self {
            start: from.start.into(),
            end: from.end.into(),
        }
    }
}

impl From<Encoding> for lsp_types::PositionEncodingKind {
    fn from(from: Encoding) -> Self {
        match from {
            Encoding::Utf8 => lsp_types::PositionEncodingKind::UTF8,
            Encoding::Utf16 => lsp_types::PositionEncodingKind::UTF16,
            Encoding::Utf32 => lsp_types::PositionEncodingKind::UTF32,
        }
    }
}

impl From<Replacement> for lsp_types::TextEdit {
    fn from(from: Replacement) -> Self {
        Self {
            range: from.range.into(),
            new_text: from.new_text,
        }
    }
}

impl TryFrom<Change> for lsp_types::OneOf<lsp_types::TextEdit, lsp_types::AnnotatedTextEdit> {
    type Error = outbound::Error;

    fn try_from(from: Change) -> Result<Self, Self::Error> {
        match from {
            Change::Replace(replacement) => Ok(match replacement.annotation_id.clone() {
                Some(annotation_id) => lsp_types::OneOf::Right(lsp_types::AnnotatedTextEdit {
                    text_edit: replacement.into(),
                    annotation_id,
                }),
                None => lsp_types::OneOf::Left(replacement.into()),
            }),
            Change::Snippet(_) => Err(outbound::Error::Snippet),
        }
    }
}

impl TryFrom<Diagnostic> for lsp_types::Diagnostic {
    type Error = outbound::Error;

    fn try_from(from: Diagnostic) -> Result<Self, Self::Error> {
        Ok(Self {
            range: from.range.into(),
            severity: Some(match from.severity {
                Severity::Error => lsp_types::DiagnosticSeverity::ERROR,
                Severity::Warning => lsp_types::DiagnosticSeverity::WARNING,
                Severity::Information => lsp_types::DiagnosticSeverity::INFORMATION,
                Severity::Hint => lsp_types::DiagnosticSeverity::HINT,
            }),
            code: from.code.map(|code| match code {
                diagnostic::Code::Number(number) => lsp_types::NumberOrString::Number(number),
                diagnostic::Code::Text(text) => lsp_types::NumberOrString::String(text),
            }),
            code_description: from
                .code_description
                .map(|href| {
                    Ok::<_, outbound::Error>(lsp_types::CodeDescription { href: uri(&href)? })
                })
                .transpose()?,
            source: from.source,
            message: from.message,
            related_information: related(from.related)?,
            // Absent rather than empty. The two read the same on the wire, and
            // a server that sent nothing should get nothing back rather than a
            // list it did not write.
            tags: absent_if_empty(
                from.tags
                    .into_iter()
                    .map(|tag| match tag {
                        diagnostic::Tag::Unnecessary => lsp_types::DiagnosticTag::UNNECESSARY,
                        diagnostic::Tag::Deprecated => lsp_types::DiagnosticTag::DEPRECATED,
                    })
                    .collect(),
            ),
            data: json(from.data)?,
        })
    }
}

/// Nothing rather than an empty list, which is what a server that had nothing
/// to say sent in the first place.
fn absent_if_empty<T>(from: Vec<T>) -> Option<Vec<T>> {
    (!from.is_empty()).then_some(from)
}

fn related(
    from: Vec<diagnostic::Related>,
) -> Result<Option<Vec<lsp_types::DiagnosticRelatedInformation>>, outbound::Error> {
    if from.is_empty() {
        return Ok(None);
    }

    from.into_iter()
        .map(|related| {
            Ok(lsp_types::DiagnosticRelatedInformation {
                location: lsp_types::Location {
                    uri: uri(&related.uri)?,
                    range: related.range.into(),
                },
                message: related.message,
            })
        })
        .collect::<Result<_, _>>()
        .map(Some)
}

impl TryFrom<CodeAction> for lsp_types::CodeAction {
    type Error = outbound::Error;

    fn try_from(from: CodeAction) -> Result<Self, Self::Error> {
        Ok(Self {
            title: from.title,
            kind: from.kind.map(lsp_types::CodeActionKind::from),
            diagnostics: absent_if_empty(
                from.diagnostics
                    .into_iter()
                    .map(lsp_types::Diagnostic::try_from)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            edit: from
                .edit
                .map(lsp_types::WorkspaceEdit::try_from)
                .transpose()?,
            command: from.command.map(lsp_types::Command::try_from).transpose()?,
            is_preferred: Some(from.is_preferred),
            disabled: from
                .disabled
                .map(|reason| lsp_types::CodeActionDisabled { reason }),
            data: json(from.data)?,
        })
    }
}

impl TryFrom<Command> for lsp_types::Command {
    type Error = outbound::Error;

    fn try_from(from: Command) -> Result<Self, Self::Error> {
        Ok(Self {
            title: from.title,
            command: from.command,
            arguments: absent_if_empty(
                from.arguments
                    .into_iter()
                    .map(|argument| {
                        argument
                            .parse()
                            .map_err(|_| outbound::Error::Json(argument.clone()))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        })
    }
}

impl TryFrom<document::Edit> for lsp_types::TextDocumentEdit {
    type Error = outbound::Error;

    fn try_from(from: document::Edit) -> Result<Self, Self::Error> {
        Ok(Self {
            text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                uri: uri(&from.uri)?,
                version: from.version,
            },
            edits: from
                .edits
                .into_iter()
                .map(lsp_types::OneOf::try_from)
                .collect::<Result<_, _>>()?,
        })
    }
}

impl TryFrom<workspace::Edit> for lsp_types::WorkspaceEdit {
    type Error = outbound::Error;

    fn try_from(from: workspace::Edit) -> Result<Self, Self::Error> {
        let change_annotations = from
            .annotations
            .into_iter()
            .map(|(id, annotation)| {
                (
                    id,
                    lsp_types::ChangeAnnotation {
                        label: annotation.label,
                        needs_confirmation: Some(annotation.needs_confirmation),
                        description: annotation.description,
                    },
                )
            })
            .collect::<std::collections::HashMap<_, _>>();

        // Always the list, never the map: the map cannot carry a version, an
        // order, or a file operation, and it is the shape a server sends only
        // when it has none of those to say.
        let operations = from
            .steps
            .into_iter()
            .map(|step| {
                Ok(match step {
                    workspace::Step::Document(edit) => {
                        lsp_types::DocumentChangeOperation::Edit(edit.try_into()?)
                    }
                    workspace::Step::Operation(operation) => {
                        lsp_types::DocumentChangeOperation::Op(operation.try_into()?)
                    }
                })
            })
            .collect::<Result<Vec<_>, outbound::Error>>()?;

        Ok(Self {
            changes: None,
            document_changes: Some(lsp_types::DocumentChanges::Operations(operations)),
            change_annotations: (!change_annotations.is_empty()).then_some(change_annotations),
        })
    }
}

impl TryFrom<workspace::Operation> for lsp_types::ResourceOp {
    type Error = outbound::Error;

    fn try_from(from: workspace::Operation) -> Result<Self, Self::Error> {
        Ok(match from {
            workspace::Operation::Create(create) => {
                lsp_types::ResourceOp::Create(lsp_types::CreateFile {
                    uri: uri(&create.uri)?,
                    options: Some(lsp_types::CreateFileOptions {
                        overwrite: Some(create.overwrite),
                        ignore_if_exists: Some(create.ignore_if_exists),
                    }),
                    annotation_id: create.annotation_id,
                })
            }
            workspace::Operation::Rename(rename) => {
                lsp_types::ResourceOp::Rename(lsp_types::RenameFile {
                    old_uri: uri(&rename.old_uri)?,
                    new_uri: uri(&rename.new_uri)?,
                    options: Some(lsp_types::RenameFileOptions {
                        overwrite: Some(rename.overwrite),
                        ignore_if_exists: Some(rename.ignore_if_exists),
                    }),
                    annotation_id: rename.annotation_id,
                })
            }
            workspace::Operation::Delete(delete) => {
                lsp_types::ResourceOp::Delete(lsp_types::DeleteFile {
                    uri: uri(&delete.uri)?,
                    options: Some(lsp_types::DeleteFileOptions {
                        recursive: Some(delete.recursive),
                        ignore_if_not_exists: Some(delete.ignore_if_not_exists),
                        annotation_id: delete.annotation_id,
                    }),
                })
            }
        })
    }
}

impl TryFrom<Offer> for lsp_types::CodeActionOrCommand {
    type Error = outbound::Error;

    fn try_from(from: Offer) -> Result<Self, Self::Error> {
        Ok(match from {
            Offer::Action(action) => lsp_types::CodeActionOrCommand::CodeAction(action.try_into()?),
            Offer::Command(command) => lsp_types::CodeActionOrCommand::Command(command.try_into()?),
        })
    }
}

/// What this bridge understands, ready to send in `initialize`.
///
/// Most of what matcha models arrives only if the application asked for it. A
/// server that is not told the client understands change annotations never
/// sends one, and a diagnostic's `data` is dropped on the way out unless
/// `dataSupport` is set -- which takes diagnostic-driven code actions with it,
/// because a server cannot match a diagnostic that has lost its own
/// identifiers.
///
/// Merge this into whatever else the application advertises. Nothing here is a
/// promise about anything matcha does not do.
///
/// `failureHandling` says `textOnlyTransactional` rather than `transactional`,
/// which is the honest answer: [`Content::apply`](crate::Content::apply) is all
/// or nothing for one buffer and matcha cannot undo across several.
///
/// `normalizesLineEndings` says true, because it does.
pub fn client_capabilities() -> lsp_types::ClientCapabilities {
    lsp_types::ClientCapabilities {
        workspace: Some(lsp_types::WorkspaceClientCapabilities {
            workspace_edit: Some(lsp_types::WorkspaceEditClientCapabilities {
                document_changes: Some(true),
                resource_operations: Some(vec![
                    lsp_types::ResourceOperationKind::Create,
                    lsp_types::ResourceOperationKind::Rename,
                    lsp_types::ResourceOperationKind::Delete,
                ]),
                failure_handling: Some(lsp_types::FailureHandlingKind::TextOnlyTransactional),
                normalizes_line_endings: Some(true),
                change_annotation_support: Some(
                    lsp_types::ChangeAnnotationWorkspaceEditClientCapabilities {
                        groups_on_label: Some(false),
                    },
                ),
            }),
            ..Default::default()
        }),
        text_document: Some(lsp_types::TextDocumentClientCapabilities {
            publish_diagnostics: Some(lsp_types::PublishDiagnosticsClientCapabilities {
                related_information: Some(true),
                tag_support: Some(lsp_types::TagSupport {
                    value_set: vec![
                        lsp_types::DiagnosticTag::UNNECESSARY,
                        lsp_types::DiagnosticTag::DEPRECATED,
                    ],
                }),
                version_support: Some(true),
                code_description_support: Some(true),
                data_support: Some(true),
            }),
            inlay_hint: Some(lsp_types::InlayHintClientCapabilities {
                dynamic_registration: Some(false),
                resolve_support: Some(lsp_types::InlayHintResolveClientCapabilities {
                    properties: vec!["tooltip".to_owned(), "textEdits".to_owned()],
                }),
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    /// A URI, with its type taken from wherever it is being put. The type is
    /// named `Url` at 0.95 and `Uri` after, so it can only ever be inferred.
    macro_rules! uri {
        ($text:literal) => {
            $text.parse().expect("a well-formed file URI")
        };
    }

    fn range() -> lsp_types::Range {
        lsp_types::Range {
            start: lsp_types::Position {
                line: 0,
                character: 0,
            },
            end: lsp_types::Position {
                line: 0,
                character: 1,
            },
        }
    }

    fn wire(severity: Option<lsp_types::DiagnosticSeverity>) -> lsp_types::Diagnostic {
        lsp_types::Diagnostic {
            range: range(),
            severity,
            message: "unused".to_owned(),
            ..lsp_types::Diagnostic::default()
        }
    }

    fn edit(uri: &'static str, version: Option<i32>) -> lsp_types::TextDocumentEdit {
        lsp_types::TextDocumentEdit {
            text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                uri: match uri {
                    "file:///a.rs" => uri!("file:///a.rs"),
                    _ => uri!("file:///b.rs"),
                },
                version,
            },
            edits: vec![lsp_types::OneOf::Left(lsp_types::TextEdit {
                range: range(),
                new_text: "x".to_owned(),
            })],
        }
    }

    #[test]
    fn a_replacement_survives_the_trip_out_and_back() {
        let there = lsp_types::TextEdit {
            range: range(),
            new_text: "answer".to_owned(),
        };

        let back = lsp_types::TextEdit::from(Replacement::from(there.clone()));

        assert_eq!(back, there);
    }

    #[test]
    fn a_workspace_edit_survives_the_trip_out_and_back() {
        let there = lsp_types::WorkspaceEdit {
            changes: None,
            document_changes: Some(lsp_types::DocumentChanges::Operations(vec![
                lsp_types::DocumentChangeOperation::Op(lsp_types::ResourceOp::Create(
                    lsp_types::CreateFile {
                        uri: uri!("file:///b.rs"),
                        options: Some(lsp_types::CreateFileOptions {
                            overwrite: Some(true),
                            ignore_if_exists: Some(false),
                        }),
                        annotation_id: None,
                    },
                )),
                lsp_types::DocumentChangeOperation::Edit(edit("file:///b.rs", Some(1))),
            ])),
            change_annotations: None,
        };

        let back = lsp_types::WorkspaceEdit::try_from(workspace::Edit::from(there.clone()))
            .expect("every URI came from one that parsed");

        assert_eq!(back, there);
    }

    #[test]
    fn a_code_action_survives_the_trip_out_and_back() {
        let there = lsp_types::CodeAction {
            title: "Import".to_owned(),
            kind: Some(lsp_types::CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![wire(Some(lsp_types::DiagnosticSeverity::ERROR))]),
            edit: None,
            command: None,
            is_preferred: Some(true),
            disabled: Some(lsp_types::CodeActionDisabled {
                reason: "not here".to_owned(),
            }),
            data: None,
        };

        let back = lsp_types::CodeAction::try_from(CodeAction::from(there.clone()))
            .expect("nothing here needs a URI");

        assert_eq!(back.title, there.title);
        assert_eq!(back.kind, there.kind);
        assert_eq!(back.is_preferred, there.is_preferred);
        assert_eq!(back.disabled, there.disabled);
        assert_eq!(back.diagnostics, there.diagnostics);
    }

    #[test]
    fn a_hint_is_deliberately_not_the_same_on_the_way_back() {
        // Label parts are joined on the way in, and the locations each part
        // could carry go with them. Asserting equality here would mean picking
        // a hint that has no parts, which tests nothing.
        let there = lsp_types::InlayHint {
            position: lsp_types::Position {
                line: 0,
                character: 0,
            },
            label: lsp_types::InlayHintLabel::LabelParts(vec![lsp_types::InlayHintLabelPart {
                value: ": i32".to_owned(),
                location: Some(lsp_types::Location {
                    uri: uri!("file:///defined.rs"),
                    range: range(),
                }),
                ..Default::default()
            }]),
            kind: None,
            text_edits: None,
            tooltip: None,
            padding_left: None,
            padding_right: None,
            data: None,
        };

        let hint = Hint::from(there);

        assert_eq!(hint.label, ": i32", "the text survives");
        // Where it was defined does not, which is why a hint is text here and
        // not something to follow.
    }

    #[test]
    fn a_uri_the_crate_will_not_take_is_an_error_rather_than_a_panic() {
        let edit = workspace::Edit {
            steps: vec![workspace::Step::Document(document::Edit {
                uri: "not a uri at all".to_owned(),
                version: None,
                edits: Vec::new(),
            })],
            annotations: std::collections::HashMap::new(),
        };

        assert_eq!(
            lsp_types::WorkspaceEdit::try_from(edit),
            Err(outbound::Error::Uri("not a uri at all".to_owned()))
        );
    }

    #[test]
    fn a_snippet_has_nowhere_to_go_in_this_crate() {
        let snippet = Change::Snippet(super::super::Snippet {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            value: "${1:name}$0".to_owned(),
            annotation_id: None,
        });

        assert_eq!(
            lsp_types::OneOf::try_from(snippet).err(),
            Some(outbound::Error::Snippet),
            "reporting this as a bad URI would send the caller looking in the \
             wrong place"
        );
    }

    #[test]
    fn the_advertised_capabilities_ask_for_everything_that_is_modelled() {
        let capabilities = client_capabilities();

        let edit = capabilities
            .workspace
            .and_then(|workspace| workspace.workspace_edit)
            .expect("workspace edits are advertised");

        assert_eq!(edit.document_changes, Some(true));
        assert_eq!(edit.resource_operations.map(|kinds| kinds.len()), Some(3));
        assert_eq!(
            edit.normalizes_line_endings,
            Some(true),
            "matcha does normalise them, and a server told otherwise may send \
             endings that do not match the file"
        );
        assert!(edit.change_annotation_support.is_some());

        let diagnostics = capabilities
            .text_document
            .and_then(|document| document.publish_diagnostics)
            .expect("diagnostics are advertised");

        assert_eq!(
            diagnostics.data_support,
            Some(true),
            "without this a server drops the identifiers it needs to resolve a \
             fix for a diagnostic"
        );
        assert_eq!(diagnostics.version_support, Some(true));
        assert!(diagnostics.tag_support.is_some());
    }

    #[test]
    fn a_severity_the_protocol_names_survives_and_anything_else_reads_as_an_error() {
        assert_eq!(
            Diagnostic::from(wire(Some(lsp_types::DiagnosticSeverity::WARNING))).severity,
            Severity::Warning
        );
        assert_eq!(
            Diagnostic::from(wire(Some(lsp_types::DiagnosticSeverity::HINT))).severity,
            Severity::Hint
        );
        assert_eq!(
            Diagnostic::from(wire(None)).severity,
            Severity::Error,
            "a server that says nothing is reporting something worth seeing"
        );
    }

    #[test]
    fn a_numeric_code_stays_numeric() {
        let mut wire = wire(None);
        wire.code = Some(lsp_types::NumberOrString::Number(42));

        assert_eq!(
            Diagnostic::from(wire).code,
            Some(diagnostic::Code::Number(42)),
            "a code sent back as text no longer matches the one that came"
        );
    }

    #[test]
    fn a_related_location_keeps_the_document_it_names() {
        let mut wire = wire(None);
        wire.related_information = Some(vec![lsp_types::DiagnosticRelatedInformation {
            location: lsp_types::Location {
                uri: uri!("file:///other.rs"),
                range: range(),
            },
            message: "first defined here".to_owned(),
        }]);

        assert_eq!(
            Diagnostic::from(wire).related[0].uri,
            "file:///other.rs",
            "matcha cannot tell this buffer from another file, so it keeps both"
        );
    }

    #[test]
    fn the_parts_of_a_label_are_joined() {
        let hint = lsp_types::InlayHint {
            position: lsp_types::Position {
                line: 0,
                character: 0,
            },
            label: lsp_types::InlayHintLabel::LabelParts(vec![
                lsp_types::InlayHintLabelPart {
                    value: ": ".to_owned(),
                    ..Default::default()
                },
                lsp_types::InlayHintLabelPart {
                    value: "i32".to_owned(),
                    ..Default::default()
                },
            ]),
            kind: Some(lsp_types::InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: Some(true),
            padding_right: None,
            data: None,
        };

        let hint = Hint::from(hint);

        assert_eq!(hint.label, ": i32");
        assert_eq!(hint.kind, Some(hint::Kind::Type));
        assert!(hint.padding_left);
        assert!(!hint.padding_right, "an absent flag is off");
    }

    #[test]
    fn an_encoding_the_protocol_has_not_named_reads_as_the_one_every_server_has() {
        assert_eq!(
            Encoding::from(lsp_types::PositionEncodingKind::UTF8),
            Encoding::Utf8
        );
        assert_eq!(
            Encoding::from(lsp_types::PositionEncodingKind::UTF32),
            Encoding::Utf32
        );
        assert_eq!(
            Encoding::from(lsp_types::PositionEncodingKind::new("utf-7")),
            Encoding::Utf16
        );
    }

    #[test]
    fn the_map_form_of_a_workspace_edit_carries_no_versions() {
        let wire = lsp_types::WorkspaceEdit {
            changes: Some(HashMap::from([(
                uri!("file:///a.rs"),
                vec![lsp_types::TextEdit {
                    range: range(),
                    new_text: "x".to_owned(),
                }],
            )])),
            document_changes: None,
            change_annotations: None,
        };

        let edit = workspace::Edit::from(wire);
        let edits: Vec<_> = edit.document_edits().collect();

        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].uri, "file:///a.rs");
        assert_eq!(
            edits[0].version, None,
            "the map form says nothing about versions, which is why a server \
             with anything to say about them sends the list"
        );
    }

    #[test]
    fn the_list_form_wins_wherever_both_are_sent() {
        let wire = lsp_types::WorkspaceEdit {
            changes: Some(HashMap::from([(
                uri!("file:///a.rs"),
                vec![lsp_types::TextEdit {
                    range: range(),
                    new_text: "from the map".to_owned(),
                }],
            )])),
            document_changes: Some(lsp_types::DocumentChanges::Edits(vec![edit(
                "file:///b.rs",
                Some(3),
            )])),
            change_annotations: None,
        };

        let edit = workspace::Edit::from(wire);
        let edits: Vec<_> = edit.document_edits().collect();

        assert_eq!(
            edits.len(),
            1,
            "the map is not read at all when a list came"
        );
        assert_eq!(edits[0].uri, "file:///b.rs");
        assert_eq!(edits[0].version, Some(3));
    }

    #[test]
    fn a_rename_refactor_keeps_its_order_and_its_versions() {
        let wire = lsp_types::WorkspaceEdit {
            changes: None,
            document_changes: Some(lsp_types::DocumentChanges::Operations(vec![
                lsp_types::DocumentChangeOperation::Op(lsp_types::ResourceOp::Create(
                    lsp_types::CreateFile {
                        uri: uri!("file:///b.rs"),
                        options: None,
                        annotation_id: None,
                    },
                )),
                lsp_types::DocumentChangeOperation::Edit(edit("file:///b.rs", Some(1))),
                lsp_types::DocumentChangeOperation::Op(lsp_types::ResourceOp::Rename(
                    lsp_types::RenameFile {
                        old_uri: uri!("file:///b.rs"),
                        new_uri: uri!("file:///a.rs"),
                        options: None,
                        annotation_id: None,
                    },
                )),
                lsp_types::DocumentChangeOperation::Edit(edit("file:///a.rs", Some(2))),
            ])),
            change_annotations: None,
        };

        let edit = workspace::Edit::from(wire);

        assert_eq!(edit.steps().len(), 4);
        assert!(matches!(
            edit.steps()[0],
            workspace::Step::Operation(workspace::Operation::Create(_))
        ));
        assert!(matches!(
            edit.steps()[2],
            workspace::Step::Operation(workspace::Operation::Rename(_))
        ));

        let edits: Vec<_> = edit.document_edits().collect();

        assert_eq!((edits[0].version, edits[1].version), (Some(1), Some(2)));
    }

    #[test]
    fn a_deletes_annotation_is_found_where_this_crate_keeps_it() {
        // Create and rename carry one on the operation. Delete carries one on
        // its options instead.
        let wire = lsp_types::ResourceOp::Delete(lsp_types::DeleteFile {
            uri: uri!("file:///gone.rs"),
            options: Some(lsp_types::DeleteFileOptions {
                recursive: Some(true),
                ignore_if_not_exists: None,
                annotation_id: Some("cleanup".to_owned()),
            }),
        });

        let workspace::Operation::Delete(delete) = workspace::Operation::from(wire) else {
            panic!("built as a delete");
        };

        assert_eq!(delete.uri, "file:///gone.rs");
        assert!(delete.recursive);
        assert!(!delete.ignore_if_not_exists);
        assert_eq!(delete.annotation_id.as_deref(), Some("cleanup"));
    }

    #[test]
    fn a_code_action_response_holds_both_shapes() {
        let wire = vec![
            lsp_types::CodeActionOrCommand::Command(lsp_types::Command {
                title: "Run".to_owned(),
                command: "run".to_owned(),
                arguments: None,
            }),
            lsp_types::CodeActionOrCommand::CodeAction(lsp_types::CodeAction {
                title: "Import".to_owned(),
                kind: Some(lsp_types::CodeActionKind::QUICKFIX),
                is_preferred: Some(true),
                ..Default::default()
            }),
        ];

        let Message::Offers(offers) = Message::from(wire) else {
            panic!("built as offers");
        };

        assert!(matches!(offers[0], Offer::Command(_)));

        let Offer::Action(action) = &offers[1] else {
            panic!("the second is an action");
        };

        assert!(action.is_preferred);
        assert!(action.is_kind("quickfix"));
    }

    #[test]
    fn diagnostics_arrive_with_the_document_and_version_they_describe() {
        let wire = lsp_types::PublishDiagnosticsParams {
            uri: uri!("file:///a.rs"),
            diagnostics: vec![wire(Some(lsp_types::DiagnosticSeverity::ERROR))],
            version: Some(11),
        };

        let Message::Diagnostics {
            uri,
            version,
            diagnostics,
        } = Message::from(wire)
        else {
            panic!("built as diagnostics");
        };

        assert_eq!(uri, "file:///a.rs");
        assert_eq!(version, Some(11));
        assert_eq!(diagnostics.len(), 1);
    }
}
