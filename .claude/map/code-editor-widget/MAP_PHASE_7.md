# Phase 7 — Examples, tests, docs

## Prerequisites

Phases 1-6 complete. The widget edits, highlights, gutters, squiggles, and hints.

## Goal

Make it usable by someone who is not the author, and lock the behaviour in with tests at all three
tiers.

**Exit criteria:** `cargo clippy --all-targets -- -D warnings` clean, `cargo test` green,
`cargo doc` warning-free, both examples run, and the passivity invariant is enforced by a test.

## Step 1 — `examples/live.rs`

`showcase.rs` uses static decorations. `live.rs` proves the real usage pattern: decorations are
*replaced wholesale* on every edit, the way an LSP round-trip delivers them.

A Level 0 single-screen app — plain `State`/`Message`/`update`/`view`, **no** `Action<I, M>` or
`Instruction`. That pattern earns its place at the third screen; here it would only obscure the
widget being demonstrated.

```rust
Message::Edit(action) => {
    let is_edit = action.is_edit();
    self.content.perform(action);

    if is_edit {
        // Stand in for an LSP response: re-derive decorations from the new text.
        self.diagnostics = analyze(&self.content.text());
        self.hints = infer_hints(&self.content.text());
    }
}
```

`analyze` can be trivial — flag every occurrence of `TODO`, or every line over 80 columns. The
point is that decoration vectors are rebuilt from scratch and the widget keeps up without stale
geometry or panics on positions that no longer exist.

Include a position that goes stale: a diagnostic at the end of the buffer, then delete text so it
points past the end. It must silently not render, never panic.

Use iced function helpers throughout — `column![]`, `row![]`, `button(text(..))`, `color!`,
`keyboard::listen().filter_map(..)` — never `Widget::new`.

## Step 2 — Tier 2 behaviour tests

`iced_test::Simulator` with a custom `FnMut(Candidate) -> Option<T>` selector
(`selector/src/lib.rs:130`). The headline test is the passivity invariant — the assumption the
entire architecture rests on:

```rust
#[test]
fn decorations_do_not_affect_behaviour() -> Result<(), Error> {
    // Drive the same input sequence against two editors: one bare, one with
    // diagnostics + hints + gutter. Assert byte-identical:
    //   content.text(), content.cursor(), layout Node::bounds(),
    //   and the click -> caret mapping.
}
```

Plus `typing_updates_the_content`, `clicking_places_the_caret`,
`the_widget_is_focusable_through_operate`.

Note `.click()` returns a `Result` that must be consumed — bind with `let _ =` when the hit target
is not needed. Test names read as sentences, no `test_` prefix.

## Step 3 — Tier 3 pixel snapshots

```rust
#[test]
#[ignore = "pixel output is platform-sensitive; run with --ignored"]
fn squiggles_render_under_the_right_glyphs() -> Result<(), Error> {
    let mut ui = simulator(/* ... */);
    let snapshot = ui.snapshot(&Theme::Dark)?;
    assert!(snapshot.matches_hash("snapshots/squiggles")?);
    Ok(())
}
```

`#[ignore]` matches iced's own convention for snapshots (`examples/todos/src/main.rs:626`).
**Warning:** `matches_hash` auto-creates the golden file on first run and returns `true`
(`test/src/simulator.rs:319-325`) — so a run on a machine where rendering is wrong silently bakes
in a wrong baseline. Generate goldens deliberately, once, and review the committed files.

Cover: squiggles, hints, gutter, and all three together.

## Step 4 — Docs

`#![warn(missing_docs)]` has been on since Phase 1, so this is a review pass, not a retrofit.

Crate-level docs in `lib.rs` need a complete runnable example and an explicit statement of the
three invariants a user must understand:

1. **Positions are UTF-8 byte indices** into a line (`text::Position`), not UTF-16 code units and
   not columns. Converting from LSP's UTF-16 is the application's job — matcha deliberately has no
   `lsp-types` dependency.
2. **Inlay hints are overlays.** They may paint over source text. They never change layout.
3. **Stale positions are ignored, never fatal.** A decoration pointing past the end of the buffer
   silently does not render.

Also document the hard requirements from Phase 1: iced must be the pinned git rev, and a renderer
backend (`wgpu` or `tiny-skia`) is mandatory — without one the widget does not typecheck.

## Step 5 — README

What it is, a screenshot from `showcase`, the `Cargo.toml` snippet with the pinned rev, a minimal
usage example, and a short "not in scope" list (virtual text, LSP protocol, center/right alignment)
so users are not surprised.

## Files

| File | Change |
| --- | --- |
| `examples/live.rs` | new — decorations rebuilt on edit |
| `examples/showcase.rs` | polish; ensure it covers wrap + Unicode + all three decorations |
| `tests/behaviour.rs` | new — Tier 2 `Simulator` tests |
| `tests/snapshots.rs` | new — Tier 3, all `#[ignore]` |
| `src/lib.rs` | crate docs with the three invariants |
| `README.md` | new |
| all `src/**` | doc review |

## Verification

```bash
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --no-deps                      # must be warning-free
cargo test -- --ignored                  # human-run, after reviewing goldens
cargo run --example showcase             # human-run
cargo run --example live                 # human-run
```

An agent must not run the `--example` or `--ignored` commands: the first opens a GUI window and
hangs a headless session, the second can bake in a wrong snapshot baseline.

## Manual checks in `showcase`

- A diagnostic on one line squiggles that line **only**.
- Scroll vertically and horizontally — decorations stay glued to their text; gutter numbers scroll
  vertically but not horizontally.
- Scroll so a wrapped line straddles the top — its number sits on its true first row.
- Toggle wrapping — a diagnostic spanning a wrap renders as multiple fragments.
- Type into a line carrying a hint — no source text shifts.
- Click in the gutter while horizontally scrolled — the caret does not jump.
- Drag-select across a hint — selection behaves exactly as in `text_editor`.

## Do NOT change in this phase

- No new features. If a gap turns up, write it down in the handoff as follow-on work.
- Do not un-`#[ignore]` the snapshot tests; they are platform-sensitive and will fail in CI.
- Do not add an LSP adapter, UTF-16 conversion, or an `lsp-types` dependency.
- Do not publish to crates.io. The pinned git dependency on an unreleased iced makes matcha
  unpublishable until iced 0.15 ships.
