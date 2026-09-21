# Phase 6 — Applying edits: caret and selection

Self-contained arithmetic over the list Phase 5 already builds. Small, and the
only phase whose whole content is a formula — so it was **written from a working
implementation** rather than described, and the code below is transcribed from a
probe that agrees with an independent oracle on all thirteen cases.

## Prerequisites

Phase 5: `Content::apply` applies edits and leaves the caret wherever
`Edit::Paste` put it.

## Why this is not optional

`Edit::Paste` sets the cursor to the end of the inserted text, and
`Action::Edit` ends in `shape_until_cursor`, which scrolls there. So after a
whole-document format — one edit spanning the buffer, the commonest batch there
is — the caret sits at EOF, the viewport is at the bottom, and the user's
selection is gone.

This is **not** the whole-document diff listed under *Out of scope*. That
preserves anchors across an arbitrary rewrite. This restores one caret and one
anchor, from a list that has already been validated, sorted and indexed.

## Three definitions that change the answer

Each was wrong or missing in the first draft, and each is invisible in the very
case the phase exists for.

- **Tie-break: a caret exactly at an insertion point ends up *after* the
  inserted text.** VS Code's behaviour, and what "accept this inlay hint" should
  feel like. A convention, not arithmetic — hence named rather than derived.
- **"Line count" means `split('\n').count()`, not `.lines().count()`.** `"x\n"`
  is **two** lines. A formatter that adds a trailing newline is the common case,
  and `.lines()` mis-shifts every caret below the edit.
- **The line delta is signed.** A reformat that shrinks the file makes it
  negative; in `usize` that panics in debug and wraps in release.

## The implementation

Read `cursor()` **once, before the first `move_to`** — `apply`'s own `move_to`
destroys it — and adjust per edit in the same reverse order. That composes
because the edits are disjoint and each adjustment uses only its own edit's
geometry.

```rust
/// Where `caret` ends up after `replacement` is applied over `start..end`.
///
/// `start`, `end` and `caret` are editor positions (line, byte index) — this
/// runs after conversion, never on wire coordinates.
fn adjust(
    caret: Position,
    start: Position,
    end: Position,
    new_text: &str,
) -> Position {
    // split('\n'), NOT lines(): "x\n" is two lines, and a formatter adding a
    // trailing newline is the common case.
    let new_lines: Vec<&str> = new_text.split('\n').collect();
    let n = new_lines.len();
    let last = new_lines[n - 1];

    // Where the replacement ends, in the new text's coordinates.
    let end_line = start.line + n - 1;
    let end_col = if n == 1 { start.index + last.len() } else { last.len() };

    // 1. Strictly before: untouched.
    if caret.line < start.line || (caret.line == start.line && caret.index < start.index) {
        return caret;
    }

    // `end` is EXCLUSIVE. An inclusive test here sends an insert-at-caret down
    // branch 2 and lands the caret BEFORE the inserted text, contradicting the
    // tie-break above.
    let after_end = caret.line > end.line
        || (caret.line == end.line && caret.index >= end.index);

    if after_end {
        // 3. Shift. Signed: a shrinking reformat makes this negative.
        let delta = (n as isize - 1) - (end.line as isize - start.line as isize);
        let line = (caret.line as isize + delta) as usize;
        let index = if caret.line == end.line {
            end_col + (caret.index - end.index)
        } else {
            caret.index
        };
        return floor(Position { line, index }, new_lines.get(line.wrapping_sub(start.line)));
    }

    // 2. Inside [start, end): keep the offset into the replaced region.
    let rel = caret.line - start.line;
    if rel >= n {
        // That line is gone. Fall back to the replacement's end -- fall back OR
        // clamp, never both: an earlier draft said both and they disagree.
        return Position { line: end_line, index: end_col };
    }
    let line_text = new_lines[rel];
    let index = if rel == 0 {
        // The `start.index` base is load-bearing and the first draft dropped
        // it. Invisible in a whole-file format, where start.index is 0; wrong
        // for every single-line edit, which is every quickfix.
        start.index + (caret.index - start.index).min(line_text.len())
    } else {
        caret.index.min(line_text.len())
    };
    floor(Position { line: start.line + rel, index }, Some(&line_text))
}
```

**Then round the column down to a char boundary of the line it landed on.** The
column is an *old* byte column clamped onto a *new* line, so nothing above
guarantees a boundary; `move_to` stores whatever it is given
(`iced/graphics/src/text/editor.rs:606-627`), and the next keystroke reaches
`String::split_off` and panics. Same rule `Bridge::clamp` applies, same reason.

The selection anchor takes the whole function independently. Drop the selection
only if it collapses onto the caret.

## Verification

The rule was checked against an oracle that works in **absolute byte offsets**,
where the arithmetic is obviously correct, and converts back to line and column.
Rebuild that oracle in the tests — it is what makes this phase trustworthy:

```rust
// Oracle: absolute offsets. Honours the same tie-break (`c < a`, so a caret at
// an insertion point ends up after) and the same boundary rounding.
fn oracle(text: &str, caret: Position, start: Position, end: Position, new_text: &str) -> Position {
    let (a, b, c) = (abs(text, start), abs(text, end), abs(text, caret));
    let moved = if c < a { c }
        else if c >= b { c + new_text.len() - (b - a) }
        else { a + (c - a).min(new_text.len()) };
    let mut out = text.to_owned();
    out.replace_range(a..b, new_text);
    let mut n = moved.min(out.len());
    while n > 0 && !out.is_char_boundary(n) { n -= 1; }
    pos(&out, n)
}
```

## Spot checks

All thirteen agreed between the rule and the oracle during planning. The bolded
ones are the mutation tests — break the named thing and that row must fail.

| case | catches |
|---|---|
| replace, caret after it | the shift branch |
| **insert exactly at the caret** | **an inclusive `end` boundary** |
| insert mid-line at the caret | the tie-break |
| **caret exactly at `end`** | **the same boundary, from the other side** |
| **caret inside, non-zero `start.index`** | **the dropped `start.index` base** |
| multi-line → multi-line | the line delta |
| **multi-line → single line (shrink)** | **an unsigned delta** |
| caret strictly before | branch 1 |
| **trailing newline in `new_text`** | **`.lines()` instead of `split('\n')`** |
| whole file, fewer lines | delta and fallback together |
| whole file, more lines | delta |
| **caret lands mid-character on the new line** | **the missing boundary round** |
| multibyte with valid boundaries | the boundary round does not over-trigger |

Plus: a selection whose anchor and head fall on opposite sides of one edit; and
a whole-document reformat that preserves line structure leaves the caret on the
same logical line and **never at EOF**.

## Known consequence, documented not fixed

`move_to` does not scroll and `Content` exposes no scroll accessor, so after
restoration **the viewport stays at the topmost edit while the caret is back
where the user left it**. Defensible — you want to see what changed — but
surprising if unstated, so state it in `apply`'s doc.

## What NOT to change

- **`widget.rs`, `geometry.rs`.**
- **Phase 5's validation, sort or commit order.** This phase adds one read
  before the loop and one `move_to` after it.
- **No diffing.** Out of scope, and a different problem.
- **Do not read `cursor()` inside the loop.** `apply`'s own `move_to` has
  already destroyed it by then.
