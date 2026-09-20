# Phase 1 — Opaque chips

## Prerequisites

The `code-editor-widget` plan is complete: 68 tests green on branch `code-editor-widget`.

## Goal

Every inlay hint draws as an opaque box sized to its own label, in a layer above the source text,
so the code beneath is hidden rather than showing through — and a test fails if the layer push is
removed.

**Exit criteria:** `cargo clippy --all-targets -- -D warnings` clean; `cargo test` green.

**Exactly one existing test is replaced**, because this phase reverses the decision it encodes:
`a_hint_is_drawn_as_text_and_never_as_a_chip_behind_it` (`widget.rs:2052-2068`) asserts
`annotated.quads.len() == bare.quads.len()` — "adding a hint adds zero quads". It is superseded by
`a_hint_is_drawn_on_an_opaque_chip_above_the_code`. No *other* test's assertions may weaken; the
constructor call sites listed in Files are mechanical edits, not behavioural ones.

## Why a layer push, and why exactly one

Within a layer both backends render **quads → meshes → images → text**, so a `fill_quad` chip in
the base layer draws *beneath* the source text and hides nothing — which is why the current pass
has no chip and says so at `widget.rs:771-775`. That ordering is nested inside
`for layer in self.layers.iter()` ([wgpu/src/lib.rs:448](/home/nickel/Programming/repos/iced/wgpu/src/lib.rs#L448));
tiny-skia does the same at `tiny_skia/src/lib.rs:98-178`. A **new** layer composites entirely
above the previous one, chip included.

The one thing that could silently undo this: `Stack::merge`
(`graphics/src/layer.rs:138-192`) can flatten a pushed layer back into its predecessor. It bails
when `candidate.end() > target.start()`, and the base layer ends at level 5 (text,
`wgpu/src/layer.rs:332`) while the chip layer starts at level 1 (quads, `:308`) — `5 > 1`, so the
merge never happens. Verify that still holds; if it ever changes, occlusion breaks with no
compile error.

Push **one** layer around the whole loop, not one per hint: a layer is a separate primitive batch.

## Step 1 — `inlay::Style` gains a fill and padding

`src/code_editor/decoration/inlay.rs`:

```rust
pub struct Style {
    /// The label color.
    pub color: Color,
    /// What the chip behind the label is filled with.
    pub background: Background,
    /// Label size as a fraction of the editor's text size.
    pub size_scale: f32,
    /// Offset from the anchor, in logical pixels.
    pub offset: Vector,
    /// Space between the label and the edge of its chip.
    pub padding: Padding,
}

pub fn new(color: Color, background: Background, text_size: Pixels) -> Self
```

`Background` and `Padding` are both `Copy` (`core/src/background.rs:5`, `core/src/padding.rs:35`),
so `Style` keeps its derive.

The widget passes `style.placeholder` and `style.background` from `text_editor::Style`, so the chip
is filled with the editor's own background — a hole punched in the code rather than a badge stuck
over it, tracking the theme for free.

**`offset.y` must default to `0.0`, and the vertical breathing room moves into `padding`.** The
current default is `-size.0 * 0.25` (`inlay.rs:52`), written when a label was transparent and
"raised, so it reads as a note about the line" was a virtue. For an opaque box it is a defect:
`min_bounds().height` is exactly one code row (the label carries the *code's* line height,
`widget.rs:746-757`), so raising the box by 2.625px at `text_size = 14` leaves the bottom 2.625px
of the annotated row uncovered — precisely where descenders live — while clipping the row above.
A chip that does not cover its own row fails the entire premise.

Padding default: `Padding::from([0.0, size.0 * 0.25])`. Note `Padding::from([f32; 2])` is
`[vertical, horizontal]` (`core/src/padding.rs:224-232`), so that is horizontal-only, which is
what the label's own row-tall box wants.

Two tests in `inlay.rs` assert the old default and must be repointed, not deleted:

- `the_default_look_nudges_a_label_clear_of_the_glyph_it_annotates` (`:61-74`) asserts
  `offset.y < 0.0`. Drop that assertion; keep `offset.x > 0.0` and `size_scale < 1.0`.
- `the_default_offset_keeps_its_proportions_at_every_text_size` (`:76-82`) becomes
  `assert_eq!(0.0, 0.0 * 2.0)` — vacuously true, the exact failure shape the dispatch preamble
  warns about. **Repoint it at `padding`**, which does scale with `size`.

**Do not add a `Border` field.** A solid box is what was asked for.

## Step 2 — Measure every label

A chip has to be exactly as wide as the text it hides, so the label must be shaped before it is
drawn. Cache one `paragraph::Plain` per hint in tree state, mirroring the gutter's `widest_number`
(`widget.rs:115`). `State` already carries the `Paragraph` generic (`widget.rs:109`) — no new
parameter.

```rust
/// One per hint, so a chip can be sized to the label it hides.
labels: RefCell<Vec<paragraph::Plain<Paragraph>>>,
```

`Plain::update(Text<&str>) -> bool` re-shapes on content change *and* attribute change — it falls
through to `raw.compare(..)`, which checks version, size, line height, font, shaping, wrapping,
ellipsis, both alignments and hint factor (`graphics/src/text/paragraph.rs:230-254`). The cache
cannot go stale. `resize_with(hints.len(), Plain::default)` each frame; positional keying re-shapes
a few labels when the list reorders, which is cheap and far simpler than keying by content.

**Measure with `Plain`, but keep drawing with `fill_text`.** Switching the draw to
`fill_paragraph` looks like an obvious saving and is a trap: seven existing tests assert on
`probe.texts`, which only `fill_text` populates, and `Probe::fill_paragraph` is an empty stub. The
saving is illusory anyway — `Plain::update` only re-shapes when something changed, and
`fill_text`'s backend path is itself keyed on content and attributes.

## Step 3 — Lay chips out left to right, per row

Two opaque boxes overlapping is far worse than two labels overlapping, so chips are placed in order
along each visual row and nudged right until they clear.

```rust
// Measure and anchor first; a hint the buffer cannot place is simply absent.
let mut placed: Vec<(usize, Point, Size)> = Vec::new();

for (index, hint) in self.inlay_hints.iter().enumerate() {
    let Some(anchor) =
        geometry::position_anchor(content.buffer(), hint_factor, hint.position)
    else {
        continue;
    };

    let _ = labels[index].update(label.with_content(hint.label.as_ref()));
    placed.push((index, anchor, labels[index].min_bounds()));
}

// Row first, then column. Anchors on one row are two copies of one `line_top`
// rather than two measurements, so comparing them exactly is sound.
placed.sort_by(|(_, a, _), (_, b, _)| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));

let mut chips: Vec<(Rectangle, Point, usize)> = Vec::new();
let mut row = f32::NAN;          // NAN != NAN, so the first hint always opens a row
let mut right = f32::NEG_INFINITY;

for (index, anchor, size) in placed {
    if anchor.y != row {
        row = anchor.y;
        right = f32::NEG_INFINITY;
    }

    // Clamp the *box*, then derive the label from it. Clamping the label instead
    // lets the box start `padding.left` further left than the clamp allowed, so
    // consecutive chips overlap by exactly that much — which is the thing this
    // whole step exists to prevent.
    let left = (anchor.x + style.offset.x - style.padding.left).max(right);

    let chip = Rectangle {
        x: left,
        y: anchor.y + style.offset.y - style.padding.top,
        width: size.width + style.padding.x(),
        height: size.height + style.padding.y(),
    };

    right = chip.x + chip.width;
    chips.push((chip, Point::new(left + style.padding.left, anchor.y + style.offset.y), index));
}
```

`Padding::x()` and `y()` are the sum accessors (`core/src/padding.rs:166,171`). **`horizontal()`
and `vertical()` are builders** that take a value and return a `Padding` (`:145,159`) — using them
here compiles into something quietly wrong, or not at all.

Keep this arithmetic in **text-origin space**; add `translation` only at draw time, matching the
squiggle pass.

A chip shifted far enough right leaves `clip_bounds` and renders nothing — indistinguishable from
being dropped. Shifting is still the better rule, because it only degenerates at the tail of a long
cascade, but do not claim clipping preserves visibility.

## Step 4 — Draw inside one layer

```rust
// Nothing to show means no layer: an empty batch still costs one.
if !chips.is_empty() {
    renderer.with_layer(clip_bounds, |renderer| {
        for (chip, position, index) in chips {
            renderer.fill_quad(
                renderer::Quad { bounds: chip + translation, ..renderer::Quad::default() },
                style.background,
            );

            renderer.fill_text(
                label.with_content(self.inlay_hints[index].label.to_string()),
                position + translation,
                style.color,
                clip_bounds,
            );
        }
    });
}
```

Guard on `chips.is_empty()`, **not** `self.inlay_hints.is_empty()` — they differ whenever a hint
resolves to no anchor, which `a_hint_anchored_past_the_end_of_its_line_is_not_drawn`
(`widget.rs:2093-2109`) exercises with three hints of which two never place.

**Leave `snap` at its default** — the opposite of the squiggle pass, where `CRISP` flattened the
wave and had to be turned off. For an opaque box, snapping to the pixel grid keeps the edges crisp.

The borrow compiles as written: `min_bounds()` returns `Size` by value, so `placed` and `chips`
hold nothing borrowed from `labels`, and the closure's shared capture of `self` does not conflict
with the `&mut renderer` it is handed.

## Step 5 — Teach `Probe` about layers

Without this, **nothing can tell a chip in the base layer from a chip above it** — and a chip in
the base layer compiles, runs, and is invisible. That is the bug this phase must be able to catch,
so the recording renderer has to grow before the tests can mean anything.

```rust
struct Probe {
    quads: Vec<(renderer::Quad, Background, usize)>,   // + the layer it landed in
    texts: Vec<Filled>,                                 // likewise
    editor: Vec<(Point, Color, usize)>,                 // fill_editor was a stub
    layer: usize,
}

fn start_layer(&mut self, _bounds: Rectangle) { self.layer += 1; }
fn end_layer(&mut self) { self.layer = self.layer.saturating_sub(1); }
```

Recording a *depth* rather than a flag lets a test say "the chip is deeper than the editor's own
text" without hard-coding how many layers the widget happens to push. `saturating_sub` because a
bare `-= 1` panics in debug on an unbalanced pop.

**`Probe::fill_editor` is currently an empty stub** (`widget.rs:1373-1380`), so the editor's text is
recorded nowhere and "the chip is above the code" cannot be expressed. Record it. This matters
doubly because the chip's default fill is `style.background` — the *same* `Background` the widget's
own frame quad uses (`widget.rs:636-643`), so layer depth is the only thing that distinguishes a
chip from the frame.

Widening `quads` forces a one-token edit to `Probe::squiggles` (`widget.rs:1299-1305`), and
`Probe::labels`' docstring (`:1307`) claims issue order, which now means (y, x) order rather than
input order. Both are mechanical; name them so they are not mistaken for scope creep.

## Step 6 — Tests

Replace the superseded test in place:

```rust
#[test]
fn a_hint_is_drawn_on_an_opaque_chip_above_the_code() {
    // The chip has to be deeper than the editor's own text, or it hides nothing.
}
```

Mutation-test it: delete the `with_layer` wrapper and confirm **this** test fails. If it still
passes it is asserting that the chip exists rather than where it is, and it is worthless.

Plus:

- `a_chip_is_as_wide_as_the_label_it_hides` — do **not** compute the expectation by calling the
  same measurement the widget uses; that re-derives the expectation from the subject and passes for
  any value. Assert a relationship instead (wider than the label, no wider than label plus padding).
- `two_hints_on_one_row_do_not_overlap` — sort recorded chips by x, assert each starts at or after
  the previous right edge. Assert first that both were drawn *and* share a row, or a dropped hint
  makes it pass for the wrong reason. This is the test that catches the Step 3 clamp bug.
- `two_hints_on_different_rows_both_sit_at_their_anchors` — the row reset works.
- `a_hint_without_a_style_still_gets_an_opaque_chip` — the default fill is not transparent.
- `an_editor_without_hints_pushes_no_layer_and_draws_no_chip` — the Step 4 guard.

## Files

| File | Change |
| --- | --- |
| `src/code_editor/decoration/inlay.rs` | `background` + `padding` fields; `Style::new` signature; `offset.y` default to `0.0`; repoint the two tests at `:61-74` and `:76-82` |
| `src/code_editor/widget.rs` | `labels` cache; hint pass rewritten; `Probe` layer depth + `fill_editor`; `squiggles` tuple edit; one test replaced, six added |
| `src/code_editor/widget.rs:900` | `const HINT: inlay::Style` is a struct literal with no `..` — add both fields. `Color::from_rgb` and `Padding::new` are both `const`, so it stays a `const` |
| `Style::new` call sites | `widget.rs:741` (library), `widget.rs:2027`, `widget.rs:2158`, `inlay.rs:63`, `inlay.rs:78`, `inlay.rs:79` — six, all mechanical. No example or integration test calls it |

## Verification

```bash
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --examples
```

## Spot checks

| Input | Expectation |
| --- | --- |
| One hint mid-line | Chip covers the full row height under the label; nothing shows through, top or bottom |
| Chip width | `label.min_bounds().width + padding.x()` |
| Two hints close together on one row | Second shifts right until it **clears** the first — no overlap, not even by `padding.left` |
| Two hints on different rows | Neither shifts |
| Hint scrolled out of view | No chip, no layer, no panic |
| Three hints, two unplaceable | One chip, one layer |
| No hints supplied | No layer pushed at all |
| Delete the `with_layer` wrapper | `a_hint_is_drawn_on_an_opaque_chip_above_the_code` fails |

## Do NOT change in this phase

- No reveal API, no keyboard handling — Phase 2, and then only in the example.
- Do not touch `editor.update`, `perform`, `State::update`, `input_method`, or `operate`.
- Do not push a layer per hint; do not set `snap: false`; do not add a `Border`.
- Do not weaken any test other than the one named as superseded.
