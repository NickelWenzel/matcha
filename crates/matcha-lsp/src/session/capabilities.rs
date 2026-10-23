//! What the client advertises, and the subset of the server's capabilities it acts on.

use gen_lsp_types::{
    ClientCapabilities, ClientCompletionItemOptions, ClientInlayHintResolveOptions,
    ClientSignatureInformationOptions, ClientSignatureParameterInformationOptions,
    CompletionClientCapabilities, DefinitionClientCapabilities, DefinitionProvider,
    DocumentFormattingClientCapabilities, DocumentFormattingProvider, FailureHandlingKind,
    GeneralClientCapabilities, HoverClientCapabilities, HoverProvider, InlayHintClientCapabilities,
    InlayHintProvider, InlayHintWorkspaceClientCapabilities, MarkupKind,
    PublishDiagnosticsClientCapabilities, RenameClientCapabilities, RenameProvider,
    ServerCapabilities, SignatureHelpClientCapabilities, TextDocumentClientCapabilities,
    TextDocumentSync, TextDocumentSyncClientCapabilities, TextDocumentSyncKind,
    WindowClientCapabilities, WorkspaceClientCapabilities, WorkspaceEditClientCapabilities,
};

use crate::Encoding;

/// The server's capabilities, reduced to what the client acts on.
#[derive(Debug)]
pub(crate) struct Server {
    /// Whether the server wants `didOpen` and `didClose`.
    pub(crate) open_close: bool,
    /// How `didChange` is sent; anything but FULL or INCREMENTAL means not at all.
    pub(crate) change: TextDocumentSyncKind,
    /// Whether and how `didSave` is sent.
    pub(crate) save: Save,
    /// The completion trigger strings; `None` when the server has no completion provider.
    pub(crate) completion: Option<Vec<String>>,
    /// Whether the server answers `textDocument/signatureHelp`.
    pub(crate) signature: bool,
    /// Whether the server answers `textDocument/hover`.
    pub(crate) hover: bool,
    /// Whether the server answers `textDocument/definition`.
    pub(crate) definition: bool,
    /// Whether the server answers `textDocument/rename`.
    pub(crate) rename: bool,
    /// Whether the server answers `textDocument/formatting`.
    pub(crate) formatting: bool,
    /// Whether the server answers `textDocument/inlayHint`, and whether it resolves hints.
    pub(crate) inlay: Option<Resolve>,
}

/// What the server asks to be sent when a document is saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Save {
    /// No `didSave`.
    Never,
    /// `didSave` without the text.
    Notify,
    /// `didSave` with the saved text.
    WithText,
}

/// Whether the server answers `inlayHint/resolve`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resolve {
    /// Hints arrive whole.
    Unsupported,
    /// Hints arrive without the lazily resolvable properties, which `inlayHint/resolve` fills in.
    Supported,
}

impl Server {
    /// An absent `textDocumentSync`, and an options object without `openClose`, `change` or
    /// `save`, mean the spec's defaults: no open/close notifications, no changes and no saves.
    pub(crate) fn new(capabilities: &ServerCapabilities) -> Self {
        let (open_close, change, save) = match &capabilities.text_document_sync {
            // LSP §textDocument_synchronization: a bare kind is the shorthand for
            // `{ openClose: true, change: kind }`, which asks for no saves.
            Some(TextDocumentSync::Kind(kind)) => (true, *kind, Save::Never),
            Some(TextDocumentSync::Options(options)) => (
                options.open_close.unwrap_or(false),
                options.change.unwrap_or(TextDocumentSyncKind::None),
                match &options.save {
                    None | Some(gen_lsp_types::Save::Bool(false)) => Save::Never,
                    Some(gen_lsp_types::Save::Bool(true)) => Save::Notify,
                    Some(gen_lsp_types::Save::SaveOptions(options)) => {
                        if options.include_text == Some(true) {
                            Save::WithText
                        } else {
                            Save::Notify
                        }
                    }
                },
            ),
            None => (false, TextDocumentSyncKind::None, Save::Never),
        };
        let completion = capabilities
            .completion_provider
            .as_ref()
            .map(|options| options.trigger_characters.clone().unwrap_or_default());
        Self {
            open_close,
            change,
            save,
            completion,
            signature: capabilities.signature_help_provider.is_some(),
            hover: matches!(
                capabilities.hover_provider,
                Some(HoverProvider::Bool(true) | HoverProvider::HoverOptions(_))
            ),
            definition: matches!(
                capabilities.definition_provider,
                Some(DefinitionProvider::Bool(true) | DefinitionProvider::DefinitionOptions(_))
            ),
            rename: matches!(
                capabilities.rename_provider,
                Some(RenameProvider::Bool(true) | RenameProvider::RenameOptions(_))
            ),
            formatting: matches!(
                capabilities.document_formatting_provider,
                Some(
                    DocumentFormattingProvider::Bool(true)
                        | DocumentFormattingProvider::DocumentFormattingOptions(_)
                )
            ),
            inlay: match &capabilities.inlay_hint_provider {
                None | Some(InlayHintProvider::Bool(false)) => None,
                Some(InlayHintProvider::Bool(true)) => Some(Resolve::Unsupported),
                Some(InlayHintProvider::InlayHintOptions(options)) => {
                    Some(resolve(options.resolve_provider))
                }
                Some(InlayHintProvider::InlayHintRegistrationOptions(options)) => {
                    Some(resolve(options.inlay_hint_options.resolve_provider))
                }
            },
        }
    }
}

fn resolve(provider: Option<bool>) -> Resolve {
    if provider == Some(true) {
        Resolve::Supported
    } else {
        Resolve::Unsupported
    }
}

/// The capabilities sent in `initialize`.
pub(crate) fn client() -> ClientCapabilities {
    ClientCapabilities {
        workspace: Some(WorkspaceClientCapabilities {
            configuration: Some(true),
            workspace_folders: Some(true),
            // Versioned edits let a rename refuse documents that moved. No resource operations:
            // an edit that creates, renames or deletes a file is refused whole.
            workspace_edit: Some(WorkspaceEditClientCapabilities {
                document_changes: Some(true),
                failure_handling: Some(FailureHandlingKind::Transactional),
                ..WorkspaceEditClientCapabilities::default()
            }),
            inlay_hint: Some(InlayHintWorkspaceClientCapabilities {
                refresh_support: Some(true),
            }),
            ..WorkspaceClientCapabilities::default()
        }),
        text_document: Some(TextDocumentClientCapabilities {
            synchronization: Some(TextDocumentSyncClientCapabilities {
                did_save: Some(true),
                ..TextDocumentSyncClientCapabilities::default()
            }),
            publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                version_support: Some(true),
                ..PublishDiagnosticsClientCapabilities::default()
            }),
            completion: Some(CompletionClientCapabilities {
                completion_item: Some(ClientCompletionItemOptions {
                    snippet_support: Some(true),
                    // The popup renders documentation as plain text; markdown from a server that
                    // ignores this is lowered.
                    documentation_format: Some(vec![MarkupKind::PlainText]),
                    ..ClientCompletionItemOptions::default()
                }),
                context_support: Some(true),
                ..CompletionClientCapabilities::default()
            }),
            signature_help: Some(SignatureHelpClientCapabilities {
                signature_information: Some(ClientSignatureInformationOptions {
                    // The box renders documentation as plain text.
                    documentation_format: Some(vec![MarkupKind::PlainText]),
                    parameter_information: Some(ClientSignatureParameterInformationOptions {
                        label_offset_support: Some(true),
                    }),
                    active_parameter_support: Some(true),
                    ..ClientSignatureInformationOptions::default()
                }),
                ..SignatureHelpClientCapabilities::default()
            }),
            hover: Some(HoverClientCapabilities {
                content_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                ..HoverClientCapabilities::default()
            }),
            definition: Some(DefinitionClientCapabilities {
                link_support: Some(true),
                ..DefinitionClientCapabilities::default()
            }),
            // Without `prepareSupport`: the editor asks for the new name itself.
            rename: Some(RenameClientCapabilities::default()),
            formatting: Some(DocumentFormattingClientCapabilities::default()),
            // Only tooltips are lazy: locations and text edits arrive with the hint, so a jump or
            // an insert needs no round trip and cannot meet a stale resolve.
            inlay_hint: Some(InlayHintClientCapabilities {
                dynamic_registration: Some(false),
                resolve_support: Some(ClientInlayHintResolveOptions {
                    properties: vec!["tooltip".to_owned(), "label.tooltip".to_owned()],
                }),
            }),
            ..TextDocumentClientCapabilities::default()
        }),
        // `$/progress` reaches the host as a notification; `window/workDoneProgress/create` is
        // answered with `null`.
        window: Some(WindowClientCapabilities {
            work_done_progress: Some(true),
            ..WindowClientCapabilities::default()
        }),
        general: Some(GeneralClientCapabilities {
            // Preference order: utf-8 positions need no conversion walk.
            position_encodings: Some(
                [Encoding::Utf8, Encoding::Utf16, Encoding::Utf32]
                    .map(Encoding::kind)
                    .to_vec(),
            ),
            ..GeneralClientCapabilities::default()
        }),
        ..ClientCapabilities::default()
    }
}
