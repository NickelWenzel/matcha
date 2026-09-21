# Phase 2 — Encoding conversion, the `Bridge`, and `Replacement`

The correctness core, and the first public surface anyone can call. Everything
later in the plan converts positions through what this phase builds.

## Prerequisites

Phase 1 is done: `[features] lsp = []` exists, `matcha::lsp` exists and exports
`Position`, `Range`, `Range::collapsed` and `Encoding`.

## Goal and exit criteria

`Content::lsp(encoding)` returns a `Bridge`, and the `Bridge` converts positions
both ways in all three encodings. `Replacement` exists as a plain data type.
Nothing draws, nothing mutates.

Done when the test table below passes and both feature builds lint clean.

## Step 1 — `lsp/encoding.rs` gains the width function

```rust
impl Encoding {
    /// How many code units `c` occupies in this encoding.
    pub(crate) fn width(self, c: char) -> u32 {
        match self {
            Encoding::Utf8 => c.len_utf8() as u32,
            Encoding::Utf16 => c.len_utf16() as u32,
            // One code unit per scalar value, which is what a `char` is.
            Encoding::Utf32 => 1,
        }
    }
}
```

## Step 2 — `lsp/bridge.rs`

### `Reason`, and why each variant carries a payload

`resolve` is the only scan. Both of its consumers need something back from a
failure — `clamp` needs somewhere to clamp *to*, `apply` needs something to
report — so the payload is what stops this becoming two converters.

```rust
/// Why a position did not resolve exactly.
///
/// `pub(crate)`: [`Content::apply`] is the only consumer outside this module,
/// and each variant carries both what [`Bridge::clamp`] recovers with and what
/// [`Error`] reports. Without the payloads `clamp` would have to re-scan,
/// which is the second converter this design exists to avoid.
// `lines` is read only by `Content::apply`, which arrives in Phase 5.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum Reason {
    /// The line is past the end of the buffer. Carries the buffer's line count.
    Line { lines: usize },
    /// The column is past the end of its line. Carries the line's byte length.
    Column { line_len: usize },
    /// The column landed inside a character. Carries that character's start.
    NotACharBoundary { floor: usize },
}
```

### The converter

Read the line through the `pub(super)` field — a module under `code_editor` can,
which is why the module lives where it does — so no `String` is allocated.

```rust
impl<'a> Bridge<'a> {
    /// The one scan. Everything else is derived from it.
    pub(crate) fn resolve(&self, from: Position) -> Result<crate::Position, Reason> {
        let editor = self.content.0.borrow();
        let lines = editor.line_count();

        // `line == lines` is the LSP end-of-document idiom, not an error: a
        // whole-file format is one edit over `0:0 .. lineCount:0`. The
        // character is ignored on that line.
        if from.line as usize == lines {
            let last = lines.saturating_sub(1);
            let len = editor.line(last).map_or(0, |l| l.text.len());
            return Ok(crate::Position { line: last, index: len });
        }
        if from.line as usize > lines {
            return Err(Reason::Line { lines });
        }

        let line = editor.line(from.line as usize).ok_or(Reason::Line { lines })?;
        let text = line.text.as_ref();

        // Source is overwhelmingly ASCII, and `is_ascii` is vectorised, so this
        // is one pass over bytes with no `char` decoding. Per LINE, never per
        // position -- a per-position check on a 100 KB minified line would add
        // a second full scan to every lookup.
        if text.is_ascii() {
            let want = from.character as usize;
            return if want <= text.len() {
                Ok(crate::Position { line: from.line as usize, index: want })
            } else {
                Err(Reason::Column { line_len: text.len() })
            };
        }

        let mut units = 0u32;
        for (byte, c) in text.char_indices() {
            if units == from.character {
                return Ok(crate::Position { line: from.line as usize, index: byte });
            }
            units += self.encoding.width(c);
            // Overshot: `from.character` points inside `c`, which is only
            // reachable in UTF-16 between the halves of a surrogate pair, and
            // in UTF-8 anywhere inside a multibyte character.
            if units > from.character {
                return Err(Reason::NotACharBoundary { floor: byte });
            }
        }

        if units == from.character {
            Ok(crate::Position { line: from.line as usize, index: text.len() })
        } else {
            Err(Reason::Column { line_len: text.len() })
        }
    }

    /// Total. Never fails; see the plan's *Divergences* section for why the
    /// decoration path clamps where the edit path refuses.
    pub fn clamp(&self, from: Position) -> crate::Position {
        match self.resolve(from) {
            Ok(position) => position,
            Err(Reason::NotACharBoundary { floor }) => {
                crate::Position { line: from.line as usize, index: floor }
            }
            Err(Reason::Column { line_len }) => {
                crate::Position { line: from.line as usize, index: line_len }
            }
            // A line past the end passes through UNCONVERTED, with column 0.
            // Clamping it to the end of the buffer would paint a squiggle at
            // the end of the last line under unrelated text, because
            // `range_fragments` widens a zero-width range. Passed through, the
            // widget drops it silently, exactly as `lib.rs:102-110` documents.
            Err(Reason::Line { .. }) => {
                crate::Position { line: from.line as usize, index: 0 }
            }
        }
    }

    /// Fallible. `None` unless the position resolves exactly -- except for the
    /// end-of-document idiom, which `resolve` accepts.
    pub fn exact(&self, from: Position) -> Option<crate::Position> {
        self.resolve(from).ok()
    }
}
```

### Ranges

`range` is **not** two independent `clamp` calls. A range with a live start and
a stale end is the trap: `range_fragments` filters runs to `[start.line,
end.line]`, so every run from the start onwards survives and `highlight` reports
each fully selected — a full-width squiggle from the start to EOF.

```rust
/// `None` when either endpoint names a line the buffer does not have.
pub fn range(&self, from: Range) -> Option<decoration::TextRange> {
    let lines = self.content.0.borrow().line_count();
    if from.start.line as usize > lines || from.end.line as usize > lines {
        return None;
    }
    // `TextRange::new` orders its endpoints, so a reversed range normalizes
    // here. That is right for a decoration and WRONG for an edit, which is why
    // `Content::apply` rejects one instead of calling this.
    Some(decoration::TextRange::new(self.clamp(from.start), self.clamp(from.end)))
}
```

### Outbound

```rust
/// `None` if the position is not in the buffer, is off a char boundary, or
/// does not fit in the `u32` the protocol uses.
pub fn locate(&self, at: crate::Position) -> Option<Position> {
    let editor = self.content.0.borrow();
    let line = editor.line(at.line)?;
    let text = line.text.as_ref();
    if at.index > text.len() || !text.is_char_boundary(at.index) {
        return None;
    }

    let character = if text.is_ascii() {
        at.index as u32
    } else {
        text[..at.index].chars().map(|c| self.encoding.width(c)).sum()
    };

    Some(Position {
        // `try_from`, never `as`: `crate::Position` is `usize` and clippy
        // allows a silent truncating cast by default.
        line: u32::try_from(at.line).ok()?,
        character,
    })
}
```

## Step 3 — the batch walk

The batch methods arrive in Phase 4, but the walk they need belongs here with
the rest of the conversion. Sort the positions by `(line, character)`, scan each
line once carrying the cursor, and scatter back through the permutation.

Calling `resolve` per position is O(positions × line length). One minified
bundle — 100 KB on a single line, 2000 diagnostics — is roughly 800 MB of
scanning per publish.

It has no caller until Phase 4, so it carries `#[allow(dead_code)]` with a
comment naming Phase 4 as the remover. Without that, this phase fails its own
`clippy -D warnings` exit.

## Step 4 — `lsp/replacement.rs`

Three plain fields, and it ships **here** rather than with `apply`: Phase 4's
`Hint.text_edits` needs it, and making Phase 4 wait on Phase 5 would serialize
two phases that are otherwise independent.

```rust
/// A range of text and what replaces it. LSP calls this a `TextEdit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    /// What to replace.
    pub range: Range,
    /// What to replace it with. Its line endings are normalized to the
    /// buffer's by [`Content::apply`], not here.
    pub new_text: String,
    /// The `ChangeAnnotation` this belongs to, if any. matcha never prompts;
    /// an application that honours `needs_confirmation` partitions on this
    /// before calling [`Content::apply`].
    pub annotation_id: Option<String>,
}
```

## Verification

```sh
cargo build && cargo build --features lsp
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features lsp -- -D warnings
cargo test --features lsp
cargo fmt --all -- --check
```

## Spot checks

**The code above was compiled and run against this whole table before it was
written down** — all 14 rows pass, as does the round trip below. It is
transcribed from a working probe, not sketched.

Fixture `"héllo"` — bytes `h`=0, `é`=1..3, `l`=3, `l`=4, `o`=5.
Fixture `"a😀b"` — bytes `a`=0, emoji=1..5, `b`=5; UTF-16 units `a`=0,
emoji=1..3, `b`=3.

| line | column | encoding | `clamp` | `exact` |
|---|---|---|---|---|
| `"hello"` | 3 | Utf16 | 3 | `Some(3)` |
| `"hello"` | 99 | Utf16 | 5 | `None` |
| `"héllo"` | 2 | Utf16 | 3 | `Some(3)` |
| `"héllo"` | 3 | Utf8 | 3 | `Some(3)` |
| `"héllo"` | 2 | Utf32 | 3 | `Some(3)` |
| `"a😀b"` | 3 | Utf16 | 5 | `Some(5)` |
| `"a😀b"` | **2** | Utf16 | **1** | **`None`** |
| `"a😀b"` | 2 | Utf32 | 5 | `Some(5)` |
| `""` | 0 | any | 0 | `Some(0)` |
| `""` | 7 | any | 0 | `None` |
| any | `u32::MAX` | any | line length | `None` |
| line `line_count` | anything | any | end of last line | `Some(end of last line)` |
| line `line_count + 1` | anything | any | that line, column 0 | `None` |

**Round trip**: for every char boundary of
`"héllo wörld\n日本語のテキスト\n👩‍👩‍👧‍👦 family"` and each of the three
encodings, `clamp(locate(p)) == p`.

The mid-character row is the one rust-analyzer's own round-trip test lacks, and
why its `to_utf8` returns an offset *inside* a character. It is the mutation
test for this phase: break the round-down and it must fail by name.

## What NOT to change

- **`widget.rs` and `geometry.rs`.** If this phase seems to need either, stop
  and report.
- **`content.rs`.** The revision counter is Phase 3. `Content::lsp` is an
  `impl Content` block living in `lsp/bridge.rs`.
- **No batch methods yet.** `diagnostics()` and `hints()` are Phase 4. The walk
  they use ships here, unused and marked so.
- **No `From` impls between `lsp::Position` and `crate::Position`**, in either
  direction, ever. The separation is what makes a wire position unusable as an
  index until it has been through this module.
