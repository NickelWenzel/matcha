# Session handoff — opaque inlay-hint chips

*Updated 2026-09-20. Status: Phase 1 done; Phase 2 (reveal + docs) remains.*

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
| Code | Phase 1 shipped: 74 tests (was 68), clippy `-D warnings` clean |
| Branch | Work belongs on `code-editor-widget` (not `master`), which is where the 68 tests live |
| Predecessor | `.claude/map/code-editor-widget/` — complete, read its `MAP_PHASE_6.md` first |

## The plan

[MAP_PLAN.md](MAP_PLAN.md). Two phases, strictly ordered.

| Phase | Doc | Status |
| --- | --- | --- |
| 1 — Opaque chips | [MAP_PHASE_1.md](MAP_PHASE_1.md) | **done** 2026-09-20 |
| 2 — Reveal and docs | [MAP_PHASE_2.md](MAP_PHASE_2.md) | pending |

The `Probe` work and all the tests live in Phase 1, not Phase 2. A phase whose exit criteria are
"the chip is as wide as its label" and "two hints do not overlap" cannot verify either without
them — and a chip in the base layer compiles, runs, and is invisible.

## What to build next: Phase 2

Hold-to-reveal in `examples/live.rs`, and the places that still document hints as transparent. **No
library code** — if Phase 2 finds itself editing `src/code_editor/`, something from Phase 1 was
left unfinished.

One entry in its table is already satisfied: the `widget.rs:771-775` comment went with the loop
Phase 1 replaced. Two stale comments remain in `widget.rs`, plus the `inlay.rs` docstrings,
`src/lib.rs:87`, `tests/snapshots.rs:95`, and `README.md:86`/`:100` — the last of which lists
opaque chips as explicitly *out of scope*.

Worth knowing before writing the reveal: Phase 1's measurement cache is
`resize_with(hints.len(), ..)`, so passing `&[]` drops every cached label and the next reveal
re-shapes all of them. One pass per reveal, not per frame — fine for the example, but if it ever
matters the fix is to keep the cache and skip the draw rather than empty the slice.

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
