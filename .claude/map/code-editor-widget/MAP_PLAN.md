# matcha — a decorated code editor widget for iced

## Context

`matcha` is an empty repo (LICENSE + .gitignore only, no `Cargo.toml`). The goal is a
standalone `CodeEditor` widget for iced that does what `iced::widget::TextEditor` does —
editing, selection, wrapping, scrolling, IME, syntax highlighting — **plus** three things the
stock widget cannot do: diagnostic squiggles, inlay-hint overlays, and a line-number gutter.

The widget cannot be built by wrapping `TextEditor`. `text_editor::Content` is
`Content<R>(RefCell<Internal<R>>)` with a **private** tuple field and a **private** `Internal`
type ([widget/src/text_editor.rs:622-631](/home/nickel/Programming/repos/iced/widget/src/text_editor.rs#L622-L631)),
so a downstream crate holding a `Content` can never reach the `R::Editor` inside it — and that
editor is the only source of shaped text geometry. Hence: own the editor directly.

`initial_plan.md` is directionally right and its core architectural call is correct. But it was
written against iced `9854bd32`, and the local checkout fast-forwarded ~250 commits to
**`fa3bae52`** *during this planning session*. Three of its prescriptions are now wrong or
unnecessary. This plan supersedes it.

---

## Verified ground truth

Verified by reading `/home/nickel/Programming/repos/iced` at
`fa3bae52874274c012a27d4bf11a83c49e1709ae` — not from memory.

| Claim | Status |
| --- | --- |
| `graphics::text::Editor::buffer() -> &cosmic_text::Buffer` is public | ✅ [graphics/src/text/editor.rs:38](/home/nickel/Programming/repos/iced/graphics/src/text/editor.rs#L38) |
| `core::text::editor::State` is public with `new`/`update`/`draw`/`input_method`/`is_focused`/`is_cursor_visible` + `Focusable` | ✅ [core/src/text/editor.rs:315-769](/home/nickel/Programming/repos/iced/core/src/text/editor.rs#L315-L769) |
| `Highlighter` split into `Parser` + `Highlighter` | ✅ `core/src/text/parser.rs:9`, `core/src/text/highlighter.rs:7` |
| `Renderer: text::Renderer<Editor = graphics::text::Editor>` is a valid bound | ✅ set by wgpu, tiny-skia, and the fallback renderer |
| `cosmic_text` re-exported for downstream use | ✅ `pub use cosmic_text;` [graphics/src/text.rs:10](/home/nickel/Programming/repos/iced/graphics/src/text.rs#L10) |
| `text_editor::{Catalog, Status, Style}` public and reusable | ✅ `widget/src/text_editor.rs:746-802`; the `theme::Base` supertrait is what supplies `theme.name()` for cache invalidation |
| `font_system()` is a public global with bundled fonts | ✅ [graphics/src/text.rs:119](/home/nickel/Programming/repos/iced/graphics/src/text.rs#L119) — **geometry is unit-testable headlessly** |
| Only private thing `TextEditor` touches is `self.content.0` | ✅ 5 call sites; everything else is public |

Environment: rustc 1.98.0 (iced needs ≥1.93, edition 2024). cosmic-text is the **`hecrj` fork**,
0.19.0, rev `1cdc3e0f` — pinned in `Cargo.toml:202`.

### Three corrections to `initial_plan.md`

**1. Do not reimplement `highlight_line` / `visual_lines_offset`.** The initial plan's steps 6-8
are its largest and riskiest chunk, and they are unnecessary. `buffer.layout_runs()` already
yields exactly the **visible** visual rows with `line_i`, `line_top` (vertically scroll-adjusted),
`line_y` (baseline), `line_height`, and `glyphs` — and cosmic-text provides both primitives we
need as library calls: `LayoutRun::highlight(start, end)` (grapheme-accurate `(x, width)` spans,
yielding *multiple disjoint spans* for mixed BiDi runs) and `Buffer::cursor_position(&Cursor)`.
iced's own `highlight_line` sums `glyph.w` assuming contiguous LTR glyphs and breaks under BiDi,
so porting it would mean copying a known-weaker algorithm.

**2. Drawing order is decided by primitive type, not call order.** The initial plan's step 12
("take control of drawing order", reproduce `State::draw` locally) buys nothing. Within one
layer, both backends render **quads → meshes → images → text**
([wgpu/src/lib.rs:459-618](/home/nickel/Programming/repos/iced/wgpu/src/lib.rs#L459-L618),
`tiny_skia/src/lib.rs:106-165`), regardless of `fill_quad` call sequence. So `fill_quad`
squiggles always land *beneath* glyphs — which is what you want for underlines. **Just call
`State::draw`, then draw decorations.** Only a real `start_layer`/`end_layer` push can put a quad
above text; the one case needing it is an opaque background chip behind an inlay hint (deferred).

**3. `hint_factor` is currently always `None`.** `graphics/src/text.rs:404-418` returns `None`
unconditionally (`// TODO: Fix hinting in cosmic-text`), so the factor is `1.0` everywhere today.
Write every division anyway so the widget stays correct when hinting returns — but know the
non-1.0 path cannot be tested right now, which makes §"Two hint factors" below a silent trap.

---

## Architecture

Single library crate + cargo examples. Pinned git dependency on iced. Conventions follow the
`/iced` skill and `~/.claude/guides/RUST_STYLE.md`.

```text
matcha/
  Cargo.toml                    iced pinned to rev fa3bae52
  src/lib.rs                    #![warn(missing_docs)]; pub use code_editor::{code_editor, ...}
  src/code_editor.rs            module root (no mod.rs anywhere)
  src/code_editor/
    content.rs                  Content — owns the graphics editor
    widget.rs                   CodeEditor + the code_editor() helper fn
    geometry.rs                 pure buffer → screen-coordinate functions (the testable core)
    gutter.rs                   line-number measurement + drawing
    decoration.rs               TextRange
    decoration/
      diagnostic.rs             Diagnostic, Severity, Style
      inlay.rs                  Hint, Style
  examples/
    showcase.rs                 static decorations, wrapping, Unicode
    live.rs                     decorations invalidated on edit (simulates LSP round-trips)
```

**No `core/` crate.** The skill's workspace split exists to keep iced out of testable logic, but
matcha's logic *is* text geometry over `cosmic_text::Buffer` — there is no iced-free layer to
extract. A single library crate is the right shape.

**Naming follows the module path, not composite names.** `decoration::diagnostic::Style` and
`decoration::inlay::Style` rather than `DiagnosticStyle`/`InlayStyle`; `diagnostic::Severity`
rather than `DiagnosticSeverity`. The one deliberate exception is `code_editor::CodeEditor`,
which stutters — it is kept because it mirrors iced's own `text_editor::TextEditor`, and a widget
library that diverges from iced's naming is harder to use, not cleaner.

**Two `Style` types will collide** in `widget.rs`: `iced::advanced::text::editor::Style` (what
`State::draw` takes) and `iced::widget::text_editor::Style` (the themed one). `use foo as bar` is
banned — import the parent modules and disambiguate by path: `editor::Style` vs
`text_editor::Style`.

**The primary constructor is the function helper**, matching iced: `code_editor(&content)`, with
`CodeEditor::new` as the type-level form callers rarely touch.

Remaining conventions, applied throughout: no `unwrap()` in library code — `expect("invariant:
…")` or `let … else`; import the parent module when using more than one item from it; within each
file, order as doc comment → imports → types → trait impls → inherent impls → private helpers →
tests; comments explain *why* in the present tense, with no plan bookkeeping or archaeology.

**Manifest requirements — decide these in Phase 1, not Phase 7:**

```toml
[dependencies]
iced = { git = "https://github.com/iced-rs/iced",
         rev = "fa3bae52874274c012a27d4bf11a83c49e1709ae",
         features = ["advanced", "highlighter"] }   # NOT default-features = false

[dev-dependencies]
iced = { git = "...", rev = "...", features = ["advanced", "highlighter", "fira-sans"] }
iced_test = { git = "...", rev = "..." }   # SAME rev as iced
```

`iced_test` must come from the **same git rev** as `iced`, never crates.io — a crates.io
`iced_test` pulls its own `iced_core`, and the two `Element`/`Renderer` types would not unify. The
`/iced` skill's `[patch.crates-io]` + `branch = "master"` form is the alternative convention; the
exact-rev pin is chosen here deliberately (Risk D). Revisit only if matcha gains a dependency that
itself depends on iced, since unifying those needs `[patch]`.

A renderer backend (`wgpu` or `tiny-skia`, both on by default) is a **hard requirement**, not a
nicety: with no backend, `iced_renderer::Renderer = ()` (`renderer/src/lib.rs:56`) and
`<() as text::Renderer>::Editor = ()` (`core/src/renderer/null.rs:44`), so the widget's
`Renderer::Editor = graphics::text::Editor` bound fails to typecheck entirely. `fira-sans` is
**not** a default feature (`Cargo.toml:25` vs `:73`) and is required from Phase 3 for
deterministic geometry tests — iced's own test crate sets it (`test/Cargo.toml:26`).

### Key design decisions

**`Content` is concrete, not generic.** Because we pin `Renderer::Editor = graphics::text::Editor`,
iced's `R` generic is dead weight:

```rust
pub struct Content(RefCell<graphics::text::Editor>);
```

Mirrors the useful half of `text_editor::Content`: `new`, `with_text`, `perform`, `move_to`,
`cursor`, `line`, `lines`, `line_count`, `line_ending`, `text`, `selection`, `is_empty` — each a
one-line delegation to the public `text::Editor` trait
([core/src/text/editor.rs:22-120](/home/nickel/Programming/repos/iced/core/src/text/editor.rs#L22-L120)).
Note the borrow idiom shifts: iced's `&self.content.0.borrow().editor` becomes
`&*self.content.0.borrow()`.

**Reuse, do not reimplement, all input handling.** `editor::State::update` is a complete
renderer-agnostic input state machine — Single/Double/Triple click detection, drag beyond bounds,
fractional scroll accumulation, touch, IME preedit, cursor-blink scheduling, and the full default
key map via `Binding::from_key_press`. Driving it is one call.

**Reuse iced's theming.** `Theme: text_editor::Catalog`, reusing `text_editor::{Status, Style}` so
a `CodeEditor` looks identical to a `TextEditor` out of the box. Decoration styling stays off the
theme system as builder methods (`.diagnostic_style(..)`, `.inlay_style(..)`, `.gutter(..)`).

**The gutter is modelled as extra left padding.** This is the trick that makes the gutter nearly
free. Every use of `padding` in `State::update` is `cursor_pos - Vector::new(padding.left,
padding.top)` (core/src/text/editor.rs:391 press, :422 drag, :457 touch press, :488 touch move);
scroll and `input_method` never touch it. So folding the gutter into `padding.left` makes
click-to-position and drag-select correct with zero extra code:

```rust
fn text_padding(&self, gutter_width: f32) -> Padding {
    Padding { left: self.padding.left + gutter_width, ..self.padding }
}
```

**`text_padding` must replace `self.padding` at all five call sites**, not three — mixing them is
a silent layout bug. In `layout`: both `limits.shrink(..)` **and** `bounds.expand(..)`
(widget/src/text_editor.rs:367, :383) — shrinking by `text_padding` while expanding by
`self.padding` leaves the widget node `gutter_width` narrower than its container, and text escapes
the clip rect. In `update`: the `padding` argument to `State::update` (:443) and
`layout.bounds().shrink(..)` for `request_input_method` (:472). In `draw`: `bounds.shrink(..)`
(:532).

`gutter_width` is needed in `layout`, `update`, `draw`, and `mouse_interaction`, and a mid-update
relayout can change it — so compute it in one pure `fn gutter_width(&self, state, renderer) -> f32`
called from all four, with the `paragraph::Plain` measurement cached in a `RefCell` on tree state
(`draw` only gets `&Tree`). `Plain::update(Text<&str>) -> bool` / `min_bounds()` are public at
`core/src/text/paragraph.rs:83-149`. Drawing uses `align_x: Alignment::Right` for free
right-alignment (wgpu/src/text.rs:562-565).

**Two hint factors, and they are different values.** `Renderer::hint_factor()`
(core/src/renderer.rs:78) is the scale hint. `Editor::hint_factor()` is
`internal.hint.then_some(internal.hint_factor)` (graphics/src/text/editor.rs:641), derived through
`text::hint_factor(new_size, new_hint_factor)` (:691) — which can return `None` even when the
renderer reports a scale. The buffer's coordinate space is scaled by the **editor's** factor
(`buffer.set_size(w * internal.hint_factor, ..)`, :729). Therefore:

- **all geometry** divides by `editor.hint_factor().unwrap_or(1.0)`;
- **`fill_text` for inlay hints** passes `renderer.hint_factor()` (matching the placeholder at
  widget/src/text_editor.rs:549).

Treat `scroll.horizontal` as buffer-space, subtracting it before dividing — following
graphics/src/text/editor.rs:175, not :545 (iced is internally inconsistent here).

### The geometry core

`geometry.rs` holds all the testable complexity. Coordinates are relative to the **text origin**,
matching `Editor::selection()` semantics.

```rust
/// A visible fragment of a text range, in text-origin-relative coordinates.
pub struct Fragment {
    /// The fragment's box.
    pub bounds: Rectangle,
    /// The text baseline within that box, used to place underlines.
    pub baseline: f32,
}

pub fn range_fragments(
    buffer: &cosmic_text::Buffer,
    hint_factor: f32,
    range: TextRange,
) -> Vec<Fragment> {
    let scroll = buffer.scroll();
    let (start, end) = (range.start(), range.end());  // TextRange normalizes at construction
    let start = cosmic_text::Cursor::new(start.line, start.index);
    let end = cosmic_text::Cursor::new(end.line, end.index);

    buffer
        .layout_runs()
        // REQUIRED: highlight() has no line-bounds check of its own.
        .filter(|run| run.line_i >= start.line && run.line_i <= end.line)
        .flat_map(|run| {
            let (top, height, baseline) = (run.line_top, run.line_height, run.line_y);

            run.highlight(start, end)
                .map(move |(x, width)| Fragment {
                    bounds: Rectangle { x: x - scroll.horizontal, y: top, width, height },
                    baseline,
                })
                .collect::<Vec<_>>()
            // ...plus a minimum-width fallback when this run is in range but
            // produced no span — see below.
        })
        .map(|fragment| /* scale bounds and baseline by 1.0 / hint_factor */)
        .collect()
}

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

/// First visual row of each visible logical line — the gutter's row list.
pub fn visible_line_rows(
    buffer: &cosmic_text::Buffer,
    hint_factor: f32,
) -> impl Iterator<Item = (usize, f32)> + '_ {
    buffer.layout_runs().filter_map(move |run| {
        // Structural test, NOT dedupe-on-line_i: when a wrapped line straddles the top
        // of the viewport, LayoutRunIter skips the rows above it and yields row 2 first.
        let is_first_row =
            run.glyphs.is_empty() || run.glyphs.iter().map(|g| g.start).min() == Some(0);
        is_first_row.then(|| (run.line_i, run.line_top / hint_factor))
    })
}
```

Four non-obvious constraints baked into the above, each verified:

- **The `.filter` on `line_i` is load-bearing.** `LayoutRun::highlight`'s predicate is
  `(start.line != line_i || c_end > start.index) && (end.line != line_i || c_start < end.index)`
  (cosmic-text `buffer.rs:85-86`). For a run outside the range both disjuncts are vacuously true,
  so **every grapheme reports as selected** and the run returns one full-width span. cosmic-text's
  own renderer guards this at `edit/editor.rs:102-104`. Without the filter, one diagnostic paints
  a squiggle across every visible line.
- **`Buffer::cursor_position` (buffer.rs:1247) already is `position_anchor`** minus the
  scroll/hint arithmetic, and already filters by `line_i`. Reimplementing it only duplicates
  fork-drift exposure. It ignores affinity, so an anchor exactly at a wrap boundary lands at the
  end of the previous visual row — assert that in a test rather than discovering it.
- **`glyphs.first()` is the visually-leftmost glyph, not the logically-first**, so the gutter's
  first-row test needs `.map(|g| g.start).min()`, which is also RTL-correct.

- **A minimum-width fallback is required.** `highlight` iterates `run.glyphs`, and spans with
  `width <= 0.0` are dropped (`buffer.rs:96-99`, `:107-112`). So a diagnostic on a **blank line**
  and a **zero-width range** (an LSP "insert here") both render as *nothing*. When a run is inside
  the range but produced no span, emit one of a small fixed width at the run's start x. Do it per
  run, so a blank line inside a multi-line range is still marked. Do **not** extend to full line
  width the way the reference renderer does for selections — underlining trailing whitespace is
  wrong for diagnostics.

A wrapped multi-line range naturally produces one rectangle per visible wrapped fragment, because
`layout_runs()` iterates visual rows and `highlight` clips per row. An *internal empty line* inside
a multi-line range yields no spans of its own (no glyphs), so the minimum-width fallback above is
what marks it — cosmic-text special-cases the same case at `edit/editor.rs:106-115`.

---

## Phases

Each phase compiles and passes `cargo clippy --all-targets -- -D warnings` on its own.

**Phase 1 — Crate skeleton + `Content`.** `Cargo.toml` exactly as specified above (pinned rev,
`fira-sans` in dev-deps, no `default-features = false`), `src/lib.rs`, `src/code_editor.rs`,
`content.rs`.
*Exit:* `Content::with_text("héllo\nworld").text()` round-trips; `line_count() == 2`.

**Phase 2 — `CodeEditor` at parity with `TextEditor`.** `widget.rs`: the struct, all builders
(`on_action`, `font`, `size`, `line_height`, `padding`, `wrapping`, `width`, `height`, `id`,
`placeholder`, `key_binding`, `style`, `highlight_with`), and the `Widget` impl. Mirror
[widget/src/text_editor.rs:323-607](/home/nickel/Programming/repos/iced/widget/src/text_editor.rs#L323-L607),
substituting our `Content`. Tree state is the same four fields: `editor: editor::State`,
`parser: RefCell<P>`, `parser_settings: P::Settings`, `last_theme: RefCell<Option<String>>` —
including the theme-change check that calls `parser.change_line(0)` (:501-509). `operate` now
takes `&mut self` and a `_viewport: &Rectangle`. Decide deliberately whether to mirror iced's
`last_status` quirk: it lives on the widget (:115), is written only on `RedrawRequested`
(:466-468), and `update` returns early when `on_edit` is `None` (:396-398) — so a disabled editor
renders as `Status::Active`.
*Exit:* `examples/showcase.rs` edits, selects, scrolls, wraps, and highlights Rust source
indistinguishably from `text_editor`.

**Phase 3 — `geometry.rs` + `decoration.rs`.** The three functions above, plus `TextRange`
(normalizing at construction), `Diagnostic`, `Severity`, `InlayHint`, `DiagnosticStyle`,
`InlayStyle`. Pure functions, no widget involvement; invalid/stale positions return empty/`None`,
never panic. Re-export `pub mod geometry` from `lib.rs` so `-D warnings` doesn't trip `dead_code`
on a caller-less module. **First task: verify `LayoutRun::highlight`, `LayoutRun::cursor_position`,
and `Buffer::cursor_position` exist in the hecrj fork** (see Risk A).
*Exit:* the geometry test suite below passes.

**Phase 4 — Line-number gutter.** `gutter.rs`. Measure the widest number via the cached
`paragraph::Plain`, thread `gutter_width()`/`text_padding()` through all the call sites named
above, draw numbers right-aligned at each `visible_line_rows()` row. Wrapped continuation rows get
no number. Clamp gutter clicks: `Action::Click` computes `x = ((position.x + scroll.horizontal) *
hint) as i32` (graphics/src/text/editor.rs:545), so a gutter click is harmless at
`scroll.horizontal == 0` (cosmic-text clamps negatives to line start) but jumps to an arbitrary
column once `scroll.horizontal > gutter_width`. Clamp to the text origin or swallow the event, and
return something other than `Interaction::Text` over the gutter.
*Exit:* numbers align through scrolling **and** through a wrapped line straddling the viewport
top; clicking still places the caret correctly (the regression that proves the padding trick).

**Phase 5 — Diagnostic squiggles.** `.diagnostics(&[Diagnostic])` + `.diagnostic_style(..)`. For
each `range_fragments` rectangle emit a wave of short `fill_quad` segments, each intersected
against the clip bounds. **`snap: false` is required** — `Quad::default().snap` is `CRISP`, `crisp`
is a default feature, and snapping every segment destroys the wave. Place the wave relative to
`run.line_y` (the baseline) rather than guessing from the line box, which drifts at large line
heights — so `range_fragments` should return the baseline alongside each rect.
*Exit:* squiggles track ranges through wrapping, scrolling, and multibyte text — and a diagnostic
on one line paints on that line only.

**Phase 6 — Inlay hints.** `.inlay_hints(&[InlayHint])` + `.inlay_style(..)`. Anchor via
`position_anchor`, render with `fill_text` at `text_origin + anchor + style.offset`. `Text` has
**eleven** required fields (core/src/text.rs:23-62) — notably `shaping: Shaping::Advanced` for
non-ASCII labels, `wrapping: Wrapping::None`, `ellipsis: Ellipsis::None`, and `hint_factor:
renderer.hint_factor()`. Defaults `size = text_size * 0.75`, `offset = Vector::new(2.0, -size *
0.25)`. Anchored placement; overlap with source text is permitted and documented. Anchors scrolled
out of view yield `None` and simply aren't drawn.
*Exit:* hints track their anchors through scroll/wrap and provably do not move source text.

**Phase 7 — Examples, tests, docs.** `examples/live.rs`, `Simulator` behaviour tests, `#[ignore]`d
pixel snapshots, crate docs (`#![warn(missing_docs)]` means every public item needs one), README.
The examples are **single-screen apps and stay at Level 0** — plain `State`/`Message`/`update`/
`view`, no `Action<I, M>` or `Instruction` machinery. That pattern earns its place at the third
screen; cargo-culting it into a 60-line demo only obscures the widget being demonstrated. Use
iced's function helpers throughout (`column![]`, `button(text(..))`, `color!`,
`keyboard::listen().filter_map(..)`), never `Widget::new`.

Dependency graph: `1 → 2 → 3 → 4 → {5, 6} → 7`. Phases 5 and 6 are independent of each other; the
gutter lands first so decorations are built against final text-origin math.

---

## Testing strategy

**Tier 1 — geometry (the bulk).** No renderer, no window. Build a real
`graphics::text::Editor::with_text(..)`, call `update(..)` with fixed bounds/font/size and
`parser::PlainText`, then assert on the geometry functions. Deterministic via the `fira-sans`
feature. Cases: ASCII; multibyte UTF-8 (`héllo`, CJK, emoji/ZWJ clusters); range inside one line;
range spanning lines; **range on one line while other lines are visible** (the B1 regression);
reversed range (`end < start`); wrapped line producing multiple fragments; internal empty line in a
multi-line range; range at line start and line end; anchor exactly at a wrap boundary; horizontal
scroll; vertical scroll; wrapped line straddling the viewport top (the gutter regression);
out-of-range line; byte index past line end; index off a char boundary.

**Tier 2 — widget behaviour.** `iced_test::Simulator` with a custom `FnMut(Candidate) -> Option<T>`
selector (`selector/src/lib.rs:130`). Assert the **passivity invariant** directly: for the same
input sequence, `content.text()`, `content.cursor()`, the layout `Node::bounds()`, and the
click→caret mapping are byte-identical with and without decorations attached. This is the invariant
the whole design rests on, so it gets a test rather than a comment.

Test names read as sentences with no `test_` prefix — `hints_do_not_shift_source_text`,
`a_diagnostic_on_one_line_squiggles_only_that_line`. `.click()` returns a `Result` that must be
consumed; bind it with `let _ =` when the hit target is not needed.

**Tier 3 — pixel snapshots.** `ui.snapshot(&theme)?.matches_hash("snapshots/squiggles")?`, marked
`#[ignore]` like iced's own (platform-sensitive), run via `cargo test -- --ignored`.

---

## Risks

**A. cosmic-text fork drift — RETIRED 2026-09-19.** The `hecrj` fork was shallow-cloned at rev
`1cdc3e0f` and read directly. `LayoutRun`, `LayoutRunIter` (struct, `new`, `from_lines`, `next`),
`highlight`, `cursor_glyph`, both `cursor_position`s, `Scroll`, `Buffer::scroll`,
`Buffer::layout_runs`, and the whole of `edit/editor.rs` are **byte-identical** to the crates.io
0.19.0 copy the design was verified against. **No fallback port is needed**; the Phase 3 design
transfers unchanged.

Three details the fork read confirmed or sharpened:

- **The missing line-bounds check is worse than "absent".** When `line_i` falls outside
  `[start.line, end.line]`, *both* `!=` guards in the predicate short-circuit to `true`, so every
  grapheme reports selected — and this fires for lines **above** `start.line` as well as below
  `end.line`. The `.filter(|run| run.line_i >= start.line && run.line_i <= end.line)` covers both
  directions, which is why it is written as a range test rather than a single comparison.
- **`line_top` is measured from the top of `scroll.line`, not from buffer line 0.** The iterator
  starts its accumulator at `0.0` at the scroll line. Geometry is therefore viewport-relative in
  exactly the way `Editor::selection()` is.
- **One fork-only difference exists, and it does not matter here:** `LayoutRunIter::next` computes
  row height via a new `LayoutLine::line_height(base)` helper rather than
  `line_height_opt.unwrap_or(base)`. It changes only the numeric value for buffers using per-span
  line heights via `Attrs`. Since matcha treats `run.line_height` as opaque, there is no impact.

The one *unrelated* fork change worth knowing: `Buffer::hit` was fixed (PR #528) so the
first-glyph test is `x < glyph.x` rather than `x < 0.0`, which only matters for non-left-aligned
text. matcha delegates all hit-testing to `editor::State` and scopes v1 to
`text::Alignment::Default`, so it is unaffected — but it is independent evidence that alignment
makes hit-testing subtle, which is why that scope limit stays.

**B. `layout_runs()` stops at the first unshaped line.** `LayoutRunIter::next` does
`line.shape_opt()?` / `line.layout_opt()?`, and `?` on `None` ends the whole iteration.
Decoration geometry must run in `draw()` **after** `editor.highlight(..)` has shaped the visible
window, must never be called from `layout`, and must tolerate `layout_runs()` covering fewer rows
than the viewport. Consistently, `Editor::highlight` itself only attributes
`current_line..=last_visible_line` (graphics/src/text/editor.rs:812-829).

**C. Never clone the `Editor`.** `with_internal_mut` does `Arc::try_unwrap(..).expect("Editor
cannot have multiple strong references")` — a stray strong clone panics on the next mutation. Use
`downgrade()` if a handle is ever needed.

**D. iced master churn.** ~250 commits landed in this area in roughly two days, including the
`Parser`/`Highlighter` split. The exact-rev pin contains this; upgrades become deliberate,
reviewable steps rather than surprise breakage.

---

## Out of scope

- **Virtual / injected text.** Inlay hints never alter wrapping, hit-testing, or cursor positions.
  If implementation starts building a second `cosmic_text::Buffer`, a logical↔visual mapping layer,
  or custom mouse hit-testing, it has drifted into the architecture this plan excludes — stop.
- **LSP protocol and UTF-16↔UTF-8 conversion.** The widget consumes already-normalized
  `text::Position` values (`index` is a UTF-8 byte index). No `lsp-types` dependency.
- **Forking or patching iced.** Public APIs only.
- **Decoration indexing (`BTreeMap`), interval trees, glyph caching.** `layout_runs()` yields only
  visible rows and the `line_i` filter bounds the work further. Revisit only if profiling says so.
- **Center/right text alignment.** v1 supports `text::Alignment::Default` only.
- **Opaque background chips behind inlay hints.** Needs a `start_layer` push (see correction 2).

---

## Verification

```bash
cargo clippy --all-targets -- -D warnings   # must pass at the end of every phase
cargo test                                  # tiers 1 and 2
cargo test -- --ignored                     # tier 3 pixel snapshots
cargo run --example showcase                # visual: wrapping, Unicode, gutter, squiggles, hints
cargo run --example live                    # decorations replaced on edit
```

Manual checks in `showcase`: a diagnostic on one line squiggles that line **only**; scroll
vertically and horizontally — decorations stay glued to their text, gutter numbers scroll
vertically but not horizontally; scroll so a wrapped line straddles the top — its number sits on
its true first row; toggle wrapping — a diagnostic spanning a wrap renders as multiple fragments;
type into a line carrying a hint — no source text shifts; click in the gutter while
horizontally scrolled — the caret does not jump; drag-select across a hint — selection behaves
exactly as in `text_editor`.

**Done when** `CodeEditor` can replace `TextEditor` with no loss of editing behaviour, syntax
highlighting works through the stock `Parser`/`Highlighter` API, squiggles and hints track
correctly through wrapping and scrolling, the gutter aligns, no private iced internals are touched,
and the passivity invariant is enforced by a test rather than by convention.

---

## FOSS comparison — cosmic-edit and the cosmic-text fork

`cosmic-edit` (`/home/nickel/Programming/github/cosmic-edit/src/text_box.rs`) is a real code editor
on the same substrate, and the `hecrj/cosmic-text` fork has a `DecorationSpan` system upstream iced
does not expose. Four findings changed this plan; the rest are recorded as forward notes.

**Adopted — these fix real defects:**

1. **Diagnostics on empty lines and zero-width ranges render as nothing.** `LayoutRun::highlight`
   iterates `self.glyphs`, so an empty line yields no spans, and both `buffer.rs:96-99` and
   `:107-112` drop spans with `width <= 0.0`. A zero-width LSP diagnostic ("insert here") and a
   diagnostic on a blank line would both be silently invisible. cosmic-text's own renderer
   special-cases the empty-line case at `edit/editor.rs:106-115`. **matcha needs an explicit
   minimum-width fallback for both.** This is a genuine gap in the original design, which assumed
   "no glyphs → nothing to draw → correct".
2. **Gutter width must come from the total line count, never from the visible rows.** cosmic-edit
   computes it from `buffer.lines.len()` digits once (`text_box.rs:445-478`). Their commit
   `8e7dbaa "Fix click/drag offset when using line numbers"` fixed exactly the failure this
   avoids: deriving width from what is on screen makes it change as you scroll from 3- to 4-digit
   numbers, which shifts the text origin and **desyncs hit-testing from rendering**. Since matcha
   folds gutter width into `padding.left`, this would corrupt click-to-position.
3. **Squiggle quad count is a real budget, not a premature-optimization concern.** A 2px-period
   wave across an 80-column line is ~320 quads *per diagnostic*, each an instanced draw.
   cosmic-edit's whole architecture is a reaction to quad volume (commit `966cc0f` "Draw most items
   with GPU, except for line numbers"). Phase 5 now carries an explicit budget and fallbacks.
4. **Round underline thickness up to whole pixels.** The fork's `render.rs:64-72` does
   `(thickness * font_size).max(1.0).ceil()` — added by `6ef1ccbe "improv text decoration visuals"`
   because thin sub-pixel strokes gamma-blend into mud. Phase 5 found it also prevents an infinite
   loop, since the stroke width doubles as the horizontal step.

   Its sibling guard, `cmp::max(0, max - min)` (`edit/editor.rs:135`), was **not** adopted: it
   exists because that renderer truncates to `i32`, and matcha never leaves float space, where
   `Rectangle::intersection` already rejects non-positive extents. The hazard that *does* survive
   in float space is NaN — `intersection` is built on `f32::max`/`f32::min`, which ignore it — so a
   NaN quad is clamped to the whole clip rect rather than dropped.

**Independent confirmation:** cosmic-edit's first-visual-row test (`text_box.rs:552-559`) dedupes
against the previously *yielded* run, which is precisely the M4 bug this plan already fixed — when
you scroll into the middle of a wrapped line, `LayoutRunIter` culls the rows above and the
continuation row gets labelled with the line number. Their fix (`a08eb6b "Fix duplicate line
numbers when wrapping"`) did not go far enough. matcha's structural test is correct. One caveat
worth recording: the test relies on `glyph.start` being untouched, which `Ellipsize` rewrites —
irrelevant here, since code editors use `Ellipsis::None`.

**Recorded, not adopted:**

- **Do not copy cosmic-edit's software-rasterized gutter.** It is a hand-rolled alpha blender with
  `// TODO: improve performance` and `//TODO: draw line numbers using iced functions for
  performance` (`text_box.rs:192-264`, `:516`). matcha's `fill_text` gutter is the direction they
  want to move. The one thing they get that we must add back is caching — see Phase 4.
- **Fractional-DPI snapping.** `calculate_ideal` (`text_box.rs:413-434`) shrinks the widget up to
  16px to find a size where `floor(view * scale) / scale == view`, so 1px strokes stay crisp at
  1.25x/1.5x. Out of scope for v1; revisit if squiggles look blurry at fractional scaling.
- **`Attrs::metadata` is a free `usize` on every glyph** — a side channel for tagging ranges
  without a parallel structure. Unnecessary here since decorations arrive as explicit ranges.
- **Multi-line selection ranges extend to the line edge** in the reference renderer
  (`editor.rs:122-130`). matcha deliberately diverges: squiggles should not underline trailing
  whitespace, matching VS Code.
- **halo and iced_aw are not relevant.** halo uses the stock `text_editor` and renders WGSL
  diagnostics as a separate list, not inline; iced_aw has no text editor.

---

## Critique resolution log

### Round 1 — exploration against real source (3 parallel Explore agents)

Verified every API claim in `initial_plan.md` against `/home/nickel/Programming/repos/iced`. The
checkout fast-forwarded from `9854bd32` to `fa3bae52` mid-session (~250 commits), which landed the
`unify-text-editing` merge. Outcome: the initial plan's core architectural call (own
`graphics::text::Editor` directly, don't wrap `text_editor::Content`) was **confirmed correct** —
`Content.0` is private and there is no accessor. Its `Parser`/`Highlighter` split and
`editor::State` reuse claims also held. Three prescriptions were superseded (see "Three
corrections" above).

One subagent claimed "there is no public position→Rectangle query; your only lever is `move_to` +
`selection()`, which mutates." **Not accepted** — `Editor::buffer()` is public
(graphics/src/text/editor.rs:38) and exposes the whole shaped `cosmic_text::Buffer`. Verified
directly rather than taken at face value.

### Round 2 — Plan-agent critique

Raised one blocker, four major, eight minor. All either fixed or explicitly dismissed:

| # | Issue | Resolution |
| --- | --- | --- |
| B1 | `LayoutRun::highlight` has no line-bounds check; runs outside the range report **every** grapheme as selected, so one diagnostic squiggles every visible line | **Fixed.** Added the `.filter(\|run\| run.line_i >= start.line && run.line_i <= end.line)` guard + `TextRange` normalization. Verified against cosmic-text's own renderer, which guards identically at `edit/editor.rs:102-104` |
| M2 | Plan said `text_padding` "shrinks limits in layout" but omitted the matching `bounds.expand(..)`; mixing the two leaves the node `gutter_width` narrower than its container and text escapes the clip rect | **Fixed.** All five call sites now named explicitly |
| M3 | `Renderer::hint_factor()` and `Editor::hint_factor()` are different values; buffer space is scaled by the *editor's* | **Fixed.** Added the "Two hint factors" section: geometry divides by `editor.hint_factor()`, `fill_text` passes `renderer.hint_factor()` |
| M4 | `visible_line_rows` deduping on `line_i` mislabels a wrapped line straddling the viewport top, because `LayoutRunIter` skips the rows above it | **Fixed.** Switched to the structural test `glyphs.iter().map(\|g\| g.start).min() == Some(0)`, which is also RTL-correct |
| M5 | With no renderer backend the widget does not render blank — it **fails to typecheck** (`<() as text::Renderer>::Editor = ()`) | **Fixed.** Promoted to a hard manifest requirement in Phase 1 |
| m6 | `Buffer::cursor_position` already is `position_anchor` minus the scroll/hint math | **Adopted.** Removes a hand-rolled `layout_runs` scan and one unit of fork-drift exposure |
| m8 | Gutter clicks jump to an arbitrary column once `scroll.horizontal > gutter_width` | **Fixed.** Clamping requirement written into Phase 4 |
| m10, m11 | Phase 3 trips `dead_code` under `-D warnings`; `fira-sans` is not a default feature, so geometry tests would shape against arbitrary system fonts | **Fixed.** `pub mod geometry` re-export; `fira-sans` moved into Phase 1's manifest |
| m12, m13 | `gutter_width` needed in four methods; squiggles should use `line_y` (baseline) not the line box; `Text` has 11 required fields; iced's `last_status` quirk | **Fixed.** Folded into the architecture and phase docs |
| m7, m9 | Off-screen anchors correctly yield `None`; `visible_line_rows` closure needs `move` | **Noted**, no change of substance |

### Round 3 — /iced skill conventions

Three real corrections, not just polish: inherited advice to alias one of the two colliding `Style`
types **violated the no-`use foo as bar` rule** (resolved by parent-module paths);
`DiagnosticStyle`/`InlayStyle` were composite names (now `diagnostic::Style`/`inlay::Style` via
submodules); and `iced_test` must come from the **same git rev** as `iced` or the `Element`/
`Renderer` types will not unify. Also settled: no `core/` crate (matcha's logic *is* buffer
geometry — nothing iced-free to extract), and examples stay at Level 0 rather than importing the
`Action<I, M>` pattern into a 60-line demo.

---

## TODO before dispatching the first phase agent

- [ ] `git -C /home/nickel/Programming/repos/iced rev-parse HEAD` — confirm it still reports
      `fa3bae52874274c012a27d4bf11a83c49e1709ae`. It moved twice during planning. If it has moved
      again, **do not silently re-target**: the pinned rev in `Cargo.toml` is the contract, and the
      local checkout is only a reading reference. Note the divergence and carry on reading at the
      pinned rev via `git show`.
- [ ] Re-read `~/.claude/guides/RUST_STYLE.md` and the `/iced` skill conventions.
- [ ] `grep` the line numbers this plan cites in `widget/src/text_editor.rs` (323-607, 367, 383,
      443, 472, 501-509, 532) and `core/src/text/editor.rs` (315-769, 391, 422, 457, 488) — confirm
      they still resolve to the cited constructs.
- [ ] Confirm `Content.0` is still private and still has no accessor — the entire "own the editor
      directly" decision rests on it.
- [ ] Phase 3 only: `cargo fetch`, then read the **hecrj fork's** `src/buffer.rs` to confirm
      `LayoutRun::highlight`, `LayoutRun::cursor_position`, and `Buffer::cursor_position`. See
      Risk A for the fallback.

---

## Appendix A — Standard agent dispatch preamble

Paste this verbatim when spawning a phase agent, then append the phase doc path.

> You are implementing one phase of a planned widget library. Read in this order, fully, before
> writing any code:
> 1. `~/.claude/guides/RUST_STYLE.md` and the `/iced` skill conventions
> 2. `.claude/map/code-editor-widget/MAP_PLAN.md` — especially "Three corrections", "Key design
>    decisions", and "Risks"
> 3. Your phase doc: `.claude/map/code-editor-widget/MAP_PHASE_<N>.md`
> 4. `CLAUDE.md` and `MEMORY.md` if present
>
> **Ground truth is source, not memory.** iced master is pinned at
> `fa3bae52874274c012a27d4bf11a83c49e1709ae`, readable at `/home/nickel/Programming/repos/iced`.
> The text/editor APIs were rewritten recently — anything you recall about iced 0.13/0.14
> `text_editor` is wrong here. Read the file before you cite it.
>
> **You may run:** `cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo test`,
> `cargo fmt`, and any read-only inspection (`git show`, `grep`, `cargo tree`).
>
> **You must NOT run:** `cargo run --example ...` (opens a GUI window that will hang a headless
> agent — visual verification is the human's step), `cargo test -- --ignored` (pixel snapshots are
> platform-sensitive and auto-create golden files on first run, so a stray run silently bakes in a
> wrong baseline), `git commit`, `git push`, or any `cargo update` / edit of the pinned iced rev.
>
> **Before reporting done, self-review for:** composite type names that should use the module path
> (`DiagnosticStyle` → `diagnostic::Style`); `use foo as bar` (banned — import the parent module);
> `unwrap()` in library code (use `expect("invariant: …")` or `let … else`); wildcard match arms
> hiding new enum variants; missing docs on public items (`#![warn(missing_docs)]` is on); comments
> that narrate the plan ("Phase 2 adds…") or the edit history rather than explaining *why*; and
> `Widget::new` where an iced function helper exists.
>
> **Forbidden patterns:** trait abstractions over a closed set (the decoration kinds are closed —
> use concrete types); transition shims or compatibility layers; placeholder/`todo!()` macros left
> in a "finished" phase; speculative helpers extracted before a second call site exists;
> AI-attribution lines in commit messages or docs.
>
> **Hard architectural guard:** this widget never implements virtual/injected text. If you find
> yourself building a second `cosmic_text::Buffer`, a logical↔visual mapping layer, or custom mouse
> hit-testing, **stop and report** — you have drifted into the architecture the plan excludes.
>
> **If you hit a real design decision this plan does not cover, STOP and report it** rather than
> inventing an answer. Report: what you changed (file by file), what you verified and how, what you
> could not verify, and anything you think the plan got wrong.

---

## Appendix B — Resume guidance (`/goon code-editor-widget`)

A fresh session picks up like this:

1. Read `.claude/map/code-editor-widget/SESSION_HANDOFF.md` — it names the current phase and status.
2. Read `goon.yaml` for the verification checklist and key files.
3. Run the `quick` checklist. On a bare repo (before Phase 1) there is no `Cargo.toml` yet and
   `cargo` will fail — that is expected, not a regression.
4. Skim `MAP_PLAN.md` "Three corrections", "Key design decisions", and "Risks". These are the parts
   most likely to be re-derived incorrectly from memory.
5. Identify the next phase from the handoff's status table, work the "TODO before dispatch"
   checklist above, then dispatch that phase's `MAP_PHASE_N.md` using Appendix A.
6. Phases 5 and 6 are independent and may run in parallel; everything else is strictly ordered
   `1 → 2 → 3 → 4 → {5, 6} → 7`.
