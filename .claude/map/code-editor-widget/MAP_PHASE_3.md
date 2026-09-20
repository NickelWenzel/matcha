# Phase 3 — `geometry.rs` + decoration types

## Prerequisites

Phase 2 complete: the widget works at parity with `TextEditor`.

## Goal

The pure functions that map logical text positions to screen rectangles and points, plus the
decoration data types they consume. **This is where all the real complexity lives**, and it is all
testable without a renderer or a window. Nothing in this phase touches the widget.

**Exit criteria:** `cargo clippy --all-targets -- -D warnings` clean and the full geometry test
suite below passes.

## Step 0 — Fork API verification (already done — read, don't repeat)

**Completed 2026-09-19.** The `hecrj` fork was cloned at rev `1cdc3e0f` and read directly. Every
API this phase depends on is **byte-identical** to the crates.io 0.19.0 copy the design was
verified against. No fallback port is needed; implement the design as written.

Confirmed present with these exact signatures:

- `Buffer::layout_runs(&self) -> LayoutRunIter<'_>` — `&self`, so it works through `editor.buffer()`
- `LayoutRun::highlight(&self, cursor_start: Cursor, cursor_end: Cursor) -> impl Iterator<Item = (f32, f32)>`
  — returns an **owned** `std::vec::IntoIter`, so nothing borrows `run`. Collecting the spans up
  front is what detaches the closure from `run`; no second `.collect::<Vec<_>>()` is needed
- `LayoutRun::cursor_position(&self, cursor: &Cursor) -> Option<f32>`
- `Buffer::cursor_position(&self, cursor: &Cursor) -> Option<(f32, f32)>` — returns `(x, line_top)`
- `LayoutRun` fields: `line_i`, `text`, `rtl`, `glyphs`, `decorations`, `line_y`, `line_top`,
  `line_height`, `line_w`. **No `layout_i`** — the visual-row index is a private iterator cursor,
  which is why the first-row test must be structural.
- `Scroll { line, vertical, horizontal }`; `Buffer::scroll()` returns **by value** (`const fn`)

Two clarifications from the fork read that affect the code below:

- **`line_top` is measured from the top of `scroll.line`, not from buffer line 0.** The iterator
  starts its accumulator at `0.0` on the scroll line, so output is viewport-relative — the same
  space `Editor::selection()` reports in.
- **The missing line-bounds check fires in both directions.** Outside `[start.line, end.line]`,
  *both* `!=` guards short-circuit to `true`, so lines **above** `start.line` are affected as well
  as those below `end.line`. The filter is a range test for that reason.

## Step 1 — Decoration types

`src/code_editor/decoration.rs` holds only the shared range type:

```rust
/// A half-open range of text, in UTF-8 byte positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    start: Position,
    end: Position,
}

impl TextRange {
    /// Creates a [`TextRange`], ordering the endpoints.
    pub fn new(a: Position, b: Position) -> Self {
        if a <= b { Self { start: a, end: b } } else { Self { start: b, end: a } }
    }

    /// The earlier endpoint.
    pub fn start(self) -> Position { self.start }

    /// The later endpoint.
    pub fn end(self) -> Position { self.end }
}
```

**Normalizing at construction is required, not tidiness.** `LayoutRun::highlight` with
`end < start` does not return empty — it returns garbage. Private fields + a constructor make the
invariant unbypassable. `Position` derives `Ord` upstream (`core/src/text.rs:631`), so `<=` works.

`src/code_editor/decoration/diagnostic.rs`:

```rust
/// How severe a [`Diagnostic`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity { Error, Warning, Information, Hint }

/// A range of text to underline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diagnostic {
    /// The range to underline.
    pub range: TextRange,
    /// How severe it is.
    pub severity: Severity,
}

/// How to draw a diagnostic underline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// The color of the wave.
    pub color: Color,
    /// Stroke thickness, in logical pixels.
    pub thickness: f32,
    /// Peak-to-trough height, in logical pixels.
    pub amplitude: f32,
    /// Horizontal period, in logical pixels.
    pub wavelength: f32,
}
```

`src/code_editor/decoration/inlay.rs`:

```rust
/// A text overlay anchored to a position, drawn without affecting layout.
#[derive(Debug, Clone)]
pub struct Hint<'a> {
    /// Where to anchor the label.
    pub position: Position,
    /// The label text.
    pub label: Cow<'a, str>,
}

/// How to draw an inlay hint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// The label color.
    pub color: Color,
    /// Label size as a fraction of the editor's text size.
    pub size_scale: f32,
    /// Offset from the anchor, in logical pixels.
    pub offset: Vector,
}
```

Module-path naming, not composite names: `diagnostic::Style` and `inlay::Style`, never
`DiagnosticStyle`/`InlayStyle`. Both are imported by their parent module at call sites.

## Step 2 — `geometry.rs`

Coordinates are relative to the **text origin** (widget bounds shrunk by padding), matching
`Editor::selection()` semantics. The widget adds the origin translation at draw time.

```rust
/// A visible fragment of a text range, in text-origin-relative coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fragment {
    /// The fragment's box.
    pub bounds: Rectangle,
    /// The text baseline within that box, used to place underlines.
    pub baseline: f32,
}

/// Returns one [`Fragment`] per visible visual row the range covers.
pub fn range_fragments(
    buffer: &cosmic_text::Buffer,
    hint_factor: f32,
    range: TextRange,
) -> Vec<Fragment> {
    let scroll = buffer.scroll();
    let start = cosmic_text::Cursor::new(range.start().line, range.start().index);
    let end = cosmic_text::Cursor::new(range.end().line, range.end().index);
    let inverse = 1.0 / hint_factor;

    buffer
        .layout_runs()
        // REQUIRED: highlight() has no line-bounds check of its own.
        .filter(|run| run.line_i >= start.line && run.line_i <= end.line)
        .flat_map(|run| {
            let mut spans: Vec<(f32, f32)> = run.highlight(start, end).collect();

            if spans.is_empty() {
                // See "the empty-line / zero-width fallback" below — this is required.
                let anchor = if run.glyphs.is_empty() {
                    Some(0.0)
                } else {
                    run.cursor_position(&start)
                };

                spans.extend(anchor.map(|x| (x, MIN_FRAGMENT_WIDTH * hint_factor)));
            }

            let (top, height, baseline) = (run.line_top, run.line_height, run.line_y);

            spans.into_iter().map(move |(x, width)| Fragment {
                // Vertical scroll is already applied by the iterator; the renderer applies
                // horizontal scroll only at draw time, so overlays subtract it themselves.
                // It is a buffer-space distance, so it goes before the hint scaling.
                bounds: Rectangle { x: x - scroll.horizontal, y: top, width, height } * inverse,
                baseline: baseline * inverse,
            })
        })
        .collect()
}

/// Returns the top-left anchor of a position, if it is currently visible.
pub fn position_anchor(
    buffer: &cosmic_text::Buffer,
    hint_factor: f32,
    position: Position,
) -> Option<Point> {
    let scroll = buffer.scroll();
    let cursor = cosmic_text::Cursor::new(position.line, position.index);

    buffer.cursor_position(&cursor).map(|(x, top)| {
        Point::new((x - scroll.horizontal) / hint_factor, top / hint_factor)
    })
}

/// Returns `(line index, top)` for the first visual row of each visible logical line.
pub fn visible_line_rows(
    buffer: &cosmic_text::Buffer,
    hint_factor: f32,
) -> impl Iterator<Item = (usize, f32)> + '_ {
    buffer.layout_runs().filter_map(move |run| {
        // Structural test, NOT dedupe-on-line_i: when a wrapped line straddles the
        // top of the viewport, LayoutRunIter skips the rows above it and yields
        // row 2 first, which a dedupe would mislabel as the line's first row.
        let is_first_row =
            run.glyphs.is_empty() || run.glyphs.iter().map(|g| g.start).min() == Some(0);

        is_first_row.then(|| (run.line_i, run.line_top / hint_factor))
    })
}
```

### The four constraints encoded above

1. **The `.filter` on `line_i` is load-bearing.** `LayoutRun::highlight`'s predicate is
   `(start.line != line_i || c_end > start.index) && (end.line != line_i || c_start < end.index)`
   (cosmic-text `buffer.rs:85-86`). For a run *outside* the range both disjuncts are vacuously
   true, so **every grapheme reports as selected** and the run returns one full-width span.
   cosmic-text's own renderer guards this identically at `edit/editor.rs:102-104`. Without the
   filter, a single diagnostic paints a squiggle across every visible line.
2. **Subtract `scroll.horizontal`, never `scroll.vertical`.** `LayoutRunIter` already applies
   vertical scroll (`line_top = self.line_top - self.scroll`, `buffer.rs:257`). Horizontal scroll
   is applied by the renderer at draw time (`wgpu/src/text.rs:606`), so overlays must subtract it
   themselves — exactly as `Editor::selection()` does (`graphics/src/text/editor.rs:175`).
3. **Subtract scroll first, then divide by `hint_factor`.** Treat `scroll.horizontal` as
   buffer-space. Follow `graphics/src/text/editor.rs:175`, not `:545` — iced is internally
   inconsistent here.
4. **`glyphs.first()` is the visually-leftmost glyph, not the logically-first.** The first-row test
   needs `.map(|g| g.start).min()`, which is also RTL-correct.

### The empty-line / zero-width fallback (required)

`LayoutRun::highlight` iterates `self.glyphs`, and both `buffer.rs:96-99` and `:107-112` drop
spans with `width <= 0.0`. Two consequences that would otherwise ship as silent bugs:

- A diagnostic on a **blank line** produces no glyphs, therefore no spans, therefore **nothing
  drawn**.
- A **zero-width range** (`start == end`) — exactly what an LSP "insert here" or
  "missing semicolon" diagnostic looks like — is filtered out, therefore **nothing drawn**.

cosmic-text's own renderer special-cases the first at `edit/editor.rs:106-115`. matcha must handle
both. After collecting spans for a run that is inside the range, if it produced no fragment, emit
one of a minimum width:

```rust
const MIN_FRAGMENT_WIDTH: f32 = 4.0;   // logical px — enough for one squiggle period

let anchor = if run.glyphs.is_empty() {
    // A blank line has no glyph to anchor to, and its own cursor answers
    // for no other line, so it can only be marked at its left edge.
    Some(0.0)
} else {
    // Anchor at the column the range starts at, so a zero-width range
    // points at its own token. `None` means the range starts on another
    // line or another visual row of this one — nothing to mark here.
    run.cursor_position(&start)
};

// MIN_FRAGMENT_WIDTH is logical while spans are buffer-space, so scale it
// up before the shared `1.0 / hint_factor` pass converts everything back.
spans.extend(anchor.map(|x| (x, MIN_FRAGMENT_WIDTH * hint_factor)));
```

**The `glyphs.is_empty()` test is the discriminator, and `unwrap_or(0.0)` is wrong.**
`LayoutRun::cursor_position` early-returns `None` whenever `cursor.line != self.line_i`, so
defaulting to `0.0` paints a spurious marker at the left edge of every row where the range merely
*isn't* — including every visual row above a range that starts on the second row of a wrapped
line, and any byte index past the end of its line. `None` must suppress the fragment, not fall back.

Anchoring at the run's left edge unconditionally is equally wrong: it is right only for blank
lines, and would draw a mid-line zero-width range at column 0, pointing at the wrong token.

Do this per *run*, not per range, so a multi-line range with a blank line in the middle still marks
the blank line. Do **not** extend it to the full line width the way the reference renderer does for
selections — underlining trailing whitespace is wrong for diagnostics, and VS Code does not do it.

### Which `hint_factor`

Callers pass `editor.hint_factor().unwrap_or(1.0)` — the **editor's**, not the renderer's. The
buffer's coordinate space is scaled by `internal.hint_factor`
(`graphics/src/text/editor.rs:729`), which is derived through `text::hint_factor(size, factor)`
(`:691`) and can be `None` even when the renderer reports a scale. It is currently always `None`
(`graphics/src/text.rs:404-418`), so every division is a no-op today — write them anyway.

## Step 3 — Tests

Build a real editor headlessly. `font_system()` is a public global
(`graphics/src/text.rs:119`), so shaping works with no window and no renderer:

```rust
fn shaped(text: &str, width: f32) -> graphics::text::Editor {
    let mut editor = graphics::text::Editor::with_text(text);

    editor.update(
        Size::new(width, 400.0),
        Font::DEFAULT,   // NOT Font::MONOSPACE — see below
        Pixels(14.0),
        LineHeight::Absolute(Pixels(20.0)),
        Wrapping::Word,
        text::Alignment::Default,
        None,                                   // hint_factor
        &mut text::parser::PlainText,
    );

    editor
}
```

Determinism depends on the `fira-sans` dev-feature added in Phase 1 **and on `Font::DEFAULT`**.
`FontSystem::new_with_fonts` also calls `db.load_system_fonts()`, and `fira-sans` only sets the
*sans-serif* family — so `Font::MONOSPACE` resolves through whatever the host happens to have
installed and shaping stops being reproducible. `Font::DEFAULT` is the bundled Fira Sans. Assert on *relationships*
(ordering, counts, containment, equality between two computed values) rather than hard-coded pixel
values wherever possible — exact advances are font-version-dependent.

### Test matrix

| Case | Expectation |
| --- | --- |
| Range inside one line, several lines visible | Exactly 1 fragment, on that line only — **the B1 regression** |
| Range spanning 3 lines | Fragments on exactly those 3 lines |
| Reversed range (`end < start`) | Same result as the forward range (normalization) |
| Wrapped line, range crossing the wrap | ≥2 fragments, distinct `y` values |
| Diagnostic on a **blank line** | Exactly 1 fragment, `width == MIN_FRAGMENT_WIDTH`, `x == 0.0` |
| **Zero-width range mid-line** (`start == end`) | 1 minimum-width fragment at the *column it points at*, not at `x == 0.0` |
| Blank line in the middle of a multi-line range | Gets its own minimum-width fragment |
| Multibyte (`héllo`, CJK, emoji/ZWJ) | Fragment widths are positive; no panic; byte indices at cluster boundaries |
| Range at line start / line end | Non-empty fragment; width > 0 |
| `position_anchor` at a wrap boundary | `Some(..)`, landing at the **end of the previous visual row** (`cursor_glyph` ignores affinity) |
| Horizontal scroll > 0 | Fragment `x` shifts left by exactly the scroll amount |
| Vertical scroll | Fragment `y` tracks; lines above the viewport produce nothing |
| Wrapped line straddling the viewport top | `visible_line_rows` does **not** report its continuation row as the first row — **the M4 regression** |
| Line index past `line_count()` | Empty `Vec` / `None` |
| Byte index past line end | Empty / `None`, never a panic |
| Index off a char boundary | Empty / `None`, never a panic |
| Anchor scrolled out of view | `None` |

Name tests as sentences: `a_range_on_one_line_yields_fragments_only_on_that_line`,
`a_wrapped_line_straddling_the_viewport_top_is_not_mislabelled`.

## Files

| File | Change |
| --- | --- |
| `src/code_editor.rs` | declare `pub mod decoration;` and `pub(crate) mod geometry;` |
| `src/code_editor/decoration.rs` | new — `TextRange`, declares `diagnostic` + `inlay` |
| `src/code_editor/decoration/diagnostic.rs` | new — `Diagnostic`, `Severity`, `Style` |
| `src/code_editor/decoration/inlay.rs` | new — `Hint`, `Style` |
| `src/code_editor/geometry.rs` | new — `Fragment` + the three functions + tests |
| `src/lib.rs` | re-export `decoration` and `geometry` |

`decoration` is `pub` because the builders take `&[diagnostic::Diagnostic]` and `&[inlay::Hint]`,
so callers must be able to name those types.

`geometry` is **`pub(crate)`**. It was briefly `pub` during this phase for one reason only — with
no callers until Phases 4-6, `-D warnings` trips `dead_code` on a caller-less private module — and
that was a scaffolding decision, not an API one. Every function takes a `cosmic_text::Buffer`, and
`Content` keeps its editor `pub(super)`, so no external caller can obtain one: leaving it `pub`
ships a module nobody outside the crate can call. Once Phases 4-6 wire all three functions into
`widget.rs`, the `dead_code` pressure is gone and it narrows. If you hit the warning mid-phase,
reach for `#[allow(dead_code)]` with a note rather than widening the API to silence a lint.

## Verification

```bash
cargo clippy --all-targets -- -D warnings
cargo test
```

## Do NOT change in this phase

- Do not touch `widget.rs`. Wiring happens in Phases 4-6.
- Do not call these functions from `layout`. `layout_runs()` terminates at the first unshaped line
  (`LayoutRunIter::next` does `line.shape_opt()?`), and shaping for the visible window only
  completes during `draw`, after `editor.highlight(..)`. See MAP_PLAN Risk B.
- Do not add caching, interval trees, or `BTreeMap` indexing. `layout_runs()` already yields only
  visible rows and the `line_i` filter bounds the work further.
- Do not reimplement `Buffer::cursor_position` by hand — it already is `position_anchor` minus the
  scroll/hint arithmetic, and hand-rolling it doubles fork-drift exposure.
- Do not add `cosmic-text` as a direct dependency; reach it via
  `iced::advanced::graphics::text::cosmic_text`.
- Do not panic on bad input. Every invalid position returns empty or `None`.
