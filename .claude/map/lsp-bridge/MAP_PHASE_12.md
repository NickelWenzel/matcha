# Phase 12 — `gen-lsp-types`, outbound and client capabilities

Phase 10's surface, for the other family.

## Prerequisites

Phase 11: `--features gen-lsp-types` exists and converts inbound.

## What differs from Phase 10

- **URIs are infallible at default features.** `Uri::from(String)` — the default
  `Uri` is `struct Uri(pub String)`. **Keep the `TryFrom` shape anyway**, for
  symmetry with Phase 10 and because the `url` feature makes it fallible again,
  and a transitive dependency can turn that on without the application asking.
- **`Change::Snippet` converts cleanly here**, unlike Phase 10:
  `gen-lsp-types` has `SnippetTextEdit`, so a snippet round-trips instead of
  erroring. This is the one place the two families genuinely differ in what they
  can express.
- **Integer enums go out through `u32`**, the same rule as inbound. Do not
  construct variants by name — 0.11's `Custom(u32)` does not exist at 0.9.
- **`LspAny`**, not `LSPAny`.

## `client_capabilities()`

```rust
pub mod from_gen_lsp_types {
    /// Exactly what this bridge understands, prefilled.
    pub fn client_capabilities() -> gen_lsp_types::ClientCapabilities;
}
```

Namespaced for the same reason as Phase 10: `--all-features` builds both, and
two free functions cannot share a path. The advertised set is the same — see
Phase 10 for what each entry buys and why `normalizesLineEndings` must be
`true`.

## Verification

```sh
cargo test --features gen-lsp-types
cargo clippy --all-targets --features gen-lsp-types -- -D warnings
cargo clippy --all-targets --all-features -- -D warnings

for v in 0.9.0 0.10.0 0.11.0; do
  cargo update -p gen-lsp-types --precise $v && cargo build --features gen-lsp-types
done
git checkout -- Cargo.lock
```

## Spot checks

| case | expected |
|---|---|
| `Replacement` → `gen_lsp_types` → back | compares equal |
| `workspace::Edit` with operations | compares equal, order preserved |
| **`Change::Snippet`** | **round-trips** — unlike Phase 10, where it errors |
| `Severity::Error` out | integer 1, not a named variant |
| **`Diagnostic`** | **deliberately lossy** — `message` is a union inbound. Assert what survives |
| `--all-features` | both `client_capabilities` coexist |
| builds at 0.9.0 and 0.11.0 | both |

## What NOT to change

- **Phase 11's inbound impls.**
- **Phase 10's rules.** Snippets error there and round-trip here; that asymmetry
  is real and deliberate.
- **`widget.rs`, `geometry.rs`, `content.rs`.**
- **Never leave `Cargo.lock` modified.**
