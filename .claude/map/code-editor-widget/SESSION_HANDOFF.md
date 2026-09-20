# Session handoff — matcha

*Updated 2026-09-20 at the end of Phase 7. Status: all seven phases implemented. What is left is
not code — it is the visual check nobody has done yet.*

## What this is

**matcha** is a code editor widget for [iced](https://github.com/iced-rs/iced). It does everything
`iced::widget::TextEditor` does — editing, selection, wrapping, scrolling, IME, syntax highlighting
— plus three things the stock widget cannot: **diagnostic squiggles**, **inlay-hint overlays**, and
a **line-number gutter**. It is a single library crate depending only on public iced APIs, with no
fork or patch of iced itself.

The central architectural fact: the widget cannot be built by wrapping `TextEditor`, because
`text_editor::Content` hides its `R::Editor` behind a private field
(`widget/src/text_editor.rs:622-631`) and that editor is the only source of shaped text geometry.
So matcha owns a `graphics::text::Editor` directly and reads geometry from
`editor.buffer()` — a public accessor onto the shaped `cosmic_text::Buffer`.

## Where things stand

| | |
| --- | --- |
| Rust source | `Cargo.toml` + 10 files + 2 examples + 2 integration tests — Phases 1-7 done |
| Tests | 61 unit + 5 integration + 2 doctests green; 4 snapshot tests `#[ignore]`d and **never run** |
| Lints | `cargo clippy --all-targets -- -D warnings` clean; `cargo doc --no-deps` warning-free |
| Branch | `code-editor-widget`, 23 commits, **Phase 7 is uncommitted in the working tree** |

Phase 7 left these changes unstaged: new `README.md`, `examples/live.rs`, `tests/behaviour.rs`,
`tests/snapshots.rs`; modified `src/lib.rs` (crate docs) and `examples/showcase.rs` (status line
now reads "line N, byte M" rather than `N:M`, because bytes are the units decorations are anchored
in and a bare `N:M` reads as a column).

## The one thing that is still not done

**Nothing in this crate has ever been verified visually.** Seven phases of decoration geometry
have been checked by a recording renderer and by construction, never by eye. The squiggle's
one-stroke drop below the baseline, the hint's 0.75x raised label, and whether non-ASCII labels
fall back to a real font are all unconfirmed.

So, in order, by a human at a machine with a display:

1. `cargo run --example showcase` and walk the manual checklist in `MAP_PHASE_7.md`.
2. `cargo run --example live` — type into it, then press *Delete the last line* and confirm the
   pinned decorations stop drawing rather than panicking. Escape restores the buffer.
3. Only then `cargo test -- --ignored`, which **records** `snapshots/*-tiny-skia.png` rather than
   comparing against them: `Snapshot::matches_image` creates a missing baseline and returns
   `Ok(true)` (`test/src/simulator.rs:261-301`). A first run on a machine where rendering is wrong
   bakes the bug in.
4. Open the four PNGs and confirm each shows what its test name claims. Then commit them.

There is no `snapshots/` directory in the repository yet, and that is deliberate.

## The plan

Full detail in [MAP_PLAN.md](MAP_PLAN.md). Seven phases, each compiling independently and each
with a self-contained doc a subagent can execute without reading the others.

| Phase | Doc | Status |
| --- | --- | --- |
| 1 — Crate skeleton + `Content` | [MAP_PHASE_1.md](MAP_PHASE_1.md) | **done** 2026-09-19 |
| 2 — `CodeEditor` at parity with `TextEditor` | [MAP_PHASE_2.md](MAP_PHASE_2.md) | **done** 2026-09-19 |
| 3 — `geometry.rs` + decoration types | [MAP_PHASE_3.md](MAP_PHASE_3.md) | **done** 2026-09-19 |
| 4 — Line-number gutter | [MAP_PHASE_4.md](MAP_PHASE_4.md) | **done** 2026-09-19 |
| 5 — Diagnostic squiggles | [MAP_PHASE_5.md](MAP_PHASE_5.md) | **done** 2026-09-19 |
| 6 — Inlay hint overlays | [MAP_PHASE_6.md](MAP_PHASE_6.md) | **done** 2026-09-19 |
| 7 — Examples, tests, docs | [MAP_PHASE_7.md](MAP_PHASE_7.md) | **done** 2026-09-20 |

What Phase 7 got wrong and how it was corrected is recorded at the end of
[MAP_PHASE_7.md](MAP_PHASE_7.md) — chiefly that `matches_hash` writes a baseline nobody can
review, and that the passivity invariant as sketched could not be written without deriving its own
expectation from the code under test.

## Follow-on work — found, deliberately not built

Phase 7 added no features. These are the gaps it turned up:

1. **`matcha::geometry` is public API that no external caller can reach.** All three functions
   take `&cosmic_text::Buffer`, and `Content`'s editor is `pub(super)`, so there is no way to get
   the buffer belonging to a `Content` from outside the crate. The module was made `pub` in Phase 3
   only to dodge `dead_code` under `-D warnings` (critique m10); every function is now called from
   `widget.rs`, so `pub(crate)` would compile clean. Either narrow it, or add a `Content::buffer()`
   accessor and mean it.
2. **`Content` exposes no bounds.** This is what made the Tier 2 gutter test have to be written as
   "a constant translation" rather than "the same click". A `Content::bounds()` (the text area the
   editor was last laid out in) would make gutter arithmetic testable from outside and is what any
   application wanting to place its own overlays would need.
3. **Wrap-boundary anchors ignore affinity.** `Buffer::cursor_position` takes the earlier of the
   two rows a boundary byte belongs to, so a hint there lands at the far right of the row above.
   Pinned by `a_hint_at_a_wrap_boundary_anchors_to_the_previous_row`, which will fail loudly rather
   than shift silently when someone fixes it.
4. **`Cargo.lock` is untracked and not in `.gitignore`** — the open decision from before, still
   open. The library convention is to ignore it, but that assumes a crate published to crates.io;
   matcha pins an *unpublished* iced by git rev precisely for reproducibility. Recommended: commit
   it. Repo owner's call.
5. **An integration test cannot tell "decorations are passive" from "decorations are ignored."**
   Only the `Probe` unit tests see draw calls, and `Probe` is private. If that distinction ever
   needs covering from outside, it needs the pixel snapshots — which is part of why they exist.

## Known risks and debt

1. ~~**cosmic-text fork drift.**~~ **RETIRED 2026-09-19.** The `hecrj` fork was cloned at rev
   `1cdc3e0f` and read directly: every API the geometry design depends on is **byte-identical** to
   the crates.io 0.19.0 copy it was verified against. No fallback port needed.
2. **iced master moves fast.** The checkout advanced ~250 commits *during planning*, including the
   merge that split `Highlighter` into `Parser` + `Highlighter` and hoisted editor interaction into
   a public `editor::State`. The pinned rev contains this. Do not re-target it casually.
3. **`hint_factor` is dead code today.** `graphics/src/text.rs:404-418` returns `None`
   unconditionally, so every `/ hint_factor` division is a no-op and the non-1.0 path cannot be
   tested. The divisions are written anyway — and geometry uses the **editor's** factor while
   `fill_text` uses the **renderer's**; they are different values.
4. ~~**Squiggle quad count.**~~ **SETTLED by measurement in Phase 5.** 20 token-wide diagnostics is
   932 quads and +0.66 ms/frame (+4%) on the software renderer; 20 full 80-column ones is 12,420
   quads, free on wgpu and +18 ms on tiny-skia.
5. **Fractional-DPI blur.** 1px strokes at 1.25x/1.5x scaling will blur. cosmic-edit solves this by
   snapping the widget to a perfectly scalable size. Out of scope for v1; revisit if it looks bad.
6. **Snapshot baselines are platform-sensitive.** They must stay `#[ignore]`d: a CI machine with
   different fonts or a different renderer will not reproduce them.
7. **`initial_plan.md` is superseded.** It targeted an older iced and three of its prescriptions are
   now wrong. Read `MAP_PLAN.md` instead; keep the old file only as history.

## Key files to read first

1. [MAP_PLAN.md](MAP_PLAN.md) — especially "Three corrections", "Key design decisions", "Risks"
2. Your phase doc, `MAP_PHASE_<N>.md` — self-contained; you should not need the others
3. `goon.yaml` — checklists, forbidden commands, style rules
4. `/home/nickel/Programming/repos/iced/widget/src/text_editor.rs` — the port reference
5. `/home/nickel/Programming/repos/iced/core/src/text/editor.rs` — `Editor` trait + `editor::State`
6. `/home/nickel/Programming/repos/iced/test/src/simulator.rs` — `Simulator`, `Snapshot`, and the
   two `matches_*` functions that write a baseline when one is missing

## Commands

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all
cargo doc --no-deps
```

**Do not run unattended:** `cargo run --example <name>` (opens a GUI window, hangs a headless
session) or `cargo test -- --ignored` (pixel snapshots create their baseline on the first run and
return `true`, silently recording a wrong one).

The first `cargo build` fetches three git forks (cosmic-text, cryoglyph, winit). It is slow, not
hung.

## Testing practice

Every phase since 3 has mutation-tested its load-bearing lines: break each one, confirm a *named*
test fails. It has caught five vacuous tests, including one in Phase 6 that re-derived its
expectation from the function it was testing. Phase 7 applied it to all six lines its integration
tests claim to cover — the gutter fold in `text_padding`, a hint influencing that padding, both
halves of `operate`, the node expansion in `layout`, and the vertical padding — and each was caught
by exactly one named test. The first cut of `a_click_places_the_caret_on_the_row_it_lands_in`
missed the last of those, and now probes the top, middle and bottom of every row because of it.

## Style

Conventions come from the `/iced` skill and `~/.claude/guides/RUST_STYLE.md`; `goon.yaml` has the
full list. The ones that bite hardest here:

- **No `mod.rs`** — `foo.rs` + `foo/`.
- **No composite type names** — `diagnostic::Style`, never `DiagnosticStyle`. The one deliberate
  exception is `code_editor::CodeEditor`, which mirrors iced's own `text_editor::TextEditor`.
- **No `use foo as bar`** — import the parent module.
- **No `unwrap()`** in library code.
- **iced function helpers** (`button(text(..))`), never `Widget::new`.
- **Ground truth is source, not memory.** Read the pinned iced before citing it.

## Project tracking

No GitHub milestones, labels, or issues exist. The phase docs plus this handoff are self-sufficient.
If you want a board, ask the repo owner first — creating issues is outward-facing.
