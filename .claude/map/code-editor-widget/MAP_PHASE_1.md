# Phase 1 — Crate skeleton + `Content`

## Prerequisites

None. This is the first phase. The repo currently holds only `LICENSE`, `.gitignore`,
`initial_plan.md`, and a stray `package.json`/`node_modules` (biome, unrelated to the Rust build —
leave them alone).

## Goal

Stand up the crate with its pinned iced dependency, and implement `Content` — the type that owns
the `graphics::text::Editor` directly, which is the whole reason this widget exists rather than
wrapping `iced::widget::TextEditor`.

**Exit criteria:** `cargo clippy --all-targets -- -D warnings` is clean, and these pass:

```rust
let content = Content::with_text("héllo\nworld");
assert_eq!(content.text(), "héllo\nworld");
assert_eq!(content.line_count(), 2);
assert!(!content.is_empty());
```

## Why `Content` must be ours

`iced::widget::text_editor::Content` is `Content<R>(RefCell<Internal<R>>)` where the tuple field is
private and `Internal` is a private struct
(`/home/nickel/Programming/repos/iced/widget/src/text_editor.rs:622-631`). There is no accessor,
no `Deref`, nothing. A downstream crate can call `text()`, `cursor()`, `line()` — but can never
reach the `R::Editor` inside, and that editor is the only source of shaped text geometry.

**Before writing code, re-verify this is still true.** If upstream has added an accessor, this
entire architecture becomes unnecessary — stop and report.

## Step 1 — `Cargo.toml`

```toml
[package]
name = "matcha"
version = "0.1.0"
edition = "2024"
rust-version = "1.93"
license = "MIT"
description = "A code editor widget for iced with diagnostics, inlay hints, and a line-number gutter"
repository = "https://github.com/NickelWenzel/matcha"

[dependencies]
iced = { git = "https://github.com/iced-rs/iced", rev = "fa3bae52874274c012a27d4bf11a83c49e1709ae", features = [
    "advanced",
    "highlighter",
] }

[dev-dependencies]
iced = { git = "https://github.com/iced-rs/iced", rev = "fa3bae52874274c012a27d4bf11a83c49e1709ae", features = [
    "advanced",
    "highlighter",
    "fira-sans",
] }
iced_test = { git = "https://github.com/iced-rs/iced", rev = "fa3bae52874274c012a27d4bf11a83c49e1709ae" }
```

Three things here are load-bearing and must not be "simplified":

- **No `default-features = false`.** A renderer backend (`wgpu` or `tiny-skia`, both default) is
  mandatory. Without one, `iced_renderer::Renderer` resolves to `()`
  (`renderer/src/lib.rs:56`) and `<() as text::Renderer>::Editor = ()`
  (`core/src/renderer/null.rs:44`) — the widget's `Renderer::Editor = graphics::text::Editor`
  bound then fails to typecheck outright.
- **`fira-sans` in dev-deps only.** Not a default feature (`iced/Cargo.toml:25` vs `:73`). Without
  it, `font_system()` loads arbitrary system fonts and Phase 3's geometry assertions become
  machine-dependent. iced's own test crate sets it (`test/Cargo.toml:26`).
- **`iced_test` from the same git rev**, never crates.io — a crates.io `iced_test` pulls its own
  `iced_core` and the `Element`/`Renderer` types will not unify.

The first `cargo build` fetches the `hecrj/cosmic-text`, `iced-rs/cryoglyph`, and `iced-rs/winit`
forks. Expect it to be slow; it is not hanging.

## Step 2 — `src/lib.rs`

```rust
//! A code editor widget for [iced], with diagnostic underlines, inlay-hint
//! overlays, and a line-number gutter.
//!
//! [iced]: https://github.com/iced-rs/iced

#![warn(missing_docs)]

pub mod code_editor;

pub use code_editor::{Content, code_editor};
```

`#![warn(missing_docs)]` is on from the start so docs accrete with the code instead of being
retrofitted in Phase 7.

## Step 3 — `src/code_editor.rs`

Module root. **Never `code_editor/mod.rs`.** Phase 1 declares only what exists:

```rust
//! The code editor widget and its supporting types.

mod content;

pub use content::Content;
```

`code_editor` (the helper fn) and the other submodules arrive in later phases; do not stub them.

## Step 4 — `src/code_editor/content.rs`

```rust
//! Owns the shaped text buffer that the widget draws and decorates.

use std::cell::RefCell;

use iced::advanced::graphics::text;
use iced::advanced::text::Editor as _;
use iced::advanced::text::editor::{Action, Cursor, Line, LineEnding};

/// The text content of a [`CodeEditor`](super::CodeEditor).
///
/// Unlike iced's `text_editor::Content`, this exposes the shaped buffer to the
/// widget, which is what makes decoration geometry possible.
pub struct Content(RefCell<text::Editor>);

impl Content {
    /// Creates an empty [`Content`].
    pub fn new() -> Self {
        Self::with_text("")
    }

    /// Creates a [`Content`] holding the given text.
    pub fn with_text(text: &str) -> Self {
        Self(RefCell::new(text::Editor::with_text(text)))
    }

    /// Applies an [`Action`] to the contents.
    pub fn perform(&mut self, action: Action) {
        self.0.borrow_mut().perform(action);
    }

    /// Moves the cursor to the given position.
    pub fn move_to(&mut self, cursor: Cursor) {
        self.0.borrow_mut().move_to(cursor);
    }

    /// Returns the current cursor position.
    pub fn cursor(&self) -> Cursor {
        self.0.borrow().cursor()
    }

    /// Returns the number of lines.
    pub fn line_count(&self) -> usize {
        self.0.borrow().line_count()
    }

    /// Returns the text of the whole buffer.
    pub fn text(&self) -> String {
        self.0.borrow().text()
    }

    /// Returns the selected text, if any.
    pub fn selection(&self) -> Option<String> {
        self.0.borrow().copy()
    }

    /// Returns whether the contents are empty.
    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }

    /// Returns the text of the line at `index`.
    pub fn line(&self, index: usize) -> Option<String> {
        Some(self.0.borrow().line(index)?.text.to_string())
    }

    /// Returns the dominant line ending, if the contents have one.
    pub fn line_ending(&self) -> Option<LineEnding> {
        Some(self.0.borrow().line(0)?.ending)
    }
}
```

`Content::line` returns an owned `String` deliberately. The `Editor` trait's `line()` returns a
borrowed `Line<'_>`, which cannot escape the `RefCell` borrow — so a pass-through signature will
not compile. Owned is what the examples and status bars actually want. Do **not** also add a
closure-taking `with_line` variant; one accessor is enough until something needs zero-copy.

Notes the implementer will hit:

- **`use iced::advanced::text::Editor as _;` is required.** Nearly everything above is a *trait*
  method, not inherent — without the trait in scope none of it resolves. The `as _` form imports
  the trait for method resolution without binding the name, so it does not collide with
  `graphics::text::Editor` (the concrete type) and is **not** the banned `use foo as bar` aliasing.
- **Do not add a `lines()` iterator.** Same borrow problem as `line()`, with no caller in this
  plan. Add it when something needs it.
- **Never clone the inner editor.** `with_internal_mut` does
  `Arc::try_unwrap(..).expect("Editor cannot have multiple strong references")`
  (`graphics/src/text/editor.rs:66`) — a second strong reference panics on the next mutation.
- Add `impl Default for Content` delegating to `new()`. Clippy's `new_without_default` will
  demand it.

## Step 5 — tests

In `content.rs`, at the bottom, after the impls:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_text_round_trips_multibyte_content() {
        let content = Content::with_text("héllo\nworld");

        assert_eq!(content.text(), "héllo\nworld");
        assert_eq!(content.line_count(), 2);
        assert!(!content.is_empty());
    }

    #[test]
    fn a_new_content_is_empty() {
        assert!(Content::new().is_empty());
    }
}
```

Test names read as sentences, no `test_` prefix.

## Files

| File | Change |
| --- | --- |
| `Cargo.toml` | new — pinned iced, dev-deps with `fira-sans` + `iced_test` |
| `src/lib.rs` | new — crate docs, `#![warn(missing_docs)]`, re-exports |
| `src/code_editor.rs` | new — module root declaring `content` |
| `src/code_editor/content.rs` | new — `Content` + tests |
| `.gitignore` | verify `target` is ignored (it already is) |

## Verification

```bash
cargo build                                   # slow first time: fetches three git forks
cargo clippy --all-targets -- -D warnings
cargo test
```

## Spot checks

| Input | Expectation |
| --- | --- |
| `Content::with_text("héllo\nworld").text()` | `"héllo\nworld"` — multibyte survives the round trip |
| `.line_count()` on the above | `2` |
| `Content::new().is_empty()` | `true` |
| `Content::with_text("a\r\nb").line_ending()` | `Some(LineEnding::CrLf)` |
| `Content::with_text("x").cursor()` | `Cursor { position: Position { line: 0, index: 0 }, selection: None }` |

## Do NOT change in this phase

- Do not write any `Widget` impl, builder methods, or decoration types — Phase 2 and 3.
- Do not add a `geometry.rs`, `gutter.rs`, or `decoration.rs`, not even empty.
- Do not add `cosmic-text` as a direct dependency. It must be reached through
  `iced::advanced::graphics::text::cosmic_text` (`graphics/src/text.rs:10`); a crates.io
  `cosmic-text` is a *different crate instance* from the `hecrj` fork and its types will not unify.
- Do not make `Content` generic over `Renderer`. The concrete form is deliberate — see MAP_PLAN
  "Key design decisions".
- Do not touch `initial_plan.md`, `package.json`, or `node_modules`.
