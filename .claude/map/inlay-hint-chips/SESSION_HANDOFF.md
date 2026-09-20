# Session handoff — opaque inlay-hint chips

*Updated 2026-09-20. Status: both phases done. What remains is not code — it is the visual check nobody has done.*

## What this is

A follow-on to [code-editor-widget](../code-editor-widget/), which is complete — 29 commits, 68
tests, all seven phases done. That plan shipped inlay hints as transparent text overlays that paint
*over* the code, which was an explicit decision, not an oversight. In practice a label sitting on
top of the code it annotates makes both unreadable.

This plan makes each hint an **opaque chip**: a filled box sized to its own label, drawn in a
layer above the source text so the code beneath is hidden. That only works because hints become
**momentary** — the application reveals them while a key is held. Occlusion stops being a defect
and becomes the feature, the same shape JetBrains uses for held-`Ctrl` hints.

## Why not "underneath the text"

The original request was for hints that do not overlap, ideally sitting below the code. That design
was costed and rejected on evidence:

- Growing **only** the annotated lines is unreachable. cosmic-text supports per-span metrics
  (`Attrs::metrics()` → `LayoutLine::line_height_opt`) and iced's own `Span` has `line_height`, but
  the `Editor` pipeline plumbs neither: `highlighter::Style` carries only colour and font style,
  `Editor::update` takes one global `LineHeight`, and every mutable buffer path goes through the
  private `buffer_mut_from_editor`. It would need an upstream iced change.
- Growing **every** line costs roughly 40% of the visible lines, annotated or not.
- End-of-line placement avoids overlap but loses the anchoring that makes a hint useful.

The chip design came from the repo owner and is better than all three, because it changes the
question: rather than finding somewhere safe to put a hint, it accepts that the only safe place is
*on top* and makes that legible.

## Where things stand

| | |
| --- | --- |
| Plan | Written and critiqued once — four blockers, five majors, all addressed |
| Code | Both phases shipped: 76 tests (was 68), clippy `-D warnings` clean, `cargo doc` clean |
| Branch | Work belongs on `code-editor-widget` (not `master`), which is where the 68 tests live |
| Predecessor | `.claude/map/code-editor-widget/` — complete, read its `MAP_PHASE_6.md` first |

## The plan

[MAP_PLAN.md](MAP_PLAN.md). Two phases, strictly ordered.

| Phase | Doc | Status |
| --- | --- | --- |
| 1 — Opaque chips | [MAP_PHASE_1.md](MAP_PHASE_1.md) | **done** 2026-09-20 |
| 2 — Documentation | [MAP_PHASE_2.md](MAP_PHASE_2.md) | **done** 2026-09-20 |

The `Probe` work and all the tests live in Phase 1, not Phase 2. A phase whose exit criteria are
"the chip is as wide as its label" and "two hints do not overlap" cannot verify either without
them — and a chip in the base layer compiles, runs, and is invisible.

## What to do next — and none of it is code

**Run the examples.** Nothing in this crate has ever been checked by eye, and chips are the most
visual thing in it. `cargo run --example showcase` now has a hints toggle, so comparing annotated
against bare is a click. Things worth looking at specifically:

- Does a chip actually hide the code under it, or does the translucent border let a hairline
  through? The border is the label's colour at 0.4 alpha, and a quad mixes its border into its
  fill rather than compositing over — so the outermost pixel is only that opaque. The rounded
  corner is anti-aliased either way, so this may be invisible. If it is not, `color.mix(fill, 0.6)`
  in `inlay::Style::new` gives an opaque outline of the same apparent dimness, at the cost of
  rewriting the two alpha assertions in `the_default_chip_is_outlined_in_the_color_of_its_label`.
- Does the chip read as laid into the code, or pasted over it? Radius is 2.0, width 1.0.
- Two hints close together on one row: they shift right to clear each other, but each snaps to the
  pixel grid independently, so a sub-pixel clearance can still show as a one-pixel overlap.
- The squiggles, the gutter alignment, and everything else on the predecessor plan's manual
  checklist, which is still outstanding.

**Then, and only then, generate the snapshot baselines.** `cargo test -- --ignored` *records*
rather than compares on a first run: `matches_image` writes the golden and returns `Ok(true)`. The
chips changed what every hint snapshot captures, so a run before the visual check bakes in exactly
the thing nobody has looked at. Open the four PNGs, confirm each shows what its name claims, then
commit them.

**Decide about hold-to-reveal.** The design premise is that hints are momentary, which is what
justifies them being opaque. The toggle demonstrates the mechanism; a held modifier is the same
conditional on a different trigger, documented in the crate docs but not built. If you want it
shown rather than described, it is an arm on `live.rs`'s existing subscription — which the Restore
button also uses, so add to it rather than replacing it.

## One test is superseded, on purpose

`a_hint_is_drawn_as_text_and_never_as_a_chip_behind_it` (`widget.rs:2052-2068`) asserts
`annotated.quads.len() == bare.quads.len()` — "adding a hint adds zero quads". It was written to
pin the old scope guard and it works exactly as intended: it fires the moment a chip appears. This
plan reverses the decision it encodes, so it is **replaced in place**, not deleted, and the
replacement asserts the stronger property — that the chip sits in a layer above the code.

No other test may weaken. `hints_do_not_shift_source_text` in particular must pass untouched: chips
are still draw-only, and that is the invariant the whole architecture rests on.

## Known risks

1. **`Stack::merge` could flatten the pushed layer** back into its predecessor
   (`graphics/src/layer.rs:138-192`), silently undoing the design with no compile error. It does not
   today — the base layer ends at level 5 and the chip layer starts at level 1, and the merge bails
   when `candidate.end() > target.start()`. Re-check it; it is the one thing that fails quietly.
2. **The chip covers the caret and the selection** when they overlap, since both are base-layer
   quads. Correct for a momentary peek; worth knowing before it surprises someone.
3. **Nothing in this crate has ever been verified visually.** Unchanged from the predecessor, and
   it now matters more: chips change what every hint snapshot would capture. The manual checklist
   and the snapshot baselines are still outstanding.
4. **iced is pinned and the checkout moves.** It advanced ~250 commits during the predecessor's
   planning. The rev in `Cargo.toml` is the contract.

## Commands

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo doc --no-deps
cargo build --examples
```

**Do not run unattended:** `cargo run --example <name>` (GUI window, hangs a headless session) or
`cargo test -- --ignored` (writes unreviewed snapshot baselines).

## End-of-session checklist

- [ ] `cargo clippy --all-targets -- -D warnings` clean
- [ ] `cargo test` green
- [ ] `cargo fmt --all`
- [ ] Update the phase status table above
- [ ] Record what the plan got wrong in MAP_PLAN's critique log
- [ ] Write the next handoff

## Style

See `goon.yaml`. The ones that bite here: module-path naming (`inlay::Style`); no `use foo as bar`;
no `unwrap()` in library code; test names as sentences; comments explain *why* in the present
tense, never plan bookkeeping. And mutation-test every load-bearing line — that practice caught six
vacuous tests on the predecessor plan.
