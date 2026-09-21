# Phase 9 — `lsp-types`, inbound

`From<lsp_types::X> for lsp::X`, and the feature that gates it.

## Prerequisites

Phase 8. Every `lsp::*` payload type exists.

## Goal and exit criteria

`--features lsp-types` builds, lints and round-trips at **0.95.0, 0.96.0 and
0.97.0**, with `Cargo.lock` restored after each.

## Step 1 — `Cargo.toml`

```toml
[features]
lsp = []
lsp-types = ["lsp", "dep:lsp-types"]

[dependencies]
# A RANGE, not a pin, and the range is the whole point. `lsp-types` has been
# frozen at 0.97 since 2024-06-04 while `async-lsp` — the obvious transport for
# an iced application — pins `^0.95`. Cargo treats a 0.x minor as a major, so
# those are disjoint: pinning either one hands an application two incompatible
# copies. A range lets Cargo unify on whatever it already resolved.
#
# Verified: every struct this reads is identical across 0.95, 0.96 and 0.97, and
# one source compiles against all three. `default-features = false` would be a
# no-op — the crate declares `default = []` — so it is omitted.
lsp-types = { version = ">=0.95, <0.98", optional = true }
```

Caveat for the docs: the range unifies only if the graph resolves **one**
`lsp-types`. An application depending on both `async-lsp @ ^0.95` and
`lsp-types = "0.97"` directly gets two copies regardless, and matcha binds to
one. Promise unification of matcha with the application's existing copy, not
unification in general.

## Step 2 — the two rules that make the range work

**Read URIs with `as_str()`.** It is allocation-free and works at all three
versions — `Url::as_str` at 0.95, a `Deref` to `fluent_uri::Uri::as_str` at
0.96+.

Be precise about what *does* fail at 0.96+, because the naive claim is false and
a reader who tests it will discard the rule: `Display` is not implemented on
`Uri` itself, so `format!("{}", uri)` and `ToString::to_string` **as a function
reference** both fail — but `uri.to_string()` as a *method call* compiles fine,
via that same `Deref`. `as_str()` is the rule because it allocates nothing and
never depends on the distinction.

**Match severity on the associated consts, with a `_` arm.**
`DiagnosticSeverity` is a newtype over a **private** `i32`, and `lsp_enum!`
generates only `Debug` and `TryFrom<&str>` — there is no `Into<i32>` and no
`.0`. So:

```rust
let severity = match d.severity {
    Some(lsp_types::DiagnosticSeverity::ERROR) => Severity::Error,
    Some(lsp_types::DiagnosticSeverity::WARNING) => Severity::Warning,
    Some(lsp_types::DiagnosticSeverity::INFORMATION) => Severity::Information,
    Some(lsp_types::DiagnosticSeverity::HINT) => Severity::Hint,
    // Absent, and anything outside 1..=4 (all legal JSON), both mean Error.
    _ => Severity::Error,
};
```

## Step 3 — the conversions

Field-for-field for `Position`, `Range`, `Replacement`, `Related`,
`code_description`, `Command`, `Create`/`Rename`/`Delete`.

Needing care:

- **`NumberOrString` → `Code`**, losslessly. Flattening `42` to `"42"` would
  make an echoed diagnostic stop comparing equal.
- **`InlayHintLabel`** — `String` or `LabelParts`; the parts' `value`s
  concatenate. This is where the flattening happens.
- **`data` and `Command.arguments`** are `LSPAny` on the wire. Read them with
  `value.to_string()`. **Do not reach for `serde_json` by name** — neither crate
  re-exports it, so naming it is an undeclared dependency that the features
  table cannot fix.
- **`WorkspaceEdit`** — the nested traversal: `Option<DocumentChanges>` →
  `Edits | Operations` → `Op | Edit` → `Create | Rename | Delete`. This is where
  the `document_changes`-supersedes-`changes` precedence is applied, and where
  `changes`-only entries are synthesized with `version: None`. Build the result
  with `workspace::Edit::new`, which Phase 7 made `pub(crate)` for exactly this.
- **`OneOf<TextEdit, AnnotatedTextEdit>`** — both become `Change::Replace`; the
  annotated one keeps its `annotation_id`.
- **`CodeActionOrCommand` → `Offer`.** Build `CodeAction` and `Command` with
  **struct literals, never positional constructors**: field order differs
  between the two crate families and `gen-lsp-types` has extra fields.
- **`PositionEncodingKind` → `Encoding`**, via `as_str()`. An unrecognised
  string is `Utf16`, per spec, not an error.

## Verification

```sh
cargo build --features lsp-types
cargo test --features lsp-types
cargo clippy --all-targets --features lsp-types -- -D warnings

# The range endpoints. This is the ONE carve-out from "no cargo update", it
# covers only this package name, and the lockfile MUST be restored after.
for v in 0.95.0 0.96.0 0.97.0; do
  cargo update -p lsp-types --precise $v && cargo build --features lsp-types
done
git checkout -- Cargo.lock
```

## Spot checks

| input | expected |
|---|---|
| `DiagnosticSeverity::WARNING` | `Severity::Warning` |
| `severity: None` | `Severity::Error` |
| `NumberOrString::Number(42)` | `Code::Number(42)`, never `Text` |
| `InlayHintLabel::LabelParts(["a","b"])` | label `"ab"` |
| a `WorkspaceEdit` with only `changes` | every `document::Edit` has `version: None` |
| a `WorkspaceEdit` with **both** | only `document_changes` survives |
| create A, edit A, rename A→B, edit B | four `Step`s, in order |
| `PositionEncodingKind::UTF8` | `Encoding::Utf8` |
| `PositionEncodingKind` of `"utf-7"` | `Encoding::Utf16` |
| a `Location` with a file URI | `Related.uri` is the URI string |

This phase also ships the **inbound** `workspace::Edit` test Phase 7 could not
build fixtures for: real wire shapes in, asserting the
`document_changes`-over-`changes` precedence and step order. The full round trip
needs outbound and is Phase 10's exit, not this one's.

## What NOT to change

- **The core `lsp` types.** This phase only converts into them.
- **`widget.rs`, `geometry.rs`, `content.rs`.**
- **No outbound.** `TryFrom` is Phase 10.
- **Never pin the dependency.** The range is the design.
- **Never leave `Cargo.lock` modified.**
