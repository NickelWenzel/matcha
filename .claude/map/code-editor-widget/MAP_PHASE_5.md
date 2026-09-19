# Phase 5 — Diagnostic squiggles

## Prerequisites

Phases 1-4 complete. `geometry::range_fragments` returns `Fragment { bounds, baseline }` including
the minimum-width fallback for blank lines and zero-width ranges.

Independent of Phase 6 — the two may run in parallel.

## Goal

Wavy underlines under diagnostic ranges, tracking correctly through wrapping and scrolling.

**Exit criteria:** a diagnostic on one line squiggles **that line only**; a range crossing a wrap
renders as multiple fragments; blank-line and zero-width diagnostics are visible; nothing shifts
when decorations are added.

## Step 1 — Builder surface

```rust
/// Sets the diagnostics to underline.
pub fn diagnostics(mut self, diagnostics: &'a [Diagnostic]) -> Self

/// Sets how each severity is drawn.
pub fn diagnostic_style(
    mut self,
    style: impl Fn(diagnostic::Severity) -> diagnostic::Style + 'a,
) -> Self
```

Diagnostics are borrowed, never owned or cached in tree state — they are application state that
changes every LSP round-trip. Storing them in the widget tree would fight iced's rebuild model.

## Step 2 — Where the drawing goes

In `draw`, **after** `editor.highlight(..)` and after `State::draw(..)`:

```rust
for diagnostic in self.diagnostics {
    let style = (self.diagnostic_style)(diagnostic.severity);

    for fragment in geometry::range_fragments(buffer, hint_factor, diagnostic.range) {
        draw_squiggle(
            renderer,
            fragment.bounds + translation,
            fragment.baseline + translation.y,
            clip_bounds,
            style,
        );
    }
}
```

Two non-negotiables:

- **After `editor.highlight(..)`.** `layout_runs()` terminates at the first unshaped line
  (`LayoutRunIter::next` does `line.shape_opt()?`), and shaping for the visible window only
  completes during `highlight`. Calling geometry from `layout` yields nothing or a truncated set.
- **Call order does not set z-order.** Within a layer, both backends draw quads before text
  (`wgpu/src/lib.rs:459-618`), so `fill_quad` squiggles land *beneath* glyphs no matter when they
  are issued. That is the desired result for underlines — do not add layer pushes to "fix" it.

`translation` is `text_origin - Point::ORIGIN`, where `text_origin` is
`bounds.shrink(self.text_padding(gutter_width)).position()` — the same value `State::draw` receives.

## Step 3 — `draw_squiggle`

```rust
fn draw_squiggle<R: renderer::Renderer>(
    renderer: &mut R,
    fragment: Rectangle,
    baseline: f32,
    clip_bounds: Rectangle,
    style: diagnostic::Style,
)
```

Emit a triangle wave of short quads hanging one stroke below the baseline (`offset` is
`thickness`), each intersected against `clip_bounds` and skipped when the intersection is empty.

**Do not step by `wavelength / 2`.** Quads are axis-aligned, so one quad per half-period cannot
draw a diagonal — it draws a *square* wave. A triangle wave has to be rasterized at the resolution
of its own stroke: step by `thickness`, and give each quad the full vertical extent of the stroke
over its slice so consecutive quads meet and the wave is unbroken. That is roughly **621 quads per
80-column line**, about twice what an earlier estimate here suggested.

Three details that decide whether it looks right:

- **`snap: false` is required.** `Quad::default().snap` is `CRISP`, and `crisp` is one of iced's
  default features (`Cargo.toml:25`). Snapping every segment to the pixel grid flattens the wave
  into a dashed line. `widget/src/float.rs:318` is the in-tree precedent for setting it explicitly.
- **Round thickness up to whole pixels:** `thickness.max(1.0).ceil()`. The cosmic-text fork does
  exactly this (`render.rs:64-72`, commit `6ef1ccbe`) because thin sub-pixel strokes gamma-blend
  into mud — but here it is also about **liveness**, not just looks. The stroke width doubles as
  the horizontal step, so an unguarded `thickness: 0.0` makes the loop never advance: an infinite
  loop inside `draw`. Guard the period the same way, `(wavelength / 2.0).max(thickness)`, or a
  zero wavelength divides by zero.
- **Anchor to the baseline, not the line box.** `Fragment::baseline` carries `run.line_y`. Placing
  the wave at `fragment.bounds.y + fragment.bounds.height` drifts away from the glyphs as line
  height grows.
- **Do not import the reference renderer's `cmp::max(0, max - min)` clamp.** It exists because
  that code truncates to `i32` and passes a `u32`; this code never leaves float space.
  `Rectangle::intersection` already returns `None` unless both extents are positive
  (`core/src/rectangle.rs:195-215`), and stepping with `next = (x + thickness).min(right)` under
  `x < right` keeps `next - x` strictly positive. A `.max(0.0)` here would be unfalsifiable — no
  test can make it fire.
- **The real float hazard runs the other way: `Rectangle::intersection` does not reject NaN.**
  It is built on `f32::max`/`f32::min`, which *ignore* NaN and return the other operand — so a NaN
  quad is not dropped, it is clamped to the entire clip rect and painted as a solid band across the
  whole editor. `wavelength` and `thickness` are public fields, so guard them at the top of the
  function rather than trusting the clip to catch it.

## Step 4 — Mind the quad budget

**This is a real constraint, not a premature-optimization worry.** A 2px-period wave across an
80-column line at ~8px/char is ~320 quads *per diagnostic*, and iced emits one instanced quad per
segment. Twenty diagnostics on screen is ~6400 quads per frame. cosmic-edit's entire architecture
is a reaction to quad volume (commit `966cc0f` "Draw most items with GPU, except for line
numbers").

**Measured, so this is settled: ship the straightforward version.** 20 token-wide diagnostics is
932 quads and costs +0.66 ms/frame (+4%) on the software renderer; 20 full 80-column diagnostics is
12,420 quads, free on wgpu and +18 ms on tiny-skia. Nothing below is warranted unless a profile
says otherwise — they are recorded as contingencies, not plans:

1. Coarsen `wavelength` to 4-6px — fewer segments, and closer to how VS Code actually looks.
2. Merge horizontally adjacent segments at the same y into single wider quads.
3. Draw one stretched, tiled image per fragment instead of per-segment quads.

Do **not** jump straight to a custom shader or the `geometry` feature. Note also that the fork has
`// TODO: Wavy` at `attrs.rs:232` — if upstream lands `UnderlineStyle::Wavy`, this whole path
collapses into a `DecorationSpan` and comes free from `editor.render()`.

## Step 5 — Tests

Geometry is already covered by Phase 3. Add behavioural tests here:

- `a_diagnostic_on_one_line_squiggles_only_that_line` — the B1 regression, at widget level.
- `a_diagnostic_on_a_blank_line_is_visible` — the minimum-width fallback.
- `a_zero_width_diagnostic_is_visible` — LSP "insert here".
- `adding_diagnostics_does_not_change_layout` — the passivity invariant: `Node::bounds()`,
  `content.text()`, and `content.cursor()` identical with and without `.diagnostics(..)`.

## Files

| File | Change |
| --- | --- |
| `src/code_editor/widget.rs` | `diagnostics` + `diagnostic_style` fields, builders, draw pass |
| `src/code_editor/decoration/diagnostic.rs` | `draw_squiggle` + a sensible `Style` default per severity |
| `examples/showcase.rs` | hard-coded diagnostics: mid-line, spanning a wrap, on a blank line, zero-width |

## Verification

```bash
cargo clippy --all-targets -- -D warnings
cargo test
```

## Spot checks

| Input | Expectation |
| --- | --- |
| Diagnostic on line 3 of a 20-line file | Squiggle on line 3 only |
| Range crossing a soft wrap | Two fragments at different `y`, together covering the range |
| Diagnostic on a blank line | A short visible squiggle |
| Zero-width range | A short visible squiggle |
| Scroll vertically | Squiggles move with their text, exactly |
| Scroll horizontally | Squiggles shift left with the glyphs, not independently |
| Diagnostic partly scrolled off the left | Clipped at the text edge; does not paint into the gutter |
| Error / Warning / Info / Hint | Four distinct colors from `diagnostic_style` |
| Multibyte line | Squiggle covers the right glyph cluster |
| No diagnostics supplied | Pixel-identical to Phase 4 |

## Do NOT change in this phase

- No inlay hints — Phase 6.
- Do not reorder `State::draw` or add layer pushes to get squiggles above text. Underlines belong
  beneath glyphs, and within a layer call order does not control z-order anyway.
- Do not extend squiggles to the full line width on intermediate lines of a multi-line range. The
  reference renderer does that for *selections*; underlining trailing whitespace is wrong here.
- Do not cache fragments in tree state. Recompute per frame from the buffer.
- Do not pass diagnostics to `editor.update`, `perform`, hit-testing, or IME positioning. They are
  draw-only. This is the invariant the whole design rests on.
- Do not add hover tooltips or click-to-navigate — that needs event handling this phase excludes.
