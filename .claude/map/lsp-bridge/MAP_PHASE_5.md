# Phase 5 — Applying edits: validation and commit

The riskiest phase in the plan. Everything here was measured; three of the rules
below were bugs in earlier drafts that would have corrupted the user's file
silently.

The caret is left wherever `Edit::Paste` puts it — **Phase 6** restores it.

## Prerequisites

Phase 2 (`Bridge::resolve`, `Replacement`) and Phase 3 (`Content::revision`).

## Goal and exit criteria

`Content::apply` applies one document's edits, all-or-nothing, refusing a buffer
that has moved. Done when the case table passes against the real editor.

## Step 1 — `lsp/document.rs`

```rust
/// One document's worth of edits, as a server grouped them.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    /// The document, as the server spelled it. matcha never interprets this;
    /// it is the application's routing key.
    pub uri: String,
    /// The version the server computed these against, if it said.
    ///
    /// [`Content::apply`] **ignores this** and takes a local revision instead:
    /// matcha does not own the `didChange` counter, so only the application can
    /// map one to the other. See [`Content::revision`].
    pub version: Option<i32>,
    /// The edits, in the order the server sent them. The order matters for
    /// several inserts at one position.
    pub edits: Vec<Change>,
}
```

## Step 2 — `lsp/replacement.rs` gains `Change`

```rust
/// An element of a document's edit list.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// An ordinary replacement.
    Replace(Replacement),
    /// A snippet, which matcha cannot apply.
    Snippet(Snippet),
}

/// A snippet edit.
///
/// Represented rather than dropped, and refused rather than applied: its text
/// is snippet *syntax*, so inserting it literally would put `$0` and `${1:x}`
/// into the user's buffer. An application that wants snippets expands them
/// itself and calls [`Content::apply`] with the result.
#[derive(Debug, Clone, PartialEq)]
pub struct Snippet {
    /// What it would replace.
    pub range: Range,
    /// Snippet syntax, **not** literal text.
    pub value: String,
    /// The change annotation this belongs to, if any.
    pub annotation_id: Option<String>,
}
```

## Step 3 — `lsp/error.rs`

The crate's first error type. Hand-written `Display` and `std::error::Error` —
`thiserror` would be a second dependency. `#[non_exhaustive]`, and a doc on
every variant *and every field*, because `missing_docs` is on.

Every variant names the failing edit **by its index in the caller's array**,
which is why the sort has to carry original indices. "Edit 7 is bad" is
actionable; `Err(())` is not.

```rust
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The buffer changed since the revision the caller passed.
    Stale {
        /// What the caller expected.
        expected: u64,
        /// What the buffer is at now.
        actual: u64,
    },
    /// An edit named a line the buffer does not have.
    LineOutOfBounds { edit: usize, line: u32, lines: usize },
    /// An edit named a column past the end of its line.
    ///
    /// The decoration path clamps this; an edit must not, because clamping
    /// turns "replace columns 10..20" into a no-op insert at the line's end.
    ColumnOutOfBounds { edit: usize, character: u32, line_len: usize },
    /// An edit named a column inside a character.
    NotACharBoundary { edit: usize, position: Position },
    /// Two edits overlap. Both indices, because one alone is unactionable.
    Overlapping { edit: usize, other: usize },
    /// An edit's range ends before it starts.
    ///
    /// Rejected rather than repaired. See [`Range::collapsed`] for the
    /// normalization servers expect, which the caller applies if it wants it.
    ReversedRange { edit: usize },
    /// The batch contained a [`Change::Snippet`].
    Unsupported { edit: usize },
}
```

## Step 4 — `Content::apply`, in `lsp/apply.rs`

```rust
impl Content {
    /// Applies one document's edits to this buffer.
    ///
    /// All-or-nothing: every edit is validated before any is committed, so a
    /// failure leaves the buffer untouched. That is deliberately unlike the
    /// decoration path, which silently ignores what it cannot place — half an
    /// applied code action is a corrupted file, and a missing squiggle is not.
    ///
    /// Returns the buffer's new [`revision`](Content::revision), so the caller
    /// can pair it with its next `didChange` without a second call.
    ///
    /// `expected` is a revision taken when the request was sent. The edits describe the document as it was then, so if the
    /// buffer has moved they would land on the wrong bytes.
    pub fn apply(
        &mut self,
        edit: &document::Edit,
        encoding: Encoding,
        expected: u64,
    ) -> Result<u64, Error>;
}
```

### The algorithm, in order

1. **Refuse a stale buffer.** `if self.1 != expected { return Err(Stale { .. }) }`.
2. **Refuse snippets.** Any `Change::Snippet` is `Err(Unsupported { edit })`.
3. **Convert every range through `Bridge::resolve`**, mapping each `Reason` plus
   the edit's original index onto an `Error` variant. This is why `resolve`
   exists and why `exact` is not used here — `exact` discards the reason.
4. **Refuse a reversed range.** `Err(ReversedRange { edit })`.
5. **Sort `(start, end)`, stable ascending, carrying the original index.**
6. **Check overlaps** in byte coordinates, strict `<`,
   `last_end = end.max(last_end)`.
7. **Normalize line endings.**
8. **Commit, iterating the sorted list in reverse.**

### The sort key — every word measured

`(start, end)`, **stable ascending, iterated in reverse**.

- **`(start, end)`, not `start`.** LSP permits "any number of inserts followed by
  a single remove or replace" at one position, and says the array need not be
  ordered. On `"XYZ"` with `[replace(0..2,"Q"), insert(0,"A")]`, which must give
  `"AQZ"`: sorting by `start` alone yields **`"QYZ"`** — the insert lost, the
  wrong range replaced — *and* falsely trips the overlap guard. `(start, end)`
  is correct in either array order, because an insert (`end == start`) sorts
  before a replace at the same point.
- **Ascending then reversed, not descending.** A stable *descending* sort
  preserves array order among equal keys and therefore **reverses** several
  inserts at one position: `"XY"` + `A`,`B` gives `"XBAY"` instead of `"XABY"`;
  with three inserts, `"321Z"` instead of `"123Z"`.
- **Reversed.** Back-to-front keeps earlier offsets valid, **and** makes the
  final `perform` the topmost edit. `topmost_line_changed` is overwritten by
  each `perform` (`iced/graphics/src/text/editor.rs:535`) and consumed once per
  frame (`:753-756`), so ascending order leaves syntax highlighting stale above
  the last edit.
- **The original index** is carried only so `Error` can name the failing edit.
  The stable sort already handles ordering.

### Line endings

Split `new_text` on `\r\n | \n | \r` and rejoin with the buffer's ending.
Idempotent by construction, which is the point: rust-analyzer already emits
`\r\n` for a DOS file, so "already CRLF" is the **common** path and a naive
`replace('\n', "\r\n")` would produce `\r\r\n`.

**When the target is unknown, do not normalize at all.**
`Content::line_ending()` returns `Option<LineEnding>` and reads line 0
(`content.rs:93-95`). There are two `None`s to handle — the outer `Option`, and
`Some(LineEnding::None)` — and `unwrap` is banned. A buffer with one line and no
trailing newline reports `LineEnding::None`, whose `as_str()` is `""`; measured,
normalizing against it turns `"a\nb"` into `"ab"`. **Including `Content::new()`,
which is every application's starting state.**

Defaulting to `Lf` instead is also wrong: a one-line buffer carries no evidence
of the file's convention, so an edit from a DOS file would have its `\r\n`
rewritten and the user's file silently converted. Insert verbatim and let
cosmic-text's `LineIter` record whatever arrives — with no evidence, that is the
only defensible answer.

### Committing

Per edit, in reverse sorted order:

```rust
self.move_to(Cursor { position: start, selection: Some(end) });
self.perform(Action::Edit(Edit::Paste(Arc::new(new_text))));
```

`Edit::Paste` is `insert_string`, which deletes the selection first, so this is
an atomic replace — correct for multi-line `new_text` and for ranges spanning
newlines. Verified against the real editor.

**Do not use `Editor::overwrite`.** It is one reshape and looks tempting, but it
sets no `topmost_line_changed` (stale syntax colours) and records no `Change`,
so a later undo runs `delete_range` with cursors into the old text.

Each `perform` bumps the revision, so the counter advances by the number of
edits. Only equality against `expected` is meaningful.

## Verification

```sh
cargo test --features lsp
cargo clippy --all-targets --features lsp -- -D warnings
```

## Spot checks

Against a real `Content`. The first eight were run during planning and pass.

| case | expected |
|---|---|
| `"let x = 1;"`, replace `(0,4)..(0,5)` with `"answer"` | `"let answer = 1;"` |
| three edits on three lines, given ascending | all three land |
| replace with `""` (pure deletion) | the range is removed — **load-bearing**: it works only because `insert_string` deletes the selection before `insert_at`'s empty-data early return |
| multi-line `new_text` | real lines, `line_count` grows |
| range spanning a newline | lines join |
| `(0,3)..(1,0)` with `""` | exactly one line break removed |
| multibyte range on char boundaries | replaced correctly |
| two inserts at one position, `[A, B]` | `"AB"`, never `"BA"` |
| `[replace(0..2,"Q"), insert(0,"A")]` over `"XYZ"` | `"AQZ"` — **and the same for the reversed array** |
| CRLF buffer, `new_text` containing `\n` | endings stay uniform |
| CRLF buffer, `new_text` already CRLF | unchanged, not `\r\r\n` |
| `Content::new()`, `new_text` `"a\nb"` | `"a\nb"` — newlines survive |
| single-line buffer, no trailing newline | as above |
| reversed range | `Err(ReversedRange)`, buffer untouched |
| column past end of line | `Err(ColumnOutOfBounds)`, buffer untouched |
| column inside a character | `Err(NotACharBoundary)` |
| char boundary inside the 25-byte ZWJ family emoji | neither panics nor mangles the cluster |
| line `line_count`, column 0 (append at EOF) | appends |
| line beyond that | `Err(LineOutOfBounds)` |
| a `Change::Snippet` anywhere in the batch | `Err(Unsupported)`, nothing applied |
| stale revision | `Err(Stale)`, buffer untouched |
| a successful 3-edit batch | `revision()` advances by **3**; the returned value equals it |
| empty `edits` | `Ok(revision)` unchanged, no-op, **no undo entry** |

Every failing case must also assert the buffer is **byte-identical** afterwards.
That is the all-or-nothing property, and a test that only checks the `Err` is
vacuous.

## What NOT to change

- **`widget.rs`, `geometry.rs`.**
- **The caret.** Phase 6. Leaving it wrong here is deliberate so the two can be
  tested apart.
- **`Editor::overwrite`.** Never.
- **`Bridge::exact` for conversion here.** Use `resolve`; `exact` throws away
  what `Error` needs.
- **`document::Edit.version`.** Read by the application, ignored by `apply`.
