# Phase 6 — Inlay hint overlays

## Prerequisites

Phases 1-4 complete. `geometry::position_anchor` exists and is tested.

Independent of Phase 5 — the two may run in parallel.

## Goal

Small text labels anchored to byte positions, drawn as **pure overlays** that never affect layout.

**Exit criteria:** hints track their anchors through scrolling and wrapping, and provably do not
move source text by a single pixel.

## The hard boundary

This is where the design is easiest to wreck. Inlay hints here are **overlays, not virtual text**.
They paint over whatever is beneath them; they do not reflow, reserve space, shift glyphs, or
participate in hit-testing. Overlap with source text is **permitted and documented** — that was an
explicit product decision, not an oversight.

If implementation starts building a second `cosmic_text::Buffer`, a logical↔visual mapping layer,
or custom mouse hit-testing to accommodate hints, **stop and report**. That is the full inline-inlay
architecture this plan excludes.

## Step 1 — Builder surface

```rust
/// Sets the inlay hints to overlay.
pub fn inlay_hints(mut self, hints: &'a [inlay::Hint<'a>]) -> Self

/// Sets how hints are drawn.
pub fn inlay_style(mut self, style: inlay::Style) -> Self
```

Borrowed, like diagnostics — application state, replaced on every LSP round-trip.

## Step 2 — Draw

In `draw`, after `editor.highlight(..)` and `State::draw(..)`:

```rust
for hint in self.inlay_hints {
    let Some(anchor) = geometry::position_anchor(buffer, hint_factor, hint.position) else {
        continue;   // scrolled out of view — nothing to draw
    };

    let size = text_size * self.inlay_style.size_scale;

    renderer.fill_text(
        Text {
            content: hint.label.to_string(),
            bounds: Size::new(f32::INFINITY, line_height),
            size,
            line_height: LineHeight::Absolute(line_height),
            font,
            align_x: text::Alignment::Default,
            align_y: alignment::Vertical::Top,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
            ellipsis: text::Ellipsis::None,
            hint_factor: renderer.hint_factor(),
        },
        text_origin + Vector::new(anchor.x, anchor.y) + self.inlay_style.offset,
        self.inlay_style.color,
        clip_bounds,
    );
}
```

`Text` has **eleven** required fields (`core/src/text.rs:23-62`) — there is no `Default`. Four are
easy to get wrong:

- **`shaping: Shaping::Advanced`** — hint labels carry non-ASCII (`→`, type names, CJK). `Basic`
  mangles them.
- **`wrapping: Wrapping::None`** and **`ellipsis: Ellipsis::None`** — a hint is one line, never
  reflowed or truncated.
- **`hint_factor: renderer.hint_factor()`** — the **renderer's**, matching the placeholder at
  `widget/src/text_editor.rs:549`. Note this differs from the geometry path, which divides by the
  **editor's** `hint_factor`. They are different values (MAP_PLAN, "Two hint factors").

Defaults: `size_scale: 0.75`, `offset: Vector::new(2.0, -size * 0.25)`, `color` from the theme's
weak text.

## Step 3 — Known limitation: wrap-boundary anchors

`Buffer::cursor_position` delegates to `cursor_glyph`, which **ignores affinity entirely**
(`buffer.rs:149-181`). At a soft-wrap boundary, byte index *i* is simultaneously `glyph.end` of the
last glyph on visual row N and `glyph.start` of the first glyph on row N+1; `find_map` takes the
first hit. So **a hint anchored exactly at a wrap boundary renders at the far right edge of the
previous row**, not at the start of the next.

Accept this for v1 and **write a test that asserts the current behaviour**, so the day someone
decides to fix it they find a failing test rather than a silent change. The fix, if ever needed, is
to select the run manually and honour `Cursor::affinity` instead of calling the convenience wrapper.

## Step 4 — Tests

The headline test is the passivity invariant. It is the reason this architecture was chosen, so it
gets a real assertion rather than a comment:

```rust
#[test]
fn hints_do_not_shift_source_text() {
    // Same content, same input sequence, with and without .inlay_hints(..).
    // Assert: Node::bounds(), content.text(), content.cursor(), and the
    // click -> caret mapping are byte-identical.
}
```

Plus: `a_hint_scrolled_out_of_view_is_not_drawn` (`position_anchor` → `None`),
`a_hint_at_a_wrap_boundary_anchors_to_the_previous_row` (documents Step 3), and
`a_hint_label_with_non_ascii_content_is_shaped` .

## Files

| File | Change |
| --- | --- |
| `src/code_editor/widget.rs` | `inlay_hints` + `inlay_style` fields, builders, draw pass |
| `src/code_editor/decoration/inlay.rs` | `Style` defaults |
| `examples/showcase.rs` | hard-coded hints: mid-line, end-of-line, on a wrapped line, non-ASCII label |

## Verification

```bash
cargo clippy --all-targets -- -D warnings
cargo test
```

## Spot checks

| Input | Expectation |
| --- | --- |
| Hint at end of a short line | Renders in the empty space after the text |
| Hint mid-line | Renders over the source text — expected and documented |
| Type before a hint's anchor | Source text does **not** shift; the hint moves with its anchor |
| Scroll vertically | Hint tracks its line |
| Scroll horizontally | Hint shifts with the glyphs |
| Hint on a line scrolled out of view | Not drawn, no panic |
| Hint anchored past end of line | Not drawn, no panic |
| Non-ASCII label (`→ Vec<String>`) | Renders correctly (`Shaping::Advanced`) |
| Click through a hint | Caret lands per the *source* text beneath, unaffected by the hint |
| Drag-select across a hint | Selection identical to Phase 4 |
| No hints supplied | Pixel-identical to Phase 4 |

## Do NOT change in this phase

- **No virtual text.** Hints never alter wrapping, layout, cursor positions, or hit-testing.
- Do not pass hints to `editor.update`, `perform`, `State::update`, or `input_method`.
- Do not add an opaque background chip behind hint labels. A quad would render *beneath* the source
  text in the same layer, so the text would show through; doing it properly needs a
  `start_layer`/`end_layer` push, which is deliberately out of scope (MAP_PLAN correction 2).
- Do not add an end-of-line placement mode. Anchored-with-overlap was the chosen policy; adding a
  second mode multiplies the geometry cases to test.
- Do not cache anchors in tree state.
- Do not add hover or click interaction on hints.
