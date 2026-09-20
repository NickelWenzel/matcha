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

`iced_test::Simulator`, `iced_test::selector`, and the public API — `Probe` lives in a private
`#[cfg(test)] mod` inside `widget.rs` and is not reachable from `tests/`, so nothing here can
assert on what is *drawn*.

The headline test is the passivity invariant — the assumption the entire architecture rests on —
but it cannot be written as "bare editor vs gutter + diagnostics + hints, byte-identical", because
the gutter deliberately moves the text origin and its width is not recoverable through the public
API (`Content` exposes neither its bounds nor its buffer). Split in two instead:

- **decorations shift a click by nothing at all** — gutter + diagnostics + hints against gutter
  alone, comparing the `Action::Click` points byte for byte;
- **the gutter shifts every click by one constant, positive amount** — bare against gutter alone,
  over several probe columns, which says "pure origin translation" without ever needing the number.

Then `content.text()`, `content.cursor()` and the widget's `Node::bounds()` compare directly
between bare and fully decorated, by clicking past the right edge of every line so both editors
clamp to the same character whatever their origins.

Do **not** re-test what `widget.rs` already pins: `hints_do_not_shift_source_text`,
`adding_diagnostics_changes_nothing_but_what_is_drawn`, and
`a_click_lands_on_the_same_character_with_the_gutter_as_without` each cover one decoration alone,
and `typing_into_a_focused_editor_reaches_the_content` covers typing. What is missing from
everywhere else is the realistic configuration and the two halves of `operate` — focus and
`text_input` — which nothing reaches.

Note `.click()` returns a `Result` that must be consumed — bind with `let _ =` when the hit target
is not needed. Test names read as sentences, no `test_` prefix.

## Step 3 — Tier 3 pixel snapshots

```rust
#[test]
#[ignore = "records a baseline on its first run; see this file's docs"]
fn squiggles_run_under_the_glyphs_they_mark() -> Result<(), Error> {
    let mut ui = Simulator::with_size(Settings::default(), SIZE, /* ... */);
    let snapshot = ui.snapshot(&Theme::Dark)?;
    assert!(snapshot.matches_image("snapshots/squiggles")?);
    Ok(())
}
```

`#[ignore]` matches iced's own convention for snapshots (`examples/todos/src/main.rs:626`).
Baselines land as `*-tiny-skia.png` because `.cargo/config.toml` pins the test renderer and
`Snapshot::path` suffixes the renderer name.

**Warning:** `matches_image` and `matches_hash` both auto-create the golden file on first run and
return `true` (`test/src/simulator.rs:261-326`) — so a run on a machine where rendering is wrong
silently bakes in a wrong baseline, and *nothing in this crate has ever been checked by eye*.
Generate goldens deliberately, once, and review the committed files.

**Images, not the hashes iced commits.** Reviewing a baseline is the entire reason for generating
it deliberately, and a `.sha256` file cannot be looked at. The comparison is equally strict either
way — both diff the whole RGBA buffer. Keep the viewport small (480x200) so a baseline stays a few
kilobytes, and shape with the default font, not `Font::MONOSPACE`: the bundled Fira Sans is what
keeps the pixels off whatever the host machine has installed.

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

What it is, a pointer to `cargo run --example showcase` (an agent cannot produce a screenshot and
must not invent a path to one), the `Cargo.toml` snippet with the pinned rev, a minimal usage
example, and a short "not in scope" list (virtual text, LSP protocol, center/right alignment) so
users are not surprised.

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

## Findings — implemented 2026-09-20

Three errors in this doc, all found by implementing it. Steps 2, 3 and 5 above are corrected in
place; what was wrong:

- **`matches_hash` cannot be reviewed.** Step 3 said to generate goldens deliberately and *review
  the committed files*, while prescribing a function whose output is a 64-character hash. The two
  cannot both hold. `matches_image` compares the same bytes and writes a PNG.
- **The Tier 2 headline test as sketched is not writable.** It asked for one comparison of "bare
  vs gutter + diagnostics + hints" that included the click → caret mapping. The gutter moves the
  text origin on purpose, so the two cannot agree on a raw click, and the offset that would make
  them agree is not obtainable from the public API — the only ways to get it are to measure the
  editor's bounds (`pub(super)`) or to derive it from the very `Action::Click` points under test,
  which is the vacuous-test shape Phase 6 already hit once. Split into two properties instead.
- **`typing_updates_the_content` was already written.** It duplicates
  `typing_into_a_focused_editor_reaches_the_content` in `widget.rs`. Replaced with a test of
  `operation.text_input`, the one half of `operate` nothing reached.

Two smaller things worth carrying forward:

- **`iced::application(..).theme(closure)` does not infer.** `.theme(|_state| Theme::GruvboxDark)`
  fails with "implementation of `Fn` is not general enough"; the parameter has to be annotated
  (`|_state: &State|`) or passed as a method, which is how `showcase` avoids it.
- **A Tier 2 test cannot tell "decorations are passive" from "decorations are ignored."** Only the
  `Probe` unit tests see draw calls. The integration test guards the difference by asserting its
  own fixtures name text the buffer actually has, which is the most it can do from outside.
