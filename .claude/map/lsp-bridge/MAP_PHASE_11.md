# Phase 11 — `gen-lsp-types`, inbound

The same surface as Phase 9, for the other faction — the metamodel-generated
crate that rust-analyzer and wgsl-analyzer use. **Two of its rules are the
opposite of Phase 9's**, which is the single easiest thing to get wrong here.

## Prerequisites

Phase 8. Phase 9 is *not* a prerequisite — the two are independent and can run
in parallel.

## Goal and exit criteria

`--features gen-lsp-types` builds, lints and round-trips at **0.9.0, 0.10.0 and
0.11.0**, with `Cargo.lock` restored after each.

## Step 1 — `Cargo.toml`

```toml
[features]
gen-lsp-types = ["lsp", "dep:gen-lsp-types"]

[dependencies]
# A range, for the same reason as `lsp-types` but from the opposite problem:
# this crate is alive and moves fast — 0.5.0 to 0.11.0 in five months — so a pin
# would make matcha ship a breaking release every time it does. Verified: every
# struct this reads is identical across 0.9, 0.10 and 0.11.
gen-lsp-types = { version = ">=0.9, <0.12", optional = true }
```

## Step 2 — the two rules that are BACKWARDS from Phase 9

**Read URIs with `to_string()`, not `as_str()`.** Measured: the default `Uri` is
`struct Uri(pub String)` with **no inherent `as_str` and no `Deref`**, so
`as_str()` does not compile. `AsRef<str>` works for the default and for the
`url` form but not for `fluent-uri`; `Display` is the only accessor common to
all three shapes. So `to_string()`.

Do **not** reach for `.0` — the `url` and `fluent-uri` features *redefine* the
type, and features are additive across a dependency graph, so a transitive
dependency can change it under matcha. Those two features are also mutually
exclusive by `compile_error!` (`generated/common.rs:68-71`), which is an
application-level hazard nothing here can fix; document it.

**Route integer enums through `u32::from`, never an exhaustive match.** 0.11
added `#[serde(untagged)] Custom(u32)` to `DiagnosticSeverity`, `DiagnosticTag`,
`InlayHintKind` and `CodeActionTag`, and 0.9 and 0.10 do not have it — so an
exhaustive match compiles at one end of the range and not the other.
`#[serde(into = "u32")]` is on every version:

```rust
let severity = match u32::from(raw) {
    1 => Severity::Error,
    2 => Severity::Warning,
    3 => Severity::Information,
    4 => Severity::Hint,
    _ => Severity::Error,
};
```

## Step 3 — the conversions

The shape differences from Phase 9, which is where most of the work is:

| | `lsp-types` (Phase 9) | `gen-lsp-types` (here) |
|---|---|---|
| workspace changes | nested, four levels | **flat `Vec<DocumentChange>`**, 4 variants |
| a document's edits | `Vec<OneOf<TextEdit, AnnotatedTextEdit>>` | `Vec<Edit>` — **3 cases**, including `SnippetTextEdit` |
| unions | one generic `OneOf<A, B>` | 88 hand-named `#[serde(untagged)]` enums (87 at 0.9) |
| `Diagnostic.message` | `String` | a `Message` union needing `MarkupContent` flattening |
| document identifier | `id.uri` | `id.text_document_identifier.uri` |
| `LSPAny` | spelled `LSPAny` | spelled **`LspAny`** |

- **`SnippetTextEdit` becomes `Change::Snippet`**, never `Replace` and never
  dropped. Its `value` is snippet syntax, and `Content::apply` refuses a batch
  containing one.
- **`gen_lsp_types::Message` already exists** — it is the `String |
  MarkupContent` union on `Diagnostic.message` — so this file has two `Message`
  types in scope. Aliasing is forbidden, so disambiguate by path.
- The precedence rule still applies, and is still applied here on the way in.

## Verification

```sh
cargo build --features gen-lsp-types
cargo test --features gen-lsp-types
cargo clippy --all-targets --features gen-lsp-types -- -D warnings
cargo clippy --all-targets --all-features -- -D warnings   # both families at once

for v in 0.9.0 0.10.0 0.11.0; do
  cargo update -p gen-lsp-types --precise $v && cargo build --features gen-lsp-types
done
git checkout -- Cargo.lock
```

## Spot checks

| input | expected |
|---|---|
| `DiagnosticSeverity::Warning` | `Severity::Warning` |
| a `Custom(99)` severity (0.11 only) | `Severity::Error` |
| a `Uri` | its string, via `to_string()` |
| `Edit::SnippetTextEdit` | `Change::Snippet`, **not** `Replace` |
| `Edit::AnnotatedTextEdit` | `Change::Replace` keeping `annotation_id` |
| a flat `Vec<DocumentChange>` | `Step`s in the same order |
| `Diagnostic.message` as `MarkupContent` | flattened to its text |
| builds at 0.9.0 **and** 0.11.0 | both, one source |

## What NOT to change

- **Phase 9's rules.** They are genuinely different here; copying `as_str()`
  across will not compile.
- **The core `lsp` types.**
- **`widget.rs`, `geometry.rs`, `content.rs`.**
- **No outbound.** Phase 12.
- **Never reach for `.0` on a `Uri`.**
