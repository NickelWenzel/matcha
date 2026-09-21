# Phase 8 — Code actions and the message envelope

The last of the payload types, and the enum that names what arrived.

## Prerequisites

Phases 4 and 7: `Diagnostic`, `Hint`, `workspace::Edit`.

## Goal and exit criteria

`CodeAction`, `Command`, `Offer` and `Message` exist and carry enough that an
application can route, rank and resolve without keeping the original wire value
alongside.

## Step 1 — `lsp/action.rs`

```rust
/// Something a server offers to do about a diagnostic or a selection.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeAction {
    /// Shown to the user.
    pub title: String,
    /// The kind, e.g. `quickfix` or `refactor.extract`.
    ///
    /// **Matched hierarchically, not by prefix**: `refactor.extract` matches a
    /// request for `refactor`, and `refactory` does not. The rule is
    /// `kind == want || (kind.starts_with(want) && kind[want.len()..].starts_with('.'))`.
    /// Documented here because every consumer that reaches for `starts_with`
    /// gets it wrong.
    pub kind: Option<String>,
    /// The diagnostics this addresses, echoed back to resolve it.
    pub diagnostics: Vec<Diagnostic>,
    /// The edits. If `command` is also set, these apply **first**.
    pub edit: Option<workspace::Edit>,
    /// A command to run after `edit`.
    pub command: Option<Command>,
    /// The server's hint that this is the obvious choice. Part of how a client
    /// ranks a list; without it every application rewrites the ranking table.
    pub is_preferred: bool,
    /// `Some(reason)` means the action exists but cannot run now.
    ///
    /// **Carried, not filtered.** Whether to hide it or grey it out is policy
    /// and belongs to the application, and the reason is what a user reads.
    pub disabled: Option<String>,
    /// Opaque server JSON, so this survives `codeAction/resolve` going out and
    /// coming back enriched.
    pub data: Option<String>,
}

/// A command a server can run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// Shown to the user.
    pub title: String,
    /// The identifier to send back.
    pub command: String,
    /// One raw JSON value per argument, so an application can rebuild
    /// `workspace/executeCommand` without matcha parsing anything.
    pub arguments: Vec<String>,
}

/// What `textDocument/codeAction` answers with.
///
/// The protocol allows either shape in the same list, so a bare
/// `Vec<CodeAction>` has nowhere to put a `Command`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Offer {
    /// A full code action.
    Action(CodeAction),
    /// A bare command.
    Command(Command),
}
```

## Step 2 — `lsp/message.rs`

The enum the feature is named for. Its job is to be what the per-crate
conversions target, so an application with one "a message arrived" path converts
once instead of per request kind.

**It carries the envelope.** An earlier draft kept only the payload, which made
it useless for the dispatch it was justified by: `PublishDiagnosticsParams` is
`{ uri, diagnostics, version }`, and an application with two buffers cannot
route a variant that keeps only the middle field.

```rust
/// Something a language server sent.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// `textDocument/publishDiagnostics`.
    Diagnostics {
        /// Which document. The application's routing key.
        uri: String,
        /// The version the server analysed, if it said.
        version: Option<i32>,
        /// The diagnostics themselves.
        diagnostics: Vec<Diagnostic>,
    },
    /// A `textDocument/inlayHint` response.
    Hints {
        /// The hints. `None` and an empty list mean different things on the
        /// wire — no hints available versus none in range — so the
        /// distinction is kept.
        hints: Option<Vec<Hint>>,
    },
    /// A `workspace/applyEdit` request.
    ///
    /// This is a *request*: the client owes a response. matcha is not the
    /// transport and will not send one, so the application must.
    Edit {
        /// What to call the change in an undo list.
        label: Option<String>,
        /// The edit.
        edit: workspace::Edit,
    },
    /// A `textDocument/codeAction` response.
    Offers(Vec<Offer>),
}
```

An application that already knows which request it issued may skip `Message`
entirely and call `Bridge` on the payload types. That is supported, and the
Phase 13 example shows both.

## Verification

```sh
cargo test --features lsp
cargo clippy --all-targets --features lsp -- -D warnings
cargo doc --no-deps --features lsp
```

## Spot checks

| case | expected |
|---|---|
| a `Message::Diagnostics` | `uri` and `version` survive construction and match |
| `Message::Hints { hints: None }` vs `Some(vec![])` | they compare unequal |
| `Message::Edit` | `label` survives |
| `Offer` | holds a `CodeAction` and a `Command`, both |
| `kind` matching `"refactor"` against `"refactor.extract"` | matches |
| the same against `"refactory"` | **does not match** |
| a disabled action | still present, with its reason |

The `refactory` row is the mutation test: replace the hierarchical match with
`starts_with` and it must fail by name.

## What NOT to change

- **`widget.rs`, `geometry.rs`, `content.rs`.**
- **Do not filter disabled actions.** Policy belongs to the application.
- **Do not collapse `Hints { hints: Option<..> }` to a bare `Vec`.** The
  distinction is on the wire.
- **No conversions yet.** Phases 9 to 12.
