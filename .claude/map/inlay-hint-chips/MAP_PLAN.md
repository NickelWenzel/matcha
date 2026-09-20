# Opaque inlay-hint chips

## Context

matcha's inlay hints are pure text overlays. They paint at their anchor and, mid-line, paint
*over* the code — which was an explicit product decision in the original plan
([code-editor-widget/MAP_PHASE_6.md](../code-editor-widget/MAP_PHASE_6.md)), not an oversight.
In practice a label sitting on top of the code it annotates makes both unreadable.

The fix is not to move the hint out of the way. It is to **make the hint opaque and momentary**:
each label gets a solid chip behind it that hides the code underneath, and the application reveals
hints only while a key is held. Occlusion stops being a defect and becomes the feature — the same
shape JetBrains uses for held-`Ctrl` hints and VS Code calls `onUnlessPressed`.

This supersedes the "hints may paint over source text" invariant. Everything else about hints —
that they reserve no room, reflow nothing, and are never hit-tested — is unchanged.

## Why this was previously excluded, and what changed

Phase 6 of the original plan says:

> Do not add an opaque background chip behind hint labels. A quad would render *beneath* the source
> text in the same layer, so the text would show through; doing it properly needs a
> `start_layer`/`end_layer` push, which is deliberately out of scope.

That is correct as stated and still is. The draw pass in `widget.rs` even carries the reasoning in
a comment. What it named was a scope boundary, not an impossibility — and the reframing to a
momentary peek is what makes the layer push worth paying for.

## Verified ground truth

Read at iced `fa3bae52874274c012a27d4bf11a83c49e1709ae`, the pinned rev.

| Claim | Status |
| --- | --- |
| A new layer composites entirely above the previous one | ✅ the quads→meshes→images→text order is nested inside `for layer in self.layers.iter()` ([wgpu/src/lib.rs:448](/home/nickel/Programming/repos/iced/wgpu/src/lib.rs#L448)) |
| `with_layer` clips to its bounds | ✅ `core/src/renderer.rs:18-33`; in-tree precedent at `widget/src/float.rs:307`, `widget/src/scrollable.rs:1338` |
| `fill_quad` accepts a `Background` | ✅ `fn fill_quad(&mut self, quad: Quad, background: impl Into<Background>)` (`core/src/renderer.rs:55`) |
| `Background` is `Copy` | ✅ `core/src/background.rs:5` — so `inlay::Style` keeps its `Copy` derive |
| `Plain::update` re-shapes on **attribute** change, not only content | ✅ falls through to `raw.compare(..)` and rebuilds on `Difference::Shape` (`core/src/text/paragraph.rs:99-117`) — a text-size change is caught, so a cached measurement cannot go stale |
| Per-line line height is **not** reachable | ✅ `highlighter::Style` carries only `color`/`style`; `Editor::update` takes one global `LineHeight`; every mutable buffer path goes through the private `buffer_mut_from_editor` (`graphics/src/text/editor.rs:1059`) |
| tiny-skia agrees with wgpu on layer ordering | ✅ `tiny_skia/src/lib.rs:98-178` — it matters because the test renderer is pinned to tiny-skia |
| A pushed layer is never flattened back | ✅ `Stack::merge` (`graphics/src/layer.rs:138-192`) bails when `candidate.end() > target.start()`; the base layer ends at level 5 (text) and the chip layer starts at level 1 (quads), so `5 > 1` — **the one thing that could silently undo this design, so re-check it** |
| `Padding::x()`/`y()` are the sum accessors | ✅ `core/src/padding.rs:166,171`. **`horizontal()`/`vertical()` are builders** taking a value (`:145,159`) — the trap is that they exist with the opposite meaning |

That last row is why the alternative design — a reserved band under each code line — was rejected.
cosmic-text supports per-span metrics (`Attrs::metrics()` → `LayoutLine::line_height_opt`) and
iced's own `Span` has `line_height`, but the `Editor` pipeline never plumbs either through. Growing
only the annotated lines would need an upstream iced change; growing them all costs roughly 40% of
the visible lines.

## Target state

```text
before                          after (while revealed)
──────────────────────          ──────────────────────
fn compute(value, other)        fn compute(v█████████████
           ‾‾‾‾‾╱               fn compute(v│param: value│
       label over code                     └────────────┘
       both unreadable              opaque chip hides the code
                                    beneath; label is legible
```

Each hint becomes a filled box sized to its own label, drawn inside one pushed layer so it
occludes the source text, the squiggles, the caret and the selection alike. Release the key, pass
`&[]`, and the code is untouched underneath.

## Key design decisions

**One layer for the whole hint pass, not one per hint.** A layer is a separate primitive batch; N
layers for N hints would be N batches. The pass already iterates hints in one loop — wrap the loop.

**Labels are measured, not guessed.** A chip has to be exactly as wide as the text it hides, which
means shaping the label before drawing it. `paragraph::Plain` per hint, cached in tree state,
mirroring the gutter's `widest_number` field. `Plain::update` returns whether it re-shaped and
compares attributes as well as content, so the cache is safe across text-size changes.

**Chips shift right to clear each other, keeping order.** Two hints on one row would otherwise
overlap as opaque boxes — much worse than two overlapping labels. Place left to right per visual
row, tracking a running right edge. Clamp the **box**, then derive the label position from it —
clamping the label instead lets the box start `padding.left` further left than the clamp allowed,
so consecutive chips overlap by exactly that much. Shifting beats dropping, though only just: a
chip pushed clear of the viewport renders nothing and is indistinguishable from dropped, so the
rule degenerates at the tail of a long cascade rather than being strictly better.

**Reveal is the application's job.** No widget API. An app already controls visibility by passing
`&hints` or `&[]`, and the keyboard handling is three lines of `keyboard::listen().filter_map(..)`.
Putting a binding in the widget would duplicate app state and force a key-choice policy that would
have to be configurable anyway. The example demonstrates the pattern.

**The chip fill defaults to the editor's own background.** That makes a chip read as a hole punched
in the code rather than as a badge stuck on top. `Background` rather than `Color`, so a caller can
use a gradient and so `text_editor::Style.background` passes straight through.

## What does not change

The passivity invariant survives intact, and `hints_do_not_shift_source_text` must still pass
**unmodified** — that is the regression test for this whole phase. Chips reserve no room, reflow
nothing, are not hit-tested, and never reach `editor.update`, `perform`, `State::update`,
`input_method`, or `operate`. A click lands on the character beneath a chip exactly as if the chip
were not there.

## Phases

**Phase 1 — Opaque chips.** `inlay::Style` gains `background` and `padding`; tree state gains a
per-hint measurement cache; the draw pass moves inside a guarded `with_layer` and lays chips out
left to right per row; `Probe` learns to record layer depth, and the tests that need it land here
too. [MAP_PHASE_1.md](MAP_PHASE_1.md)

**Phase 2 — Reveal and docs.** Hold-to-reveal in the example, and the eight places that still
document hints as transparent. No library code. [MAP_PHASE_2.md](MAP_PHASE_2.md)

`1 → 2`. The `Probe` work and the tests belong to Phase 1, not Phase 2: a phase whose exit criteria
are "the chip is as wide as its label" and "two hints do not overlap" cannot verify either without
them, and a chip in the base layer compiles, runs, and is invisible.

## Testing strategy

**One existing test is superseded, by design.**
`a_hint_is_drawn_as_text_and_never_as_a_chip_behind_it` (`widget.rs:2052-2068`) asserts
`annotated.quads.len() == bare.quads.len()` — "adding a hint adds zero quads". It was written to
pin the old scope guard and it does its job: it fires the moment a chip appears. This plan reverses
the decision it encodes, so it is replaced in place rather than deleted. Every other test keeps its
assertions; the `Style::new` call sites are mechanical edits.

New coverage, all mutation-tested (break the line, confirm a named test fails — the practice that
caught six vacuous tests across the original plan):

- **the chip is in a layer above the source text**, not merely drawn — needs `Probe` to record
  `start_layer`/`end_layer` **and** `fill_editor`, which is currently an empty stub
  (`widget.rs:1373-1380`), so the editor's own text is recorded nowhere and the comparison cannot
  be expressed. It matters doubly because the chip's default fill is the same `Background` the
  widget's frame quad uses, so layer depth is the only thing that tells them apart.
- the chip is as wide as its label plus padding
- two hints on one row do not overlap, and keep their left-to-right order
- a hint with no style still gets an opaque chip (the default is not transparent)
- passing `&[]` draws no chip and no layer

## Out of scope

- **A reserved band below each code line.** Blocked on per-line line height, which iced does not
  expose. Recorded above.
- **True virtual text.** Unchanged from the original plan: a second buffer, a logical↔visual map,
  and custom hit-testing.
- **A reveal binding in the widget.** Decided: the app's job.
- **Animating the reveal.** The app can drive opacity through `inlay::Style` if it wants to.
- **Dropping colliding chips.** Decided: shift right, keep order.

## TODO before dispatching the first phase agent

- [ ] `git -C /home/nickel/Programming/repos/iced rev-parse HEAD` — confirm still `fa3bae52…`. It
      moved twice during the original planning; the pin in `Cargo.toml` is the contract.
- [ ] Confirm the six ground-truth rows above still resolve, especially the per-layer loop at
      `wgpu/src/lib.rs:448` — the entire design rests on it.
- [ ] Re-read `~/.claude/guides/RUST_STYLE.md` and the `/iced` skill.
- [ ] Read the existing hint draw pass in `src/code_editor/widget.rs` and the `Probe` renderer in
      its `#[cfg(test)] mod tests`.

## Appendix A — Standard agent dispatch preamble

> You are implementing one phase of a planned iced widget library. Work in
> /home/nickel/Programming/repos/matcha on branch `code-editor-widget`.
>
> **Load the `/iced` skill first** (Skill tool, `skill: "iced"`). §3 and §4 apply to the widget;
> §2 applies only to the example, and there only at Level 0.
>
> Read in order: `~/.claude/guides/RUST_STYLE.md`; `.claude/map/inlay-hint-chips/MAP_PLAN.md`;
> your phase doc; the existing `src/code_editor/widget.rs`, `src/code_editor/decoration/inlay.rs`.
>
> **Ground truth is source, not memory.** iced is pinned at `fa3bae52…`, readable at
> `/home/nickel/Programming/repos/iced`.
>
> **You may run:** `cargo build`, `cargo build --examples`,
> `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo fmt`, `cargo doc --no-deps`,
> and read-only inspection.
>
> **You must NOT run:** `cargo run --example …` (GUI window, will hang you),
> `cargo test -- --ignored` (auto-creates snapshot baselines nobody has reviewed), `git commit`,
> `git push`, `cargo update`, or edit the pinned rev or `.cargo/config.toml`.
>
> **Style:** module-path naming (`inlay::Style`, never `InlayStyle`); no `use foo as bar`; no
> `unwrap()` in library code; `#![warn(missing_docs)]` is on; comments explain *why* in the present
> tense, never plan bookkeeping or edit history; test names read as sentences with no `test_`
> prefix; use the crate's re-exports (`crate::Position`) over `iced::advanced` paths; iced function
> helpers, never `Widget::new`.
>
> **Forbidden:** trait abstractions over a closed set; transition shims; `todo!()` left behind;
> speculative helpers before a second call site; AI-attribution lines anywhere.
>
> **Hard architectural guard:** never build a second `cosmic_text::Buffer`, a logical↔visual
> mapping layer, or custom hit-testing. Chips are draw-only. If you find yourself touching
> `editor.update`, `perform`, `State::update`, `input_method`, or `operate`, stop and report.
>
> **Testing:** mutation-test every load-bearing line — break it, confirm a named test fails. Six
> vacuous tests were caught this way across the original plan, in two distinct shapes: a test that
> re-derives its expectation by calling the function under test, and a test whose assertion has
> more slack than the property it claims. Watch for both.
>
> **If you hit a real design decision the plan does not cover, STOP and report it.**
>
> Report: changes file by file, what you verified and how, what you could not verify, exact final
> output of clippy/test/build --examples, and anything the plan got wrong — every agent on the
> original plan found real errors in its phase doc, including two instructions that were actively
> wrong.

## Appendix B — Resume guidance (`/goon inlay-hint-chips`)

1. Read `SESSION_HANDOFF.md` for the current phase and status.
2. Read `goon.yaml` for the checklist and forbidden commands.
3. Run the `quick` checklist. 68 tests should pass before any of this work starts.
4. Skim this plan's "Verified ground truth" and "Key design decisions".
5. Work the TODO checklist above, then dispatch the next phase doc using Appendix A.

Phases are strictly ordered `1 → 2`.

## Critique resolution log

### Round 0 — scoping

The request was "hints should not overlap the text, ideally appearing underneath it". Four
placements were costed: a reserved band below each line, end-of-line, true virtual text, and
z-order. Z-order was discarded immediately — putting hints *behind* the glyphs does not stop
overlap, it just makes the hint the loser.

The band was the preferred reading of "underneath" until the per-line line-height check came back
negative: cosmic-text supports it, iced's `Span` has it, and the `Editor` pipeline carries neither.
Growing every line uniformly costs ~40% of the visible lines.

The chip design came from the user and is better than any of the four, because it changes the
question. Every placement above tries to find somewhere safe to put a hint; the chip accepts that
the only safe place is *on top* and makes that legible, then leans on the app to keep it momentary.

### Round 1 — Plan-agent critique

Four blockers and five majors, all addressed. The three that mattered:

**The chip did not cover its own row.** `min_bounds().height` is exactly one code row, but the
default `offset.y` is `-size * 0.25` — a virtue for a transparent label ("raised, so it reads as a
note about the line"), a defect for an opaque box. At `text_size = 14` it left the bottom 2.625px
of the annotated row uncovered, exactly where descenders live, while clipping the row above. The
headline promise would have been false for the default style. Fixed by defaulting `offset.y` to
`0.0` and moving vertical breathing room into `padding` — which in turn makes one existing test
vacuous (`assert_eq!(0.0, 0.0 * 2.0)`), so it is repointed at `padding` rather than left to pass
for the wrong reason.

**The collision clamp overlapped by `padding.left`.** The draft clamped the label origin and then
derived the box by subtracting `padding.left`, so each box started inside its predecessor by that
much — the precise failure the step exists to prevent, and one Phase 2's own overlap test would
have caught after Phase 1 shipped it.

**`Padding::horizontal()` is a builder, not a sum.** It takes a value and returns a `Padding`; the
sums are `x()` and `y()`. The draft's hedge — "verify they exist and return sums" — was the worst
possible check, because they exist with the opposite meaning.

Also: one existing test is superseded rather than broken (the exit criterion now says so); the
`Probe` work moved from Phase 2 into Phase 1, because a phase cannot verify "the chip is as wide as
its label" without it; `Probe::fill_editor` has to start recording or "above the code" is
inexpressible; the layer push needs a guard on *placed* chips rather than supplied hints; and
`examples/live.rs` already owns a `subscription` that the Restore flow depends on, so Phase 2 adds
an arm rather than replacing the method.

The critique also verified the one thing the draft had not thought to check: `Stack::merge` can
flatten a pushed layer back into its predecessor, which would silently undo the whole design. It
cannot here — the base layer ends at level 5 and the chip layer starts at level 1 — but that is now
a ground-truth row with a standing instruction to re-check it.
