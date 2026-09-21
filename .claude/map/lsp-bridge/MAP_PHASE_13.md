# Phase 13 — The mock-LSP example and the docs

The last phase: something a human can run, and the prose that stops claiming
this feature does not exist.

## Prerequisites

Phase 10. The example feeds real `lsp_types` values through the bridge.

## Step 1 — `Cargo.toml`

The crate's **first** `[[example]]` block. It is forced: `required-features` has
no other spelling, and until now examples have been auto-discovered. Flag it in
the commit message as the departure it is.

```toml
[[example]]
name = "lsp"
required-features = ["lsp-types"]

[dev-dependencies]
# The example deserializes real notification payloads, so it needs a JSON
# parser. A dev-dependency only — the library still has exactly one dependency,
# and nothing in `lsp` deserializes anything.
serde_json = "1"
```

## Step 2 — `examples/lsp.rs`

Follow `examples/live.rs`, which is already a mock language server: `analyze()`
and `infer_hints()` there carry "Stands in for a language server" docs. This is
its successor, and the difference is that the payloads are **real wire JSON**
rather than functions over the text.

Shape:

- `//!` module doc saying what it stands in for and what is deliberately fake.
- `const`s holding real `textDocument/publishDiagnostics` and
  `textDocument/inlayHint` payloads as JSON string literals, plus a
  `textDocument/codeAction` response with an edit in it.
- A `Message` enum with the house `<Thing><PastParticiple>` naming, plus the
  invariant `Edit(Action)`.
- A subscription on a timer, so the mock messages arrive *after* the app has
  started — which is the whole point. A server's answer describes an older
  document, and an example where everything is ready at startup hides that.
- A key binding that applies the code action, so `Content::apply` and the
  revision guard are both exercised by hand.

**Show both paths**, because the plan promises both:

```rust
// The envelope path: one place where anything might arrive.
let message: matcha::lsp::Message = serde_json::from_str::<PublishDiagnosticsParams>(RAW)?.into();

// The direct path: the app already knows what it asked for.
let diagnostics = self.content.lsp(self.encoding).diagnostics(&params.diagnostics);
```

And show the revision pairing, since it is the thing an application cannot infer:

```rust
// Recorded when the request goes out.
let sent = (self.lsp_version, self.content.revision());
// ... and checked when the answer comes back.
match self.content.apply(&edit, self.encoding, sent.1) {
    Ok(()) => self.status = "applied".into(),
    Err(matcha::lsp::Error::Stale { .. }) => self.status = "buffer moved; re-request".into(),
    Err(other) => self.status = format!("{other}"),
}
```

## Step 3 — the three documents

**`src/lib.rs:71-80`** says matcha "has no `lsp-types` dependency and no opinion
about the protocol". Now it has an optional one and a documented opinion.
Rewrite to: the widget still takes native units and gains nothing; the bridge
sits beside it behind a feature that is off by default.

**`src/lib.rs:102-110`** needs more than a touch-up. It currently reads:

> A decoration naming a line the buffer does not have, or a byte past the end of
> the line it does name, is silently not drawn.

That is still true, and the bridge is built to keep it true — which is why an
out-of-range line passes through unconverted rather than clamping. But the
section now has to separate two things it currently blurs:

- **not currently drawable** — off screen, or naming a line that is gone. Silent,
  harmless, self-correcting.
- **no longer valid** — a position that still resolves but now means something
  else, because the buffer moved under it. Harmless for a decoration and
  unsurvivable for an edit, which is why `Content::apply` takes a revision.

That distinction is what the plan's *Divergences* section turns on, and the
crate docs are where a user meets it.

**`README.md:98-99`** lists the LSP protocol under *Not in scope*. Move it: the
core is still protocol-free, and the bridge is an opt-in feature. Add a short
section showing the three features and the `Cargo.toml` line, and mention the
new example beside the other two.

## Verification

```sh
cargo build --example lsp --features lsp-types
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo doc --no-deps --all-features
cargo fmt --all -- --check
```

**Do not run the example.** It opens a window and hangs a headless session;
`cargo run --example lsp --features lsp-types` is the human's step, and the
handoff should say so.

## Spot checks

| check | expected |
|---|---|
| `cargo build` with no features | still succeeds; nothing in the example is compiled |
| `cargo build --example lsp` without the feature | skipped, not a failure |
| `README.md` | no longer lists the protocol under *Not in scope* |
| `src/lib.rs` | no longer claims no opinion about the protocol |
| the `lib.rs` staleness section | distinguishes "not drawable" from "no longer valid" |
| `cargo doc --all-features` | the `lsp` module renders, with feature badges |

## What NOT to change

- **`widget.rs`, `geometry.rs`.** Still true in the last phase.
- **The other two examples.** `showcase` and `live` stay as they are.
- **The one-dependency claim.** `serde_json` is a dev-dependency; the library's
  runtime dependency is still iced alone, and the README should keep saying so.
