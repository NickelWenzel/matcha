# Phase 2 — Reveal and docs

## Prerequisites

Phase 1 complete: hints draw as opaque chips inside a guarded layer push, `Probe` records layer
depth, and `a_hint_is_drawn_on_an_opaque_chip_above_the_code` fails if the push is removed.

## Goal

Show the hold-to-reveal pattern in a running example, and correct every place the crate still tells
users that hints merely paint over code.

**Exit criteria:** `cargo doc --no-deps` clean; `cargo build --examples` clean; no doc, comment, or
docstring still claims hints are transparent overlays.

This phase writes no library code. If it finds itself changing `src/code_editor/`, something from
Phase 1 was left unfinished.

## Step 1 — Hold-to-reveal in the example

The widget knows nothing about this. An app controls visibility by passing hints or not, which is
why there is no reveal API — a binding in the widget would duplicate app state and force a
key-choice policy that would have to be configurable anyway.

`examples/live.rs` **already has a `subscription`** (`:163-171`) mapping Escape → `Restored`, which
the Restore button flow depends on. **Add an arm to it; do not replace the method.**

```rust
keyboard::listen().filter_map(|event| match event {
    // ...the existing Escape arm stays...
    keyboard::Event::ModifiersChanged(modifiers) => {
        Some(Message::PeekHints(modifiers.control()))
    }
    _ => None,
})
```

and in `view`:

```rust
code_editor(&self.content)
    .on_action(Message::Edit)
    // Hints are a peek: the chips hide the code underneath, which is the point
    // while the key is held and unhelpful the rest of the time.
    .inlay_hints(if self.peeking { &self.hints } else { &[] })
```

**Know what the reveal costs.** Phase 1's measurement cache is `resize_with(hints.len(), ..)`, so
passing `&[]` drops every cached label and the next reveal re-shapes all of them. That is one
shaping pass per reveal rather than per frame, and fine for the example — but if it ever matters,
the fix is to keep the cache and skip the draw rather than to empty the slice.

`ModifiersChanged` is the right event — tracking `KeyPressed`/`KeyReleased` for a bare modifier is
fiddlier and misses focus changing mid-hold. `keyboard::listen` yields only `Ignored` events, and
neither matcha's `update` nor iced's `text_editor` calls `shell.capture_event()` at the pinned rev,
so the event does reach it. `Modifiers::control()` is at `core/src/keyboard/modifiers.rs:55`.

Level 0 throughout: plain `State`/`Message`/`update`/`view`, no `Action<I, M>`. iced function
helpers, never `Widget::new`.

## Step 2 — Correct every stale claim

The old invariant is repeated in more places than the obvious one. All of these now say the
opposite of what the code does:

| Location | What it still claims |
| --- | --- |
| `src/lib.rs:82-89` (the claim is at `:87`) | the documented invariant — "They may paint over source text" |
| `src/code_editor/widget.rs:716-721` | "Call order does not decide z-order" — true only *within* a layer now |
| ~~`src/code_editor/widget.rs:771-775`~~ | **Already done in Phase 1** — the comment described the loop Phase 1 replaced, so it went with it |
| `src/code_editor/widget.rs:262` | the `inlay_hints` docstring — "A hint paints over whatever is beneath it" |
| `src/code_editor/decoration/inlay.rs:1` | module docstring — "Labels drawn among the text without displacing it" |
| `src/code_editor/decoration/inlay.rs:15` | `Hint`'s docstring |
| `tests/snapshots.rs:95` | "where it paints over the code" |
| `README.md:86`, `README.md:100` | usage text and the "not in scope" list |

The crate-level invariant becomes something like:

> **Inlay hints are opaque overlays.** Each label is drawn on a solid chip that hides the code
> beneath it, in a layer above the text — so hints are best shown momentarily, while a key is held.
> They still never change layout: nothing reflows, and a click lands on the character under the
> chip exactly as if the chip were not there.

Keep the second half. It is still true and it is the invariant that matters.

`widget.rs:716-721` should not simply be deleted — the fact that call order does not decide z-order
*within* a layer is still what makes squiggles land under the glyphs without ordering work. Narrow
it rather than dropping it.

## Files

| File | Change |
| --- | --- |
| `examples/live.rs` | an arm on the existing subscription, `peeking` state, conditional `inlay_hints` |
| `src/lib.rs` | the invariant, reworded |
| `src/code_editor/widget.rs` | two stale comments (`:716-721` and the `inlay_hints` docstring); the third went with Phase 1's rewrite |
| `src/code_editor/decoration/inlay.rs` | two stale docstrings |
| `tests/snapshots.rs` | one stale comment |
| `README.md` | usage text and scope list |

## Verification

```bash
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --no-deps
cargo build --examples
```

`cargo run --example live` is the human's step: hold the key, confirm chips appear over the code
and vanish on release.

## Spot checks

| Input | Expectation |
| --- | --- |
| Hold the reveal key | Chips appear; code beneath is hidden, not blended |
| Release it | Code exactly as it was; no residue |
| Press Escape | Still restores the buffer — the existing arm survived |
| Type while holding | Editing works normally; chips track their anchors |
| Click through a chip | Caret lands on the character underneath |
| `grep -ri "paints over"` | No hits outside this plan's own docs |

## Do NOT change in this phase

- No library code. No reveal API on the widget.
- No new placement modes, no end-of-line fallback, no below-line band.
- Do not un-`#[ignore]` the snapshot tests or generate baselines: nothing in this crate has been
  checked by eye, and `matches_image` writes a golden on first run and returns `true`. The chips
  change what every hint snapshot would capture, so the baselines matter more than before, not less.
- Do not replace `examples/live.rs`'s subscription wholesale.
