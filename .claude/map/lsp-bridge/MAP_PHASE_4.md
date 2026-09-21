# Phase 4 — Diagnostics and inlay hints

The payload types a server actually sends, and the two batch methods that turn
them into decorations the widget already draws.

## Prerequisites

Phase 2: `Bridge`, `clamp`, `exact`, `range`, and `Replacement`.
**Not** Phase 5 — that is why `Replacement` ships in Phase 2.

## Goal and exit criteria

`bridge.diagnostics(&d)` and `bridge.hints(&h)` produce vectors the existing
`.diagnostics()` and `.inlay_hints()` builders accept, and an integration test
drives a real widget with them.

## Step 1 — `lsp/diagnostic.rs`

Severity **reuses the existing type**. `decoration::diagnostic::Severity`
(`decoration/diagnostic.rs:8-19`) is already exactly `Error | Warning |
Information | Hint`, already public, already zero-dependency. A second copy
would buy an identity `From` and nothing else.

```rust
/// A problem a language server reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Where the problem is.
    pub range: Range,
    /// How severe it is.
    ///
    /// An absent severity, and an out-of-range one (`0`, `5`, `99` are all
    /// legal JSON), both become [`Severity::Error`]. The protocol leaves the
    /// choice to the client: helix picks `Warning`, VS Code and zed pick
    /// `Error`. `Error` is right here because these severities differ only by
    /// the colour of a squiggle, and under-reporting a real error is the worse
    /// of the two mistakes.
    pub severity: decoration::diagnostic::Severity,
    /// What the server said. The widget does not draw this — its own
    /// `Diagnostic` is `{ range, severity }` — so it is here for the
    /// application to show on hover or in a list.
    pub message: String,
    /// Which tool reported it: `rustc`, `clippy`, `rust-analyzer`. This is how
    /// a user tells three sources apart in one gutter, and part of what a
    /// server matches on when a diagnostic is echoed back.
    pub source: Option<String>,
    /// The error code, losslessly.
    pub code: Option<Code>,
    /// A URL documenting the code.
    pub code_description: Option<String>,
    /// Carried, never drawn. See below.
    pub tags: Vec<Tag>,
    /// Other places implicated in this problem.
    pub related: Vec<Related>,
    /// Opaque server JSON, echoed back in `CodeActionContext.diagnostics`.
    ///
    /// Never parsed here, which is why this module needs no serde. Without it,
    /// "apply the quickfix for this diagnostic" silently returns nothing from
    /// rust-analyzer, which keeps its assist ids in this field. The round trip
    /// is semantic rather than byte-exact — key order may differ — and every
    /// server compares semantically.
    pub data: Option<String>,
}

/// An error code, which the protocol allows to be either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Code {
    /// A numeric code.
    Number(i32),
    /// A textual code, such as `unused_variables`.
    Text(String),
}

/// Extra meaning a server attaches to a diagnostic.
///
/// **Carried and never drawn.** `Unnecessary` means dimmed and `Deprecated`
/// means struck through; both are text-rendering effects, and the widget's own
/// `Diagnostic` has only a range and a severity. Drawing them would mean
/// changing `widget.rs`, which this plan does not do. They are here so an
/// application can act on them — greying an unused import in its own UI, or
/// echoing the diagnostic back intact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    /// Unreachable or unused code.
    Unnecessary,
    /// Deprecated or obsolete code.
    Deprecated,
}

/// Another location implicated in a diagnostic.
///
/// Kept whole, URI and all. matcha has no notion of a file — there is no
/// filename on a [`Content`] — so it cannot tell "somewhere in this buffer"
/// from "somewhere else", and guessing would be worse than handing the
/// application the URI it already knows how to interpret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Related {
    /// The document, as the server spelled it.
    pub uri: String,
    /// Where in that document.
    pub range: Range,
    /// What the server said about it.
    pub message: String,
}
```

## Step 2 — `lsp/hint.rs`

```rust
/// A label a language server wants shown inside a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    /// Where to anchor it.
    pub position: Position,
    /// The label, with any parts already flattened.
    ///
    /// The protocol allows a list of parts, each of which may carry its own
    /// location. Flattening drops those, which gives up ctrl-clickable type
    /// hints — **deliberate, and irreversible**: adding them back later is a
    /// breaking change to this type.
    pub label: String,
    /// Whether it annotates a type or a parameter.
    pub kind: Option<hint::Kind>,
    /// Whether the server asked for a space before the label.
    ///
    /// Carried for echo-back; it does not change the drawn chip.
    /// `decoration::inlay::Hint` is `{ position, label }` only, and the
    /// widget's own `inlay::Style::offset.x` already clears the annotated
    /// glyph, so the common case reads correctly without it.
    pub padding_left: bool,
    /// Whether the server asked for a space after the label.
    pub padding_right: bool,
    /// A tooltip. matcha has no hover surface; carried for the application.
    pub tooltip: Option<String>,
    /// The edits that turn this hint into real text — "accept this hint".
    /// Hand them to [`Content::apply`].
    pub text_edits: Vec<Replacement>,
    /// Opaque server JSON, for `inlayHint/resolve`.
    pub data: Option<String>,
}
```

## Step 3 — the batch methods

```rust
impl<'a> Bridge<'a> {
    /// Converts diagnostics for [`CodeEditor::diagnostics`].
    ///
    /// **Total**: the returned vector has exactly as many entries as the input,
    /// in the same order, so index *n* of the result is index *n* of the input.
    /// That correspondence is load-bearing — the widget's `Diagnostic` carries
    /// no message, so an application showing one on hover has to keep the
    /// `lsp::Diagnostic` vector alongside, and dropping entries here would
    /// silently unalign the two.
    ///
    /// A diagnostic naming a line the buffer no longer has is still counted.
    /// [`Bridge::range`] returns `None` for it, and the entry emitted in its
    /// place has BOTH endpoints on that out-of-range line, so the widget's own
    /// run filter drops it and nothing draws. Do not fall back to clamping
    /// both endpoints — a live start with a stale end would then squiggle from
    /// the start to the end of the buffer.
    pub fn diagnostics(&self, from: &[Diagnostic]) -> Vec<decoration::diagnostic::Diagnostic>;

    /// Converts inlay hints for [`CodeEditor::inlay_hints`].
    ///
    /// **Not total**, and the one place this module drops anything: a hint
    /// whose flattened label is empty is left out. `LabelParts(vec![])` is
    /// legal, and the widget sizes a chip from its label plus padding, guarding
    /// only on the list being empty — so a zero-width label paints an opaque
    /// box over the code *and* shifts the next chip along that row. The drop is
    /// about the label, never the position.
    ///
    /// Order is preserved and the result is **never sorted**: the widget
    /// already sorts by shaped geometry (`widget.rs:806`), which handles
    /// wrapped rows and is strictly better than sorting by logical position.
    pub fn hints(&self, from: &[Hint]) -> Vec<decoration::inlay::Hint<'static>>;
}
```

Both use the single-pass batch walk from Phase 2, and **this phase removes its
`#[allow(dead_code)]`**.

`Hint<'static>` is right and worth a comment where it is returned: the labels
are owned `String`s, so `Cow::Owned` gives `'static`, which coerces into the
widget's `&'a [Hint<'a>]` by covariance. An attempt to "optimize" it into a
borrow hits E0597, because the application cannot hold a self-referential state
struct.

## Step 4 — `tests/lsp.rs`

Ships **here**, not at the end. It is what proves the converted types satisfy
the widget's `&'a [Hint<'a>]` bound, and that is the kind of thing that fails at
integration time or not at all.

Follow `tests/behaviour.rs`: `#![cfg(feature = "lsp")]`, a local
`#[derive(Debug, Clone)] enum Message { Edit(Action) }`, an `editor(..)` helper
taking the decoration slices, `iced_test::simulator`, and sentence-named tests.

## Verification

```sh
cargo test --features lsp
cargo clippy --all-targets --features lsp -- -D warnings
cargo clippy --all-targets -- -D warnings
```

## Spot checks

| input | expected |
|---|---|
| 5 diagnostics, one naming line 9999 | `diagnostics()` returns **5**; the stale one draws nothing |
| a diagnostic with `severity: None` | `Severity::Error` |
| a diagnostic with `severity: 99` | `Severity::Error` |
| a range with `end < start` | normalized by `TextRange::new`; draws forward |
| `InlayHintLabel::LabelParts(["a", "b"])` | one hint, label `"ab"` |
| `InlayHintLabel::LabelParts([])` | **dropped** |
| `InlayHintLabel::String("")` | **dropped** |
| 3 hints given out of positional order | 3 hints, **in the given order** |
| `Code::Number(42)` round-tripped | stays `Number(42)`, never `Text("42")` |

Mutation tests worth writing: break the empty-label drop and a named test must
fail; make `hints()` sort and a named test must fail.

## What NOT to change

- **`widget.rs`, `geometry.rs`, `decoration/`.** The whole point is that the
  bridge produces what the existing builders already take.
- **No sorting of hints.** Ever.
- **No dropping of diagnostics.** `diagnostics()` is total.
- **Do not add padding to the drawn chip.** The fields are carried, not applied.
- **No `apply` yet** — Phase 5 — even though `Hint.text_edits` names it.
