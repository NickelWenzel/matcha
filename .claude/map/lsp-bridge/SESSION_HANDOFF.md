# Session handoff — the LSP bridge

Written by `/map`. Nothing is implemented yet; this hands over a planned,
critiqued, not-yet-started piece of work.

## What matcha is

A code editor widget for [iced](https://github.com/iced-rs/iced): a single
library crate, ~5100 lines, one dependency (git-pinned iced), no fork and no
patch. It edits, selects, wraps, scrolls and highlights exactly as
`iced::widget::TextEditor` does, and adds the three things a code editor needs
that the stock widget cannot draw — diagnostic squiggles, inlay-hint chips, and
a line-number gutter folded into the editor's own padding.

## What this plan adds

An **optional** `lsp` feature that translates language-server messages into
matcha's own types, converts between LSP's position encodings and the editor's
UTF-8 byte offsets, and applies single-document text edits to a `Content`.
Two further features, `lsp-types` and `gen-lsp-types`, supply conversions to and
from whichever LSP type crate the application already uses.

The README currently lists the LSP protocol under *Not in scope*. That is
deliberately reversed here — the *widget* still takes native units and gains no
field; the conversion sits beside it behind a feature nobody gets by default.

## Where things stand

| | |
|---|---|
| branch | `lsp_bridge`, cut from `master` at `2533bf9`, **no commits yet** |
| baseline | 69 unit + 5 behaviour + 2 doctests green; 4 snapshots `#[ignore]`d; clippy and fmt clean |
| plan | `MAP_PLAN.md` + 13 phase docs, **battle-tested through 6 rounds of critique** |
| phases done | **13 of 13** |

## The plan

Read `MAP_PLAN.md` in full before starting. Its **Divergences** section is not
optional: five rules differ between the decoration path and the edit path, and
every one of them was a bug in an earlier draft that would have corrupted the
user's file.

| # | phase | status |
|---|---|---|
| 1 | Feature scaffold | **done** |
| 2 | Encoding conversion, the `Bridge`, and `Replacement` | **done** |
| 3 | Revision counting, and what a converted position means | **done** |
| 4 | Diagnostics and inlay hints | **done** |
| 5 | Applying edits: validation and commit | **done** |
| 6 | Applying edits: caret and selection | **done** |
| 7 | Workspace edits | **done** |
| 8 | Code actions and the message envelope | **done** |
| 9 | `lsp-types`, inbound | **done** |
| 10 | `lsp-types`, outbound and client capabilities | **done** |
| 11 | `gen-lsp-types`, inbound | **done** |
| 12 | `gen-lsp-types`, outbound and client capabilities | **done** |
| 13 | The mock-LSP example and the docs | **done** |

Phase 2 is the gate. Past it, Phase 4 and Phase 5 are independent; Phase 6 is a
leaf nothing depends on; Phase 7 needs only Phase 5. Phase 8 is the join.
Phases 9–10 and 11–12 are independent pairs.

## What is left

The plan is finished. Nothing in it is outstanding, and the two things below
are for a person rather than a session.

**Run the example and look at it.** Nothing in this feature has been seen on a
screen: every decoration it produces was checked against a recording renderer
and by construction, exactly as the widget's own six phases were.

```sh
cargo run --example lsp --features lsp-types
```

Press **1**, **2**, **3** to deliver the payloads and **a** to apply the code
action. Worth checking by eye: that the squiggle sits under `unwrap()` and not
one byte to either side, since its column is UTF-16 and the line before it has
no multibyte characters to prove the conversion by; that the two hint chips read
as annotations rather than as code; and that editing between **3** and **a**
leaves the status line saying the buffer moved.

**Snapshot baselines still do not exist**, which predates this work and is
unchanged by it. `tests/snapshots.rs` documents the sequence: look at the
examples first, then `cargo test -- --ignored` to record, then open each PNG and
confirm it shows what its test name claims, then commit. This feature draws
nothing new, so it has no business running them.

## Follow-ons, deliberately not built

Each is named in the plan's *Out of scope* with the reason:

- **Incremental `didChange`.** matcha cannot report what changed — iced's
  `History` is private and `perform` exposes no change stream — so an
  application can only send the whole document. That is about 600 KB of
  allocation per keystroke on a 20k-line file, and it dwarfs everything the
  encoding layer does. Fixing it is an upstream iced change.
- **Pull diagnostics** (`textDocument/diagnostic`), which is a different shape
  and where the ecosystem is heading.
- **Semantic tokens, completion, hover, signature help.** The widget cannot draw
  or host any of them.
- **Range-limited inlay-hint requests.** matcha knows its visible rows only
  inside `geometry`, which this plan kept closed.

## Residual risk, stated plainly

The design is settled and the arithmetic is verified against probes. What six
rounds of critique kept finding, right to the last one, is **transcription
drift**: a rule stated correctly in one place and stale in another after a large
edit. Round 6 caught a `pub(super)` missing from one tuple element
(`error[E0616]`), a signature the surface block had not caught up with, and a
return type that disagreed between the plan and its phase doc.

Those are now fixed and mechanically re-checked. But if a phase does not
compile, suspect the plan before suspecting yourself, check the phase doc
against `MAP_PLAN.md`'s public-surface block, and **fix the plan too** — the
block is the artefact the next agent copies.

## Known traps, all verified by running code

These cost several rounds of critique to find. Do not rediscover them.

- **`position_anchor` returning `None` means "not drawable now", not "stale"** —
  it also returns `None` for a valid line scrolled off screen.
- **`Editor::overwrite` is a trap**: no `topmost_line_changed` (stale syntax
  colours) and no recorded `Change` (a later undo panics on stale cursors).
- **`move_to` neither validates nor clamps.** An out-of-range line panics in
  `delete_range`; an index off a char boundary panics in `String::split_off`.
- **A CRLF buffer plus a bare `\n` in `new_text` produces mixed endings** —
  measured: `"a\r\nb\r\nc"` → `"a\r\nX\nY\r\nc"`.
- **`Content::new()` and every single-line buffer report `LineEnding::None`**,
  whose `as_str()` is `""`. Normalizing against it deletes every newline.
- **cosmic-text reads `\n\r` as one terminator; LSP reads two.** Accepted, but
  it desynchronizes every subsequent line number when it occurs.
- **The two LSP crates need opposite URI accessors.** `as_str()` for
  `lsp-types`; `to_string()` for `gen-lsp-types`.

## Tech debt this plan deliberately leaves

- **Incremental `didChange` is impossible.** iced's `History` and `Internal` are
  private and `perform` exposes no change stream, so an app can only send
  `TextDocumentSyncKind::Full` — roughly 600 KB of allocation per keystroke on a
  20k-line file. That is the real performance cliff, and fixing it is an
  upstream iced change.
- **One undo step per edit.** A 50-edit code action is 50 Ctrl-Z presses.
- **No snapshot baselines exist** anywhere in the repo yet. See below.

## Key files to read first

1. `.claude/map/lsp-bridge/MAP_PLAN.md` — the plan; **Divergences** first
2. `.claude/map/lsp-bridge/goon.yaml` — checklist, style, forbidden commands
3. `src/lib.rs` — crate docs; the three invariants at `:71-110`
4. `src/code_editor.rs` — module layout and re-exports (27 lines)
5. `src/code_editor/content.rs` — the `pub(super)` editor; gains the revision counter
6. `src/code_editor/decoration.rs` — `TextRange`, private fields, ordering constructor
7. `src/code_editor/decoration/diagnostic.rs` — the `Severity` the bridge reuses
8. `Cargo.toml` — gains the crate's first `[features]` table
9. `/home/nickel/Programming/repos/iced/core/src/text/editor.rs` — ground truth
10. `/home/nickel/Programming/repos/iced/graphics/src/text/editor.rs` — `perform`, `move_to`

## Running things

```sh
cargo test                                          # baseline, must stay green
cargo test --features lsp                           # from Phase 1
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo doc --no-deps --all-features
```

The full matrix is in `goon.yaml` under `checklist`. A plain `cargo test`
exercises none of this feature, which is why the matrix exists.

**Never run** `cargo test -- --ignored` — snapshot baselines auto-create on
first run and return `true`, so a stray run bakes in an unreviewed baseline.
**Never run** `cargo run --example` — it opens a window and hangs a headless
session. Visual checks are the human's step.

## End-of-session checklist

1. `cargo test` and the feature matrix for whatever is implemented
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo fmt --all -- --check`
4. Record the phase's findings in `MAP_PLAN.md` and mark it done
5. Commit — conventional commits, **no AI attribution**
6. Write the next handoff

## Style

- No `mod.rs`; `foo.rs` + `foo/`
- **No composite type names, including in `lsp`** — it is `lsp::Hint` and
  `lsp::workspace::Edit`, *not* `InlayHint` and `WorkspaceEdit`. See the plan's
  naming table. The one exception is `lsp::CodeAction`, because `matcha::Action`
  already exists and means iced's editor action.
- No `use foo as bar`; disambiguate by module path
- No `unwrap()` in library code; `expect` with a reason
- `#![warn(missing_docs)]`; docs go *inside* the `cfg` with their item
- Test names read as sentences, no `test_` prefix
- Conventional commits: `type(scope): description`
- Ground truth is the iced source at the pinned rev, never recall
