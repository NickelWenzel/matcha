# Phase 10 — `lsp-types`, outbound and client capabilities

The direction that makes the extra fields worth carrying, plus the single
highest-value item in the whole feature.

## Prerequisites

Phase 9: `--features lsp-types` exists and converts inbound.

## Why outbound is not optional

Every justification for carrying `data`, `code`, `source` and `tags` is an
echo-back — into `CodeActionContext.diagnostics`, into `codeAction/resolve`,
into any request built from `Bridge::locate`. Inbound-only, an application has
to keep the original `lsp_types` value alongside matcha's, at which point
matcha's copy is dead weight and the feature has not removed the glue it exists
to remove.

## Step 1 — `TryFrom`, not `From`, and the reason is URIs

At 0.96+ a `Uri` is constructible only through `FromStr`
(`lsp-types-0.97.0/src/uri.rs:42-53`); at 0.95 the field is a `url::Url`, also
`FromStr`. Both can refuse, and `unwrap`/`expect` are banned — so every type
carrying a URI is fallible: `Related`, `document::Edit`, `workspace::Edit`,
`Operation`, `CodeAction`, `Message`.

The type itself can **never be named**: it is `Url` at 0.95 and `Uri` at 0.96+.
The spelling that compiles at all three infers it from the struct field, and
erases the error because those differ too. **Verified at 0.95.0, 0.96.0 and
0.97.0:**

```rust
let loc = lsp_types::Location {
    uri: s.parse().map_err(|_| uri::Error)?,
    range,
};

let mut changes = HashMap::new();          // key type inferred from below
changes.insert(s.parse().map_err(|_| uri::Error)?, edits);
let edit = lsp_types::WorkspaceEdit {
    changes: Some(changes),
    ..Default::default()
};
```

`uri::Error` is the unit struct from the plan's surface block, wrapped by
`OutboundError::Uri`. It needs a doc,
and it is `uri::Error` rather than `UriError` because the module path carries
the noun, like every other type here.

**Which URIs are valid differs across the range.** Measured:
`"C:\\Users\\x\\a.rs"` parses at 0.95 and is **rejected** at 0.97. So the range
unifies *types*, not *behaviour* — say so next to the unification promise.

## Step 2 — the other two failure modes

- **`Change::Snippet` has no `lsp-types` analogue.** A document's edits there
  are `Vec<OneOf<TextEdit, AnnotatedTextEdit>>`. Converting a batch containing a
  snippet is an **error**, not a silent drop — the same reason `Content::apply`
  refuses one.
- **`data` and `Command.arguments` are `LSPAny` on the wire.** Writing is
  `s.parse().map_err(..)?` with the type inferred from the struct field — the
  same never-name-the-type trick as URIs. **Do not name `serde_json`**: neither
  crate re-exports it, so a `serde_json::to_string` call is an undeclared
  dependency the features table cannot fix.

## Step 3 — `client_capabilities()`

```rust
pub mod from_lsp_types {
    /// Exactly what this bridge understands, prefilled.
    pub fn client_capabilities() -> lsp_types::ClientCapabilities;
}
```

**It lives in a module, not at `lsp::` root.** Phase 12 ships a function of the
same name for the other family, and two items cannot both be
`matcha::lsp::client_capabilities` — `cargo test --all-features` builds both.

Every optional field Phases 4 and 7 model arrives **only if the application
advertised it**, so this is the difference between the feature working and
silently receiving nothing:

- `workspace.workspaceEdit.documentChanges` — without it a server sends only
  `changes`, and the whole ordered-`Step` traversal is dead code.
- `.resourceOperations` — create, rename, delete.
- `.changeAnnotationSupport` — without it no `AnnotatedTextEdit` ever arrives,
  so `annotation_id` is always `None`.
- `.normalizesLineEndings: true` — matcha **does** normalize. helix advertises
  `false` and reproduces the corruption this plan measured.
- `.failureHandling: textOnlyTransactional` — honestly what per-document
  all-or-nothing gives. Not `transactional`: matcha cannot roll back across
  documents.
- `publishDiagnostics.{tagSupport, codeDescriptionSupport, dataSupport,
  relatedInformation, versionSupport}` — one per optional field on
  `lsp::Diagnostic`.
- `inlayHint.resolveSupport` — for `Hint.data`.

The core stays zero-dependency because this lives behind the feature that
already has the types.

## Verification

```sh
cargo test --features lsp-types
cargo clippy --all-targets --features lsp-types -- -D warnings
cargo clippy --all-targets --all-features -- -D warnings   # catches a name collision

for v in 0.95.0 0.96.0 0.97.0; do
  cargo update -p lsp-types --precise $v && cargo build --features lsp-types
done
git checkout -- Cargo.lock
```

## Spot checks

| case | expected |
|---|---|
| `lsp_types::Replacement` → `lsp::` → back | compares equal |
| the same for `workspace::Edit` | compares equal |
| the same for `CodeAction` | compares equal |
| **`Hint`** | **deliberately lossy** — labels were flattened in Phase 4. Assert what survives and name what does not |
| an unparseable URI | `Err(OutboundError::Uri(..))`, never a panic |
| a batch containing a `Change::Snippet` | `Err(OutboundError::Snippet)`, not a silent drop |
| `Encoding::Utf8` → `PositionEncodingKind` | `UTF8` |
| `client_capabilities()` | `documentChanges`, `resourceOperations`, `changeAnnotationSupport` and `normalizesLineEndings` all set |
| `--all-features` build | compiles; no `client_capabilities` collision |

The `Hint` row matters: writing an equality test for a lossy type means picking
a fixture that dodges the lossy branch, which is the vacuous test this plan
warns about. Assert the loss explicitly instead.

## What NOT to change

- **The inbound impls.** Phase 9 owns them.
- **`widget.rs`, `geometry.rs`, `content.rs`.**
- **Never name the URI type**, and never `unwrap` a parse.
- **Do not report a snippet failure as a URI failure.**
- **Never name `serde_json`.**
- **Never leave `Cargo.lock` modified.**
