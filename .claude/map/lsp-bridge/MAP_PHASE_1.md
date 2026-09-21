# Phase 1 — Feature scaffold

The crate's **first** `[features]` table and **first** `#[cfg(feature)]`.
Nothing converts yet; this phase exists so that every later phase has a gated
module to add to, and so the two-build lint matrix is in place before there is
anything complicated to lint.

## Prerequisites

None. This is the first phase. The tree is green at `2533bf9` on branch
`lsp_bridge`: 69 unit + 5 behaviour + 2 doctests pass, 4 snapshots are
`#[ignore]`d, clippy and fmt are clean.

## Goal and exit criteria

When this phase is done:

- `cargo build` and `cargo build --features lsp` both succeed.
- `cargo clippy --all-targets -- -D warnings` and
  `cargo clippy --all-targets --features lsp -- -D warnings` are both clean.
- `cargo test` still reports the same 69 + 5 + 2.
- `matcha::lsp::{Position, Range, Encoding}` exist and are documented.
- `cargo doc --no-deps --features lsp` renders the module.

## Step 1 — `Cargo.toml`

Add **only** `lsp = []`. Each conversion phase adds its own feature and optional
dependency later; do not add `lsp-types` or `gen-lsp-types` here.

The file's house style is that every non-obvious stanza carries a comment
explaining the consequence of getting it wrong — match it.

```toml
# The LSP bridge is optional and additive. `lsp` itself pulls in nothing: the
# application hands over values it has already decoded, so nothing here
# deserializes and the feature costs no dependency at all. What it gates is
# API surface, which is the honest reason rather than dependency weight.
#
# `lsp-types` and `gen-lsp-types` arrive in later phases and each add one
# optional dependency on a permissive version range.
[features]
lsp = []

[package.metadata.docs.rs]
# Without this docs.rs renders none of the gated modules.
all-features = true
```

## Step 2 — the module

Create `src/code_editor/lsp.rs`, `src/code_editor/lsp/position.rs` and
`src/code_editor/lsp/encoding.rs`.

**The placement is load-bearing and verified.** `src/code_editor/lsp.rs` is a
descendant of `code_editor`, so it can reach `Content`'s `pub(super)` field
(`content.rs:17`) and later borrow line text with zero allocation. A module at
`src/lsp.rs` cannot. Do not move it.

`src/code_editor.rs` gains one line beside the existing `pub mod` declarations:

```rust
#[cfg(feature = "lsp")]
pub mod lsp;
```

and `src/lib.rs` re-exports it in the existing `pub use` block — the same shape
`decoration` and `gutter` already use:

```rust
#[cfg(feature = "lsp")]
pub use code_editor::lsp;
```

## Step 3 — `lsp::Position` and `lsp::Range`

In `lsp/position.rs`. Two things are not negotiable.

**`line` is declared before `character`.** The derived `Ord` is lexicographic in
declaration order, and Phase 5 sorts edits by position. Swapping the fields
would sort by column across lines and corrupt text.

**This type must stay distinct from `matcha::Position`.** That separation is
what makes a wire position unusable as an index until it has been through
`Bridge::clamp` or `Bridge::exact` — the compiler enforces what a comment
otherwise only asks for. Nothing outside `lsp/bridge.rs` may convert between
them, and there is no `From` impl in either direction.

```rust
//! Positions as a language server counts them.

/// A position in a language server's coordinates.
///
/// The `character` is counted in whatever [`Encoding`] the server negotiated —
/// UTF-16 code units unless it said otherwise — which is why this is a
/// different type from [`matcha::Position`], whose `index` is a UTF-8 byte
/// offset. Converting between them needs the text of the line, and is
/// [`Bridge`]'s job.
///
/// [`matcha::Position`]: crate::Position
/// [`Bridge`]: super::Bridge
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    /// Zero-based line number.
    ///
    /// Declared before `character` so that the derived [`Ord`] compares lines
    /// first. Phase 5 sorts text edits by this ordering.
    pub line: u32,
    /// Zero-based offset into the line, in the negotiated encoding's code units.
    pub character: u32,
}

/// A range of text in a language server's coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Range {
    /// Where the range starts.
    pub start: Position,
    /// Where it ends, exclusive.
    pub end: Position,
}

impl Range {
    /// The range with its start moved to its end.
    ///
    /// The de-facto normalization for a range whose end precedes its start,
    /// which the protocol does not define and which several servers emit
    /// anyway. VS Code caps the start to the end, and servers target that —
    /// so this is an *insert at `end`*, and deliberately **not** a swap:
    /// swapping would delete everything between two endpoints the server
    /// meant to collapse.
    ///
    /// [`Content::apply`] rejects a reversed range rather than guessing; this
    /// is what an application calls to recover.
    ///
    /// [`Content::apply`]: crate::Content::apply
    pub fn collapsed(self) -> Self {
        Self {
            start: self.end,
            end: self.end,
        }
    }
}
```

## Step 4 — `lsp::Encoding`

In `lsp/encoding.rs`. No conversion logic yet — that is Phase 2. Only the enum,
its `Default`, and the documentation of why the default is what it is.

```rust
//! How a language server counts the `character` in a position.

/// How a language server counts the `character` in a [`Position`].
///
/// Negotiated during initialization, which matcha takes no part in: the
/// application performs the handshake and passes the result here.
///
/// [`Position`]: super::Position
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    /// UTF-8 bytes — the same units the editor stores.
    Utf8,
    /// UTF-16 code units.
    ///
    /// The protocol's default, and the only encoding a server is obliged to
    /// support. An unrecognised `positionEncoding` string means this, per
    /// spec, rather than an error.
    #[default]
    Utf16,
    /// UTF-32 code units, which is to say Unicode scalar values.
    Utf32,
}
```

## Step 5 — the module root

`src/code_editor/lsp.rs` declares the submodules and re-exports their types flat,
the way `decoration.rs` does. It also carries the module documentation, which is
where a reader arriving from the crate root learns what the feature is for.

```rust
//! Translating language-server messages into the editor's own terms.
//!
//! Available with the `lsp` feature. matcha takes no part in the protocol
//! itself — there is no transport here, no JSON-RPC framing and no handshake.
//! The application decodes a message and hands the value over; what this module
//! does is convert positions between the encoding the server negotiated and the
//! UTF-8 byte offsets the editor stores, and turn the results into the
//! decorations the widget already draws.
//!
//! The `lsp-types` and `gen-lsp-types` features add conversions to and from
//! those crates, so an application does not have to write the mapping itself.

mod encoding;
mod position;

pub use encoding::Encoding;
pub use position::{Position, Range};
```

## Verification

```sh
cargo build
cargo build --features lsp
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features lsp -- -D warnings
cargo fmt --all -- --check
cargo test
cargo doc --no-deps --features lsp
```

All must pass. `cargo test` must still report **69 unit, 5 behaviour, 2 doctests,
4 ignored** — this phase adds no tests and must not change any count.

## Spot checks

| check | expected |
|---|---|
| `Encoding::default()` | `Encoding::Utf16` |
| `Position { line: 1, index: 0 } > Position { line: 0, index: 99 }` (as `lsp::Position`, `character` for `index`) | `true` — `Ord` compares lines first |
| `Range { start: a, end: b }.collapsed()` where `a < b` | `Range { start: b, end: b }` |
| `cargo build` with no features | succeeds; `matcha::lsp` does not exist |
| `grep -c "cfg(feature" src/code_editor/lsp.rs` | 0 — the whole module is gated at its declaration, not item by item |

These three tests are **verified** — the whole of this phase's code was
compiled in a scratch crate under `clippy --all-targets -D warnings` with
`missing_docs` on, with and without the feature, and they pass. Put them in
`lsp/position.rs` and `lsp/encoding.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    #[test]
    fn a_collapsed_range_becomes_an_insert_at_its_end() {
        let range = Range {
            start: at(0, 4),
            end: at(0, 9),
        };
        let collapsed = range.collapsed();

        assert_eq!(collapsed.start, collapsed.end);
        assert_eq!(
            collapsed.start, range.end,
            "an insert AT end, never a swap"
        );
    }

    #[test]
    fn a_position_orders_by_line_before_column() {
        assert!(
            at(1, 0) > at(0, 99),
            "Ord must compare lines first, or Phase 5 sorts edits by column"
        );
    }

    #[test]
    fn the_default_encoding_is_the_protocols_own() {
        assert_eq!(Encoding::default(), Encoding::Utf16);
    }
}
```

## What NOT to change

- **`src/code_editor/widget.rs` and `src/code_editor/geometry.rs`.** If this
  phase seems to need either, something has gone wrong — stop and report.
- **`src/code_editor/content.rs`.** It gains a revision counter in Phase 3 and
  nothing before then.
- **The pinned iced rev**, in `Cargo.toml`, `README.md` or `src/lib.rs`.
- **`README.md` and the crate docs in `src/lib.rs`.** They still say the LSP
  protocol is out of scope. That prose is rewritten in Phase 13, deliberately,
  once there is something to describe.
- **No conversion logic.** `Encoding` has no methods in this phase. Adding
  `width()` or a column converter here would leave dead code that fails
  `clippy -D warnings`.
