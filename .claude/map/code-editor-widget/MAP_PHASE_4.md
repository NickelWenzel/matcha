# Phase 4 — Line-number gutter

## Prerequisites

Phases 1-3 complete. `geometry::visible_line_rows` exists and is tested.

## Goal

A left gutter showing line numbers, aligned with their logical lines through scrolling and
wrapping, without breaking click-to-position.

**Exit criteria:** numbers align with their lines; a wrapped line straddling the viewport top shows
its number on its *true* first row; clicking still places the caret correctly — that last one is
the regression that proves the padding trick.

## The core idea

The gutter is modelled as **extra left padding**. `editor::State::update` derives editor-relative
coordinates as `cursor_pos - Vector::new(padding.left, padding.top)` at every one of its four use
sites (`core/src/text/editor.rs:391` press, `:422` drag, `:457` touch press, `:488` touch move);
scroll and `input_method` never touch padding. So folding the gutter into `padding.left` makes
hit-testing, click-to-position, and drag-select correct with **zero** extra code.

```rust
fn text_padding(&self, gutter_width: f32) -> Padding {
    Padding { left: self.padding.left + gutter_width, ..self.padding }
}
```

## Step 1 — Gutter width, computed from the total line count

**Critical: derive the width from `content.line_count()`, never from the visible rows.**

```rust
fn gutter_width(&self, state: &State<Parser>, renderer: &Renderer) -> f32 {
    let Some(style) = self.gutter else { return 0.0 };
    let digits = self.content.line_count().max(1).ilog10() as usize + 1;
    // measure the string "0".repeat(digits) via the cached paragraph, add padding
}
```

cosmic-edit's commit `8e7dbaa "Fix click/drag offset when using line numbers"` fixed exactly the
bug this avoids. If the width is derived from what is on screen, it changes as you scroll from
3-digit to 4-digit numbers — which shifts the text origin, and because matcha folds that width into
`padding.left`, **it desyncs hit-testing from rendering**. Clicks land on the wrong column.

A width change also changes the wrap width, so it must trigger a reshape. That happens for free:
`layout` passes `limits.bounds()` to `editor.update(..)` every pass, and cosmic-text reshapes when
the bounds change.

Measure with a `paragraph::Plain<Renderer::Paragraph>` cached in a `RefCell` on the tree state
(`draw` only gets `&Tree`), keyed on digit count. `Plain::update(Text<&str>) -> bool` and
`min_bounds()` are public (`core/src/text/paragraph.rs:83-149`).

`gutter_width` is needed in `layout`, `update`, `draw`, **and** `mouse_interaction`, and a
mid-update relayout can change it — so it is one pure method called from all four, not a value
cached across them.

## Step 2 — Thread `text_padding` through all five call sites

Replace `self.padding` with `self.text_padding(gutter_width)` at **all five**. Mixing them is a
silent layout bug:

| Method | Site | Upstream ref |
| --- | --- | --- |
| `layout` | `limits.shrink(..)` | `widget/src/text_editor.rs:367` |
| `layout` | `bounds.expand(..)` | `:383` |
| `update` | `padding` arg to `State::update` | `:443` |
| `update` | `layout.bounds().shrink(..)` for `request_input_method` | `:472` |
| `draw` | `bounds.shrink(..)` for `text_bounds` | `:532` |

Shrinking by `text_padding` while expanding by `self.padding` leaves the widget node
`gutter_width` narrower than its container (with the default `width: Length::Fill`,
`limits.resolve` returns the already-shrunk max), and the editor's text area then extends past the
node's right edge, so `State::draw`'s clip no longer bounds the text.

## Step 3 — Draw the numbers

In `draw`, after `State::draw`, iterate `geometry::visible_line_rows(buffer, hint_factor)`:

```rust
for (line, top) in geometry::visible_line_rows(buffer, hint_factor) {
    renderer.fill_text(
        Text {
            content: (line + 1).to_string(),
            bounds: Size::new(gutter_width, line_height),
            size: text_size,
            line_height,
            font,
            align_x: text::Alignment::Right,
            align_y: alignment::Vertical::Top,
            shaping: text::Shaping::Basic,       // digits only
            wrapping: text::Wrapping::None,
            ellipsis: text::Ellipsis::None,
            hint_factor: renderer.hint_factor(),
        },
        Point::new(bounds.x + self.padding.left + gutter_width, bounds.y + self.padding.top + top),
        style.color,
        viewport_intersection,
    );
}
```

Four things to get right:

- **Wrapped continuation rows get no number.** `visible_line_rows` already filters to first rows.
- **The gutter scrolls vertically but never horizontally.** `top` comes from `line_top`, which is
  already vertically scroll-adjusted; simply never subtract `scroll.horizontal` here.
- **`Shaping::Basic` is enough** for ASCII digits and is cheaper than `Advanced`.
- **`align_x: Alignment::Right`** with a fixed `bounds.width` right-aligns for free (the renderer
  does `position.x -= min_bounds.width`, `wgpu/src/text.rs:562-565`). If per-row x jitter ever
  appears, the fallback is cosmic-edit's trick: left-align a space-padded string
  (`format!("{:width$}", n)`, `line_number.rs:31`) so every number occupies identical columns.

**Performance note.** `fill_text` re-shapes each number every frame. cosmic-edit avoids this by
caching shaped numbers keyed on `(number, digit_width)` (`line_number.rs:28-52`). Ship the simple
version first, then measure with a 10k-line file; if the gutter shows up in a profile, cache a
`Paragraph` per visible row rather than reaching for their rasterizer, which they themselves want
to delete (`text_box.rs:516`: `//TODO: draw line numbers using iced functions for performance`).

## Step 4 — Gutter clicks

`Action::Click` computes `x = ((position.x + scroll.horizontal) * hint) as i32`
(`graphics/src/text/editor.rs:545`). A gutter click produces a negative `position.x`:

- at `scroll.horizontal == 0`, cosmic-text clamps to line start (`buffer.rs:1170-1173`) — harmless;
- once `scroll.horizontal > gutter_width`, the sum goes **positive** and the caret jumps to an
  arbitrary column.

In `update`, when a press or drag lands inside the gutter, clamp the x to the text origin or
swallow the event before calling `State::update`. Also return something other than
`Interaction::Text` from `mouse_interaction` over the gutter — `Interaction::default()` is right
until gutter click-to-select-line exists.

## Step 5 — Builder and style

```rust
/// How to draw the line-number gutter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Line-number color.
    pub color: Color,
    /// Gap between the numbers and the text, in logical pixels.
    pub spacing: f32,
}
```

`.gutter(Style)` enables it; absent means `gutter_width` is `0.0` and every site above is a no-op,
so Phase 2 behaviour is preserved exactly when the gutter is off.

## Files

| File | Change |
| --- | --- |
| `src/code_editor.rs` | declare `pub mod gutter;` |
| `src/code_editor/gutter.rs` | new — `Style`, width measurement, draw helper |
| `src/code_editor/widget.rs` | `gutter` field, `.gutter(..)` builder, `text_padding`, `gutter_width`, five threaded call sites, gutter draw, click clamp |
| `examples/showcase.rs` | enable the gutter |

## Verification

```bash
cargo clippy --all-targets -- -D warnings
cargo test
```

## Spot checks

| Input | Expectation |
| --- | --- |
| 5-line file | Numbers 1-5, aligned on their right edge |
| Scroll down in a 500-line file | Numbers track; gutter width does not change mid-scroll |
| File crosses 99 → 100 lines | Gutter widens once, text reflows, hit-testing still correct |
| Wrapped line spanning 3 rows | One number on row 1; rows 2-3 blank |
| Scroll so a wrapped line straddles the top | Its number is **not** shown on the continuation row |
| Click on a character | Caret lands on that character, not offset by `gutter_width` |
| Click in the gutter | Caret goes to line start; **no jump** when horizontally scrolled |
| Drag-select from text into the gutter | Selection behaves as in Phase 2 |
| Gutter disabled | Pixel-identical to Phase 2 |

## Do NOT change in this phase

- No squiggles, no inlay hints — Phases 5 and 6.
- Do not derive gutter width from visible rows, the longest visible number, or a constant
  per-character estimate. Total line count, measured, or it desyncs hit-testing.
- Do not use a separate `padding` value in only some of the five call sites.
- Do not rasterize numbers by hand. `fill_text` is the point.
- Do not add gutter click-to-select-line, breakpoint dots, fold arrows, or relative line numbers.
- Do not change `visible_line_rows` to dedupe on `line_i` — that reintroduces cosmic-edit's
  `a08eb6b` bug.
