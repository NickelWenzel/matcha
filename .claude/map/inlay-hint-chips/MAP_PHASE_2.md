# Phase 2 — Documentation

## Prerequisites

Phase 1 complete, plus the follow-on that outlined the chips and put a hints toggle in both
examples. 76 tests green.

## Goal

Every place the crate describes inlay hints now describes the old ones. Correct all of them, and
document the two things that shipped after the plan was written: the border, and that visibility is
the application's to control.

**Exit criteria:** `cargo doc --no-deps` clean; `cargo test` green (the doctest in `lib.rs` has to
keep compiling); no doc, comment, or docstring still claims hints are transparent, and none claims
opaque chips are out of scope.

**This phase writes no library code and adds no tests.** If it finds itself editing
`src/code_editor/*.rs` beyond doc comments, or touching `examples/`, something is wrong.

## What changed since this doc was written

**Hold-to-reveal is not built.** The original plan demonstrated the momentary peek with a held
modifier. Both examples now carry a **toggle** instead, added at the owner's request. It is the
same mechanism — an app passes `&hints` or `&[]`; the widget has no visibility API — differing only
in the trigger, so building a second control into an example that already has one would add a knob
without teaching anything.

**Document the peek in prose instead.** The crate docs should say that hints are best shown
momentarily and that binding the same conditional to a held key is how you get that, without an
example doing it. That preserves the premise that justifies opaque chips while leaving the examples
legible.

**Chips are outlined**, from the label's colour scaled down. `inlay::Style` now carries
`border: Border` alongside `background` and `padding`. Nothing documents this yet.

## Step 1 — Correct every stale claim

Line numbers verified against the current tree; re-check before editing, since Phase 1 moved
several.

| Location | What it still claims |
| --- | --- |
| `README.md:100-101` | **"Opaque chips behind inlay hints"** is in the *not in scope* list, with the layer reasoning as justification. The headline correction: it is not only in scope, it shipped |
| `README.md:9` | "drawn among the code without displacing any of it" |
| `README.md:86` | "They may paint over source text" |
| `src/lib.rs:82` | the invariant's heading — "Inlay hints are overlays" |
| `src/lib.rs:87` | "line paints over the code there" |
| `src/code_editor/widget.rs:264` | the `inlay_hints` docstring — "A hint paints over whatever is beneath it" |
| `src/code_editor/widget.rs:~722` | "Call order does not decide z-order" — true only *within* a layer now |
| `src/code_editor/decoration/inlay.rs:1` | module docstring — "Labels drawn among the text without displacing it" |
| `src/code_editor/decoration/inlay.rs:15` | `Hint`'s docstring — "drawn without affecting layout" |
| `tests/snapshots.rs:95` | "where it paints over the code" |

**Two that must NOT change**, because they are still true and deleting them would lose something:

- `README.md:94` — "**Virtual text.** Hints are overlays; nothing here pushes code aside to make
  room." Chips hide code; they still do not displace it. This entry is the distinction the whole
  architecture rests on and it belongs in the scope list.
- `widget.rs:~722` is **narrowed, not deleted.** That call order does not decide z-order *within a
  layer* is still what makes squiggles land beneath the glyphs with no ordering work. Only the
  implication that it holds across layers is now wrong.

The crate-level invariant in `src/lib.rs` becomes something like:

> **Inlay hints are opaque overlays.** Each label sits on a filled, outlined chip that hides the
> code beneath it, drawn in a layer above the text — so hints read best shown momentarily rather
> than left on. They still never change layout: nothing reflows, and a click lands on the character
> under a chip exactly as if the chip were not there.

Keep that second sentence. It is still true and it is the invariant that matters.

## Step 2 — Document what shipped after the plan

**Visibility is the application's.** The widget has no reveal API, deliberately: an app turns hints
off by passing an empty slice. Both examples show it with a toggle. The crate docs should say this
plainly and note that binding the same conditional to a held modifier is how the momentary peek the
design assumes is built — one or two sentences, no example.

**The chips are outlined.** `inlay::Style` carries `border`, derived by default from the label's
colour scaled down. Worth a sentence where `Style` is documented, including the tradeoff already
recorded in the code: a quad mixes its border into its fill rather than compositing it over, so the
outermost pixel is only as opaque as the border — a soft edge rather than a hole, since the rounded
boundary is anti-aliased either way.

## Files

| File | Change |
| --- | --- |
| `src/lib.rs` | the invariant, reworded; visibility and the peek pattern |
| `README.md` | the scope-list entry removed, two usage claims corrected, the border and the toggle mentioned |
| `src/code_editor/widget.rs` | the `inlay_hints` docstring; the z-order comment narrowed |
| `src/code_editor/decoration/inlay.rs` | module and `Hint` docstrings; a sentence on `border` |
| `tests/snapshots.rs` | one stale comment |

## Verification

```bash
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --no-deps
cargo build --examples
grep -rn "paints over\|without displacing" src/ tests/ README.md    # expect no hits
```

`cargo run --example showcase` stays the human's step.

## Spot checks

| Input | Expectation |
| --- | --- |
| `README.md` scope list | No longer claims opaque chips are out of scope; still claims virtual text is |
| `src/lib.rs` invariant | Says hints hide code, and that they still never change layout |
| `cargo doc` output | The `Style` docs mention the border; `inlay_hints` does not promise transparency |
| The `lib.rs` doctest | Still compiles — it is a real compile check, not prose |
| `grep -rn "paints over"` | No hits outside this plan's own docs |

## Do NOT change in this phase

- No library code beyond doc comments. No tests. No example changes.
- Do not add a reveal API, a placement mode, an end-of-line fallback, or a below-line band.
- Do not delete `README.md:94` — hints still do not displace code, and that is the distinction
  worth keeping.
- Do not un-`#[ignore]` the snapshot tests or generate baselines. Nothing here has been checked by
  eye, and chips changed what every hint snapshot would capture, so an unreviewed baseline would
  bake in precisely the thing nobody has looked at.
