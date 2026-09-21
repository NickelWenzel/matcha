# Phase 3 — Revision counting, and what a converted position means

The smallest phase, and the one `Content::apply` depends on for honesty.

A language server's response describes the document at some *earlier* version.
Converting it against the current buffer is fine for a decoration — the drift is
visible and the next publish fixes it — and is silent corruption for an edit,
which lands on bytes that have moved. matcha does not own the LSP version
counter, because the application sends `didChange`. What it can own is a local
revision, so the application only has to remember a pairing.

## Prerequisites

Phase 2: `Bridge` exists and converts.

## Goal and exit criteria

`Content::revision()` exists, changes on edits and nothing else, and the crate
documents that converted positions are a snapshot rather than an anchor.

## Step 1 — the counter

`Content` is a **tuple struct** (`content.rs:17`), and `self.content.0` appears
at `widget.rs:443, 499, 606, 930, 1075` plus roughly sixteen test sites.
Converting it to a named-field struct would edit `widget.rs`, which constraint 1
defines as the design having broken. So the counter is a **second tuple
element**:

```rust
pub struct Content(pub(super) RefCell<text::Editor>, pub(super) u64);
```

**Both elements are `pub(super)`.** The accessor lives in `lsp/bridge.rs`,
which is a *sibling* of `content`, not a descendant — a bare `u64` is private to
`content` and `self.1` from there is `error[E0616]`. Verified by compiling.

A plain `u64`, not a `Cell`: every mutator takes `&mut self`, so interior
mutability buys nothing and would allow a bump through `&Content` while a
`Bridge` is alive.

The field and its bumps are **unconditional** — no `#[cfg]` in `content.rs`.
Only the accessor is gated, and it lives in `lsp/bridge.rs`, so `content.rs`
never sees a feature gate.

Every constructor gains the initial value:

```rust
pub fn with_text(text: &str) -> Self {
    Self(RefCell::new(text::Editor::with_text(text)), 0)
}
```

`Clone` routes through `with_text` (`content.rs:33-35`) and therefore **restarts
at 0**. Either choice breaks monotonicity across a clone; restarting is the safe
direction, because a revision recorded before the clone then reads as *stale*
rather than falsely fresh.

## Step 2 — bump on edits only

```rust
pub fn perform(&mut self, action: editor::Action) {
    // Not every action: the widget publishes ALL of them for the application
    // to feed back (`widget.rs:524`), so `Scroll`, `Click`, `Drag`, `Move` and
    // `Select*` all arrive here. Bumping on those would change the revision on
    // every mouse-move of a drag-select, and `apply` would then refuse a
    // buffer whose text never changed.
    //
    // `is_edit()` is iced's own (`core/src/text/editor.rs:148-152`) and is
    // `matches!(self, Self::Edit(_))`, so it correctly includes Undo and Redo.
    if action.is_edit() {
        self.1 += 1;
    }
    self.0.borrow_mut().perform(action);
}
```

`move_to` does **not** bump: it moves the caret and changes no text.

## Step 3 — the accessor

In `lsp/bridge.rs`, gated:

```rust
impl Content {
    /// How many times this content has been edited.
    ///
    /// Bumped by every edit — [`perform`] with an [`Action::Edit`], including
    /// undo and redo, and [`apply`] — and by nothing else. Moving the caret,
    /// scrolling, clicking and dragging all leave it alone.
    ///
    /// It exists so an edit can be refused when the buffer has moved under it.
    /// A language server answers about the document as it was when the request
    /// was made; applying those ranges to a buffer that has since changed edits
    /// the wrong bytes, silently. matcha cannot detect that on its own, because
    /// the application owns the `didChange` version counter — so record the
    /// pair when sending, and hand the revision back to [`apply`]:
    ///
    /// ```ignore
    /// let sent = (lsp_version, content.revision());
    /// // ... later, when the response arrives ...
    /// content.apply(&edit, encoding, sent.1)?;
    /// ```
    ///
    /// Meaningful only within one [`Content`]; a clone starts again at 0.
    ///
    /// [`apply`]: Content::apply
    pub fn revision(&self) -> u64 {
        self.1
    }
}
```

`apply` does not exist yet, so the doctest is `ignore`. Phase 5 turns it into a
real example.

## Step 4 — the documentation that makes the decoration path honest

Add to the `lsp` module docs:

> **Converted positions are a snapshot.** A position that has been through the
> bridge is a byte offset into the buffer *as it was at that moment*. It is not
> an anchor: it does not follow later edits, and nothing here updates it. That
> is why decorations are replaced wholesale on every round trip rather than
> patched, and why drift between one publish and the next is expected rather
> than a bug. For an edit, where drift is not survivable,
> [`Content::apply`] refuses a buffer whose [`revision`] has moved.

This matters because the crate's existing promise (`lib.rs:102-110`) is about
positions that no longer *resolve*. This is about positions that still resolve
and now mean something else. Do not let the docs blur them.

## Verification

```sh
cargo test --features lsp
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features lsp -- -D warnings
cargo doc --no-deps --features lsp
```

The no-feature clippy run matters here: the field and its bumps are
unconditional, so a default build compiles them. `self.1 += 1` counts as a read,
so `dead_code` stays quiet.

## Spot checks

| action | revision changes? |
|---|---|
| `Content::with_text(..)` | starts at 0 |
| `perform(Action::Edit(Edit::Insert('a')))` | yes |
| `perform(Action::Edit(Edit::Undo))` | yes |
| `perform(Action::Edit(Edit::Redo))` | yes |
| `perform(Action::Move(..))` | **no** |
| `perform(Action::Select(..))`, `SelectWord`, `SelectLine`, `SelectAll` | **no** |
| `perform(Action::Click(..))`, `Drag(..)` | **no** |
| `perform(Action::Scroll { .. })` | **no** |
| `move_to(..)` | **no** |
| `text()`, `line()`, `cursor()`, any read | **no** |
| `clone()` | the clone starts at 0 |

`apply`'s effect on the counter is **Phase 5's** exit criterion, not this one —
it does not exist yet.

A drag-select is the case worth a named test of its own: a `Click` followed by
several `Drag`s and a `Move` must leave the revision untouched. That is the
regression the `is_edit()` gate exists to prevent.

## What NOT to change

- **`widget.rs`.** The counter is a second tuple element precisely so that
  `self.content.0` keeps working everywhere it already appears.
- **The `pub(super)` visibility of the editor.** Unchanged.
- **`Content::clone`'s reshape.** It stays as it is; only the revision resets.
- **No `apply` yet.** Phase 5.
