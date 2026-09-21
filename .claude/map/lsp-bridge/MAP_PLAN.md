# MAP — An LSP bridge behind an optional feature

A feature-gated translation layer that turns language-server messages into
matcha's own types, converts between LSP's position encodings and the editor's
UTF-8 byte offsets, and applies single-document text edits to a `Content`.

Branch: `lsp_bridge`, cut from `master` at `2533bf9`. Nothing committed yet.

**Read the *Divergences* section before implementing any phase.** Five rules
differ between the decoration path and the edit path, on purpose. Applying the
wrong one silently corrupts the user's file, and every one of them was a bug in
an earlier draft of this plan.

---

## Current state

matcha is a single library crate, 5115 lines, one dependency (git-pinned iced),
no `[features]` table, and no `#[cfg]` anywhere but six `#[cfg(test)]`. It is
green: 69 unit + 5 behaviour + 2 doctests pass, 4 snapshot tests are `#[ignore]`d
because no baselines exist yet, clippy `--all-targets -D warnings` is clean.

What the bridge has to work with:

- **`Position` is iced's**, re-exported at `src/code_editor.rs:24`. Fields are
  `line: usize` and `index: usize` — a **byte** offset into the line's text,
  excluding the line ending. It derives `Ord` lexicographically.
- **`TextRange` has private fields** and one constructor, `TextRange::new`,
  which orders its endpoints (`decoration.rs:17-27`).
- **`decoration::diagnostic::Severity`** already exists and is exactly
  `Error | Warning | Information | Hint` (`decoration/diagnostic.rs:8-19`),
  zero-dependency and public. **The bridge reuses it rather than defining a
  second copy.**
- **`decoration::diagnostic::Diagnostic` is `{ range, severity }` only** — no
  message. Anything else a server sends stays on `lsp::Diagnostic`.
- **Decorations are borrowed slices** replaced wholesale each frame
  (`widget.rs:241,272`).
- **The widget already sorts hints by shaped geometry** (`widget.rs:806`), which
  handles wrapped rows. The bridge must **not** sort them.
- **`range_fragments` widens a zero-width range** to `MIN_FRAGMENT_WIDTH` and
  names "what an LSP 'insert here' looks like" as the reason
  (`geometry.rs:56-80`). The bridge must not reimplement this — and must not
  hand it a range it did not mean.
- **Nothing in the crate converts a byte offset into anything else.** The
  encoding layer is entirely new code.
- **`Content` keeps its editor `pub(super)`** (`content.rs:17`) and exposes no
  `buffer()`. Its text accessor is `line(&self, index) -> Option<String>`.

### The three documents that must change

| where | says |
|---|---|
| `src/lib.rs:71-80` | "matcha has no `lsp-types` dependency and no opinion about the protocol" |
| `src/lib.rs:102-110` | "A decoration naming a line the buffer does not have … is silently not drawn" — still true, and the bridge must keep it true |
| `README.md:98-99` | Under *Not in scope*: "The LSP protocol, including UTF-16 ↔ UTF-8 conversion" |

---

## Target state

```
lsp_types::Diagnostic      ──┐  feature: lsp-types      ┌─▶ decoration::* (draw)
                             ├──▶  lsp::Diagnostic ─────┤
gen_lsp_types::Diagnostic  ──┘  feature: gen-lsp-types  └─▶ Content::apply (write)
       both directions              matcha's own,            via lsp::Bridge
                                    zero dependencies
                                    feature: lsp
```

Three features, each additive:

```toml
[features]
lsp           = []
lsp-types     = ["lsp", "dep:lsp-types"]
gen-lsp-types = ["lsp", "dep:gen-lsp-types"]

[dependencies]
lsp-types     = { version = ">=0.95, <0.98", optional = true }
gen-lsp-types = { version = ">=0.9,  <0.12", optional = true }

[package.metadata.docs.rs]
all-features = true
```

**Why permissive ranges rather than pins.** The Rust LSP type ecosystem is split
and neither faction is safe to pin to. `lsp-types` has been frozen since
2024-06-04 at 0.97, and `async-lsp` — the obvious transport for an iced app —
pins `^0.95`. Cargo treats a `0.x` minor as a major, so `^0.95` and `^0.97` are
disjoint: two copies in the graph, and the app's `Diagnostic` is a different
type from matcha's. Meanwhile `gen-lsp-types` went 0.5.0 → 0.11.0 in five
months. A range lets Cargo unify on whatever the app already resolved.

Two honest caveats for the docs. **The range unifies only if the graph resolves
a single `lsp-types`.** An app depending on both `async-lsp @ ^0.95` *and*
`lsp-types = "0.97"` gets two copies regardless; matcha binds to one and its
conversions silently do not apply to the other's types. Promise unification of
*matcha with the app's existing copy*, not unification in general. And
`default-features = false` would be a no-op — `lsp-types` declares `default = []`
and `gen-lsp-types` declares no `default` key — so it is omitted rather than
written as decoration.

### Module placement

`src/code_editor/lsp.rs` + `src/code_editor/lsp/`, re-exported as `matcha::lsp`
— the same shape `decoration` uses. The placement is load-bearing: a module
under `code_editor` reaches `Content`'s `pub(super)` field, so the bridge borrows
line text with **zero allocation** and needs no new accessor. Verified.

`impl Content` blocks may live in `lsp/`, and do: `Content::lsp`,
`Content::apply` and `Content::revision` are all declared there. This keeps the
crate's first `#[cfg(feature)]` out of `content.rs` entirely.

### Public surface

This block is the artefact a phase agent copies. It is generated from the phase
text, not maintained beside it. **Every `lsp::*` type derives at least
`Debug, Clone, PartialEq`** — the phases' exit tests assert on them — plus `Copy`
where it is free (`Position`, `Range`, `Encoding`) and `Eq, Hash` where there is
no float. Derives appear below only where the choice is load-bearing — if the two ever disagree, the phases win and
this block is regenerated.

```rust
// ---- coordinates (Phase 1) ----
#[derive(..., PartialOrd, Ord)]          // line declared BEFORE character,
pub struct Position { pub line: u32, pub character: u32 }   // so Ord sorts by line
pub struct Range { pub start: Position, pub end: Position }

impl Range {
    /// `start = end`. Unconditional -- it does NOT test for a reversed range,
    /// so calling it on a well-formed one destroys that range. The caller
    /// checks `end < start` first; `Error::ReversedRange` names the edit.
    ///
    /// This is the de-facto normalization servers target (VS Code caps start to
    /// end), and deliberately NOT a swap.
    pub fn collapsed(self) -> Self;
}

#[derive(Default)]
pub enum Encoding { Utf8, #[default] Utf16, Utf32 }

// ---- the bridge, and Replacement (Phase 2) ----
pub struct Bridge<'a> { /* &'a Content, Encoding */ }

impl Content {
    pub fn lsp(&self, encoding: Encoding) -> Bridge<'_>;               // Phase 2

    /// Bumped by every edit -- `perform(Action::Edit(_))`, including `Undo`
    /// and `Redo`, and `apply`. NOT by `Move`, `Select*`, `Click`, `Drag`,
    /// `Scroll` or `move_to`: the widget publishes every action for the
    /// application to feed back, so bumping on all of them would change the
    /// revision on every mouse-move of a drag-select.
    ///
    /// The app pairs this with the LSP version it last sent in `didChange` and
    /// hands it back to `apply`. Meaningful only within one `Content`; a clone
    /// starts again at 0.
    pub fn revision(&self) -> u64;                                     // Phase 3
}

/// Why a position did not resolve. Internal, because `apply` is the only
/// caller that needs the reason and it turns each one into an `Error` variant
/// carrying the offending edit's index.
///
/// Each variant carries what `clamp` needs to recover AND what `Error` needs to
/// report, so there is exactly one scan. `NotACharBoundary` carries the rounded
/// index precisely so `clamp` does not have to re-scan -- without it, "one
/// converter, three faces" would quietly become two converters.
#[allow(dead_code)] // `lines` is read only by `Content::apply`, in Phase 5
pub(crate) enum Reason {
    /// The line is past the end of the buffer. Carries the buffer's line count
    /// for `Error::LineOutOfBounds`.
    Line { lines: usize },
    /// The column is past the end of its line. Carries the line's byte length,
    /// which is also what `clamp` clamps to.
    Column { line_len: usize },
    /// The column landed inside a character. Carries that character's start,
    /// which is what `clamp` rounds down to.
    NotACharBoundary { floor: usize },
}

impl<'a> Bridge<'a> {
    /// The real converter. Everything else is derived from it, which is what
    /// keeps the two policies in one place.
    pub(crate) fn resolve(&self, from: Position) -> Result<crate::Position, Reason>;

    /// Total. Clamps the column, and clamps `line == line_count` to the end of
    /// the last line. A line BEYOND that is returned as-is with **index 0**,
    /// so the widget drops it as it already documents. ("Unconverted" refers to
    /// the line number, not the column -- never carry `character` across.) A mid-character column rounds
    /// DOWN to that character's start.
    pub fn clamp(&self, from: Position) -> crate::Position;

    /// Fallible. `None` for a line past `line_count`, a column past the end of
    /// its line, or a column off a char boundary.
    ///
    /// `line == line_count` still clamps to the end of the last line -- it is
    /// the LSP end-of-document idiom, not an error, and returning `None` here
    /// would no-op every whole-file format. This is what `apply` uses, through
    /// `resolve`.
    pub fn exact(&self, from: Position) -> Option<crate::Position>;
    /// Editor position -> LSP. Narrows usize to u32 with `try_from`, never `as`.
    pub fn locate(&self, at: crate::Position) -> Option<Position>;

    /// `None` when EITHER endpoint names a line past the end of the buffer.
    /// Not two independent `clamp` calls: a range with a live start and a
    /// stale end would otherwise squiggle from the start to EOF.
    pub fn range(&self, from: Range) -> Option<decoration::TextRange>;
    pub fn locate_range(&self, at: decoration::TextRange) -> Option<Range>;

    // ---- decorations (Phase 4) ----
    /// Total: output length always equals input length.
    pub fn diagnostics(&self, from: &[Diagnostic]) -> Vec<decoration::diagnostic::Diagnostic>;
    /// NOT total: a hint whose flattened label is empty is dropped, because the
    /// widget would paint an opaque zero-width chip for it. Order is preserved;
    /// indices are not. Never sorted -- `widget.rs:806` already sorts by geometry.
    pub fn hints(&self, from: &[Hint]) -> Vec<decoration::inlay::Hint<'static>>;
}

// ---- decoration payloads (Phase 4) ----
pub struct Diagnostic {
    pub range: Range,
    /// Absent and out-of-range both become `Error`; see Phase 4.
    pub severity: decoration::diagnostic::Severity,    // the EXISTING type
    pub message: String,
    pub source: Option<String>,
    pub code: Option<Code>,
    /// A URL documenting the code. A URI on the wire
    /// (`CodeDescription { href }`), so it is re-parsed on the way out --
    /// which puts `Diagnostic` on the fallible-outbound list.
    pub code_description: Option<String>,
    pub tags: Vec<Tag>,
    pub related: Vec<Related>,
    /// Opaque server JSON, echoed back in `CodeActionContext`. Never parsed
    /// here, which is why the core needs no serde. The round trip is semantic,
    /// not byte-exact -- key order may differ, and every server compares
    /// semantically.
    pub data: Option<String>,
}
pub enum Code { Number(i32), Text(String) }    // NumberOrString, losslessly
pub enum Tag { Unnecessary, Deprecated }       // carried, never drawn
/// Kept whole. matcha has no notion of a file, so it cannot tell "this buffer"
/// from "another file" -- the app owns that decision.
pub struct Related { pub uri: String, pub range: Range, pub message: String }

pub struct Hint {
    pub position: Position,
    pub label: String,              // label parts flattened; see Phase 4
    pub kind: Option<hint::Kind>,
    pub padding_left: bool,
    pub padding_right: bool,
    pub tooltip: Option<String>,
    /// "Accept this hint" -- turns the chip into real text via `Content::apply`.
    pub text_edits: Vec<Replacement>,
    pub data: Option<String>,
}

// ---- edits (Phase 5; caret restoration is Phase 6) ----
/// Ships in Phase 2, not with `apply`: Phase 4's `Hint.text_edits` needs it,
/// and making Phase 4 wait on Phase 5 would serialize two phases that are
/// otherwise independent.
pub struct Replacement {
    pub range: Range,
    pub new_text: String,
    pub annotation_id: Option<String>,
}
pub mod document {
    pub struct Edit {
        pub uri: String,
        /// `Some` means the server computed this against that version.
        pub version: Option<i32>,
        pub edits: Vec<super::Change>,
    }
}
/// A `TextDocumentEdit`'s element. `gen-lsp-types` has a third case matcha
/// cannot honour, and it is represented rather than dropped: applying a snippet
/// as literal text would insert `$0` into the user's buffer.
#[non_exhaustive]
pub enum Change { Replace(Replacement), Snippet(Snippet) }

/// A `SnippetTextEdit`. Carried so an app can see it and act; `Content::apply`
/// refuses a batch containing one with `Error::Unsupported`.
pub struct Snippet {
    pub range: Range,
    /// Snippet syntax, NOT literal text.
    pub value: String,
    pub annotation_id: Option<String>,
}

impl Content {
    /// Applies one document's edits. All-or-nothing, and refuses if the buffer
    /// moved since `expected`.
    pub fn apply(
        &mut self,
        edit: &document::Edit,
        encoding: Encoding,
        expected: u64,
    ) -> Result<u64, Error>;
}

#[non_exhaustive]
pub enum Error {
    Stale { expected: u64, actual: u64 },
    LineOutOfBounds { edit: usize, line: u32, lines: usize },
    ColumnOutOfBounds { edit: usize, character: u32, line_len: usize },
    NotACharBoundary { edit: usize, position: Position },
    Overlapping { edit: usize, other: usize },
    ReversedRange { edit: usize },
    /// A `Change::Snippet`, which matcha cannot apply.
    Unsupported { edit: usize },
}

// ---- workspace (Phase 7) ----
pub mod workspace {
    /// Normalized at construction, in the **inbound** `From` impls (Phases 9
    /// and 11) -- NOT a mirror of the wire shape. Inbound is `From`; `TryFrom`
    /// is the outbound direction and does the reverse. `document_changes` supersedes `changes`; when only
    /// `changes` is present its entries are synthesized with `version: None`.
    /// Doing this once, in one place, is what gives the precedence rule an
    /// owner, and it is why the accessors can hand out borrows.
    #[non_exhaustive]
    pub struct Edit {
        steps: Vec<Step>,                        // wire order, interleaved
        annotations: HashMap<String, Annotation>,
    }

    /// Edits and resource operations in one ordered sequence, because the
    /// interleaving is load-bearing: create A, edit A, rename A→B, edit B is a
    /// normal rename refactor, and two flat accessors would erase the order
    /// between them.
    #[non_exhaustive]
    pub enum Step { Document(super::document::Edit), Operation(Operation) }

    impl Edit {
        /// Builds one from already-normalized parts.
        ///
        /// `pub(crate)`, not `pub`: the fields are private so the accessors can
        /// hand out borrows, and Rust field privacy is module-scoped, so
        /// `lsp::from_lsp_types` -- a *sibling* module -- cannot build one
        /// without this. Phases 9 and 11 are its only callers.
        pub(crate) fn new(steps: Vec<Step>, annotations: HashMap<String, Annotation>) -> Self;

        /// Every step, in wire order. The primary accessor.
        pub fn steps(&self) -> &[Step];
        /// Just the document edits, for an app that has no other files open.
        pub fn document_edits(&self) -> impl Iterator<Item = &super::document::Edit>;
        /// Resolves a `Replacement::annotation_id`. Without this the id is
        /// carried and unusable, and `needs_confirmation` unreachable.
        pub fn annotation(&self, id: &str) -> Option<&Annotation>;
    }

    #[non_exhaustive]
    pub enum Operation { Create(Create), Rename(Rename), Delete(Delete) }

    // Fixed here rather than invented by a phase agent: these shapes are public
    // API that Phases 9 and 11 must round-trip.
    pub struct Create { pub uri: String, pub overwrite: bool,
                        pub ignore_if_exists: bool, pub annotation_id: Option<String> }
    pub struct Rename { pub old_uri: String, pub new_uri: String, pub overwrite: bool,
                        pub ignore_if_exists: bool, pub annotation_id: Option<String> }
    pub struct Delete { pub uri: String, pub recursive: bool,
                        pub ignore_if_not_exists: bool, pub annotation_id: Option<String> }

    pub struct Annotation {
        pub label: String,
        /// The app must prompt before applying. matcha never prompts.
        pub needs_confirmation: bool,
        pub description: Option<String>,
    }
}

// ---- actions and the message envelope (Phase 8) ----
pub struct CodeAction {
    pub title: String,
    /// Matched hierarchically: `refactor.extract` matches `refactor`,
    /// `refactory` does not.
    pub kind: Option<String>,
    /// The diagnostics this action addresses, echoed back to resolve it.
    pub diagnostics: Vec<Diagnostic>,
    pub edit: Option<workspace::Edit>,
    /// If both this and `edit` are present, the edit applies first.
    pub command: Option<Command>,
    pub is_preferred: bool,
    /// `Some(reason)` means the server offers it but it cannot run now. The app
    /// decides whether to hide or grey out; the reason is what a user reads.
    pub disabled: Option<String>,
    pub data: Option<String>,
}
pub struct Command {
    pub title: String,
    pub command: String,
    /// One raw JSON value per argument, so the app can rebuild
    /// `workspace/executeCommand` without matcha parsing anything.
    pub arguments: Vec<String>,
}
/// `textDocument/codeAction` answers with a union of both.
#[non_exhaustive]
pub enum Offer { Action(CodeAction), Command(Command) }

// ---- outbound failure (Phases 10, 12) ----
/// Why an outbound conversion failed.
///
/// Outbound is `TryFrom` for two unrelated reasons, so one error carries both
/// rather than either lying about the other.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundError {
    /// A URI the target crate would not parse. See [`uri::Error`].
    Uri(uri::Error),
    /// A [`Change::Snippet`] converted to `lsp-types`, which has no
    /// `SnippetTextEdit`. `gen-lsp-types` does, so this cannot arise there.
    Snippet,
}

pub mod uri {
    /// A URI that a language-server crate would not accept.
    ///
    /// Outbound conversion has to build a `Url` (lsp-types 0.95) or a `Uri`
    /// (0.96+) by parsing, and both can refuse. The parse errors differ between
    /// those types, and the types themselves can never be named here, so the
    /// reason is erased. Named `uri::Error` rather than `UriError` for the same
    /// reason every other type here is: the module path carries the noun.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Error;
}

// ---- per-family conversion modules (Phases 9-12) ----
//
// Namespaced, because two free functions cannot both be
// `matcha::lsp::client_capabilities` and `--all-features` builds both.
#[cfg(feature = "lsp-types")]
pub mod from_lsp_types {
    /// Exactly what this bridge understands, prefilled. Every optional field
    /// matcha models arrives ONLY if the app advertised it.
    pub fn client_capabilities() -> lsp_types::ClientCapabilities;
}
#[cfg(feature = "gen-lsp-types")]
pub mod from_gen_lsp_types {
    pub fn client_capabilities() -> gen_lsp_types::ClientCapabilities;
}

/// Something a language server sent, with the envelope that makes it routable.
#[non_exhaustive]
pub enum Message {
    Diagnostics { uri: String, version: Option<i32>, diagnostics: Vec<Diagnostic> },
    Hints { hints: Vec<Hint> },
    Edit { label: Option<String>, edit: workspace::Edit },
    Offers(Vec<Offer>),
}
```

### Names map to the spec through a table, not through spelling

The project forbids composite type names. Applied uniformly:

| matcha | LSP |
|---|---|
| `lsp::Hint` | `InlayHint` |
| `lsp::hint::Kind` | `InlayHintKind` |
| `lsp::Replacement` | `TextEdit` |
| `lsp::Change` | the `TextEdit \| AnnotatedTextEdit \| SnippetTextEdit` union |
| `lsp::diagnostic::Code` | `NumberOrString` |
| `lsp::diagnostic::Tag` | `DiagnosticTag` |
| `lsp::diagnostic::Related` | `DiagnosticRelatedInformation` |
| `lsp::Offer` | `CodeActionOrCommand` |
| `lsp::document::Edit` | `TextDocumentEdit` |
| `lsp::workspace::Edit` | `WorkspaceEdit` |
| `lsp::workspace::Step` | `DocumentChangeOperation` |
| `lsp::workspace::Operation` | `ResourceOp` |
| `lsp::workspace::Annotation` | `ChangeAnnotation` |
| `lsp::workspace::Create` / `Rename` / `Delete` | `CreateFile` / `RenameFile` / `DeleteFile` |
| `lsp::Encoding` | `PositionEncodingKind` |
| `lsp::Snippet` | `SnippetTextEdit` |
| *(reused)* `decoration::diagnostic::Severity` | `DiagnosticSeverity` |

**`Replacement` rather than `Edit` is load-bearing.** `matcha::Edit` already
exists — iced's *keystroke* enum (`Insert(char)`, `Backspace`, `Paste`),
re-exported at `lib.rs:139` — and `Content::perform` takes one while
`Content::apply` takes the other. With `use foo as bar` forbidden, a collision
there would be permanent.

**`CodeAction` keeps its composite name**, because `matcha::Action` exists and
means iced's editor action. `lsp::Action` would be a genuine trap.

**`lsp::Position` deliberately shadows `crate::Position`.** They must stay
distinct types: that is what makes "came off the wire" unusable as an index
until it has been through `clamp` or `exact` — zed buys the same guarantee with
an `Unclipped<T>` newtype (`crates/language/src/language.rs:1668-1670`). Nothing
else in the crate may convert between them.

---

## Divergences — the five rules that differ by consumer

Read this before any phase. A decoration that is slightly wrong is a cosmetic
glitch the next publish fixes. An edit that is slightly wrong is the user's file,
changed in a way they did not ask for and may not notice. So the same input gets
different treatment depending on where it is going, and every one of these was
a bug in an earlier draft.

| | decoration path (`Bridge::clamp`, batch methods) | edit path (`Content::apply`) |
|---|---|---|
| column past end of line | clamp (spec-mandated) | `Err(ColumnOutOfBounds)` — clamping turns "replace 10..20" into a no-op insert |
| column off a char boundary | round **down** to the char's start | `Err(NotACharBoundary)` — rounding silently widens the replaced range |
| `line == line_count` | clamp to end of last line | clamp to end of last line (the EOF idiom) — **identical on both sides, listed because the rows around it invite the opposite inference** |
| `line > line_count` | **pass through unconverted**, so the widget drops it | `Err(LineOutOfBounds)` |
| reversed range (`end < start`) | `TextRange::new` swaps, harmlessly | `Err(ReversedRange)` |

Two of those need their reasoning written down, because both look wrong at first.

### Why an out-of-range line passes through rather than clamping

An earlier draft clamped every out-of-range line to the end of the document.
That is worse than dropping, not better: `range_fragments` widens a zero-width
range to `MIN_FRAGMENT_WIDTH` so an LSP "insert here" is visible
(`geometry.rs:56-80`), so a diagnostic clamped from line 4000 of a 300-line file
paints a **squiggle at the end of the last line, under text it has nothing to do
with**. It also breaks the invariant `lib.rs:102-110` states outright: *"A
decoration naming a line the buffer does not have … is silently not drawn."*

Passing the line through unconverted keeps both promises. `range_fragments`
filters runs to `[start.line, end.line]` (`geometry.rs:51-56`) and
`position_anchor` returns `None` for a line the buffer lacks
(`geometry.rs:100-112`), so it draws nothing, silently, with no special case in
the bridge.

**A range is dropped if *either* endpoint's line is past `line_count`.** The
pass-through rule is stated per position, and per position it is right — but a
range with a valid `start` and an out-of-range `end` is not two independent
positions. `range_fragments` filters runs to `[start.line, end.line]`
(`geometry.rs:51-56`), so every run from `start` onwards survives the filter and
`highlight` reports each one fully selected: a full-width squiggle from `start`
to EOF. That is the same false squiggle clamping would have produced. So
`Bridge::range` checks both endpoints and yields nothing when either is beyond
the buffer.

`line == line_count` is different and must still clamp: it is the canonical LSP
spelling of *end of document*. A whole-file `textDocument/formatting` response is
one edit over `0:0 .. lineCount:0`, and rust-analyzer, gopls and tsserver all
emit it. Dropping that would no-op every format-on-save. helix clamps it for
exactly this reason (`helix-lsp/src/lib.rs:151-158`, tested at `:1052-1063`).

**Clamping an out-of-range line is convergent client behaviour, not spec.**
LSP 3.17 mandates the clamp for `character` only. Say so rather than implying
otherwise.

### Why a reversed range is rejected for edits, and what the app should do

This is a deliberate divergence from de-facto client behaviour, and the plan
says so rather than presenting it as plain correctness.

helix's *decoration* path collapses (`lsp_range_to_range`, `lib.rs:270-280`); its
*edit* path **discards** the edit outright (`generate_transaction_from_edits`,
`lib.rs:453-456`: `log::error!("Invalid LSP text edit start > end, discarding")`).
So helix's edit policy is already closer to reject than to repair.

The recovery servers expect is **`start = end`** — VS Code's semantics, which
helix's comment names the TS server as relying on. It is an *insert at `end`*.
It is **not a swap**: swapping deletes everything between two endpoints the
server meant to collapse. So the plan ships `Range::collapsed()` rather than
telling the app to "normalize", which would invite exactly that swap.

---

## Constraints

1. **The widget gains no field and no code path.** `widget.rs` and `geometry.rs`
   are not touched. `highlight_with` (`widget.rs:294-317`) assigns 22 fields,
   19 copied from `self`, and adding one would mean threading it there.
   **`Content` is a different matter and *may* gain fields** — it gains a
   revision counter in Phase 3. The ban is on the widget, not on the crate.
2. **`#![warn(missing_docs)]` + `clippy --all-targets -D warnings`** means every
   public item needs a doc *inside* its `cfg`, including every enum variant and
   every struct field. A default-features build will not catch a missing doc
   behind `#[cfg(feature = "lsp")]`, so the lint runs per feature combination.
3. **No `mod.rs`** — `lsp.rs` + `lsp/`.
4. **No composite type names**, applied uniformly per the table above. The one
   exception is `lsp::CodeAction`, because `matcha::Action` exists.
5. **No `use foo as bar`.** `lsp::Position` and `crate::Position` are
   disambiguated by path, as are `lsp::Message` and `gen_lsp_types::Message`.
6. **No `unwrap()` in library code**; `expect` with a reason. Note
   `Content::line_ending()` returns `Option<LineEnding>`, so there are two
   `None`s to handle on the normalization path.
7. **The iced rev is the contract.** No `cargo update`, no touching the pin —
   with one carve-out, because Phases 9-12 cannot otherwise meet their exit
   criteria. `Cargo.lock` is tracked and a range resolves to one version, so
   checking the endpoints requires
   `cargo update -p lsp-types --precise <0.95.0|0.96.0|0.97.0>` and
   `cargo update -p gen-lsp-types --precise <0.9.0|0.10.0|0.11.0>`. Those two
   package names and those six versions are permitted, and each **must be
   followed by `git checkout -- Cargo.lock`**. The iced rev is never touched.
8. **`Content::apply` must not hold a `Bridge`, or any `Ref`, across its own
   `self.0.borrow_mut()`.** `RefCell::borrow_mut` takes `&self`, so the borrow
   checker permits the overlap and the failure is a runtime panic. External
   callers are *not* at risk — `Bridge<'a>` borrows `Content`, so
   `content.perform(..)` while a bridge is alive is a compile error (E0502).
   **The hazard is confined to crate-internal code that reaches the `RefCell`
   directly, which is `Content::apply` itself.** Validate through a borrow,
   drop it, then commit.
9. **No AI attribution** in commits, docs, or any public content.
10. **Test names read as sentences**, no `test_` prefix, article or gerund opener.
11. **Never run `cargo test -- --ignored`.** Snapshot baselines auto-create on
    first run and return `true` (`iced/test/src/simulator.rs:261-296`), so a
    stray run bakes in an unreviewed baseline. Also never `cargo run --example`
    — it opens a window and hangs a headless session.

---

## Phases

Thirteen, each sized for one subagent and each leaving the tree green.

### Phase 1 — Feature scaffold — **DONE**

*Landed as planned; the code in `MAP_PHASE_1.md` compiled unchanged. Two things
worth carrying forward: the feature gate lands in exactly two places
(`code_editor.rs` at the `pub mod` and `lib.rs` at the re-export), so `lsp.rs`
itself contains no `cfg` at all and later phases should keep it that way; and
`cargo test` without the feature still reports 69/5/2 with 4 ignored, which is
the invariant every later phase has to preserve.*

`[features] lsp = []` only — each conversion phase adds its own feature, its own
optional dependency and the `docs.rs` metadata. The module at
`src/code_editor/lsp.rs`, re-exported as `matcha::lsp`. `Position` (deriving
`PartialEq, Eq, PartialOrd, Ord` with `line` declared first), `Range`,
`Range::collapsed`, `Encoding` (deriving `Clone, Copy, Default`).
**Exit:** builds and lints clean with and without `--features lsp`.

### Phase 2 — Encoding conversion, the `Bridge`, and `Replacement` — **DONE**

*Two deviations, both removing a planned `allow(dead_code)` rather than
narrowing one. **The batch walk moved to Phase 4**, whose `diagnostics()` and
`hints()` are its only callers — shipping it here meant shipping code nothing
calls. **`Reason::Line` became a unit variant**: nothing here reads a `lines`
payload, and Phase 5's `apply` holds `&mut self` so it can ask `line_count()`
itself. `Reason` is `pub(crate)`, so adding the field back costs nothing. Phase 2
therefore carries no suppressions at all.*

*Mutation testing earned its place. Seven mutations, six caught immediately —
and **the ASCII fast path's `<=` survived**, because no test covered a column
exactly at the end of an ASCII line. `clamp` cannot see the difference there;
only `exact` can, and that column is what an edit appending to a line names.
`the_column_at_the_end_of_a_line_is_exact` closes it, and all seven now fail by
name.*

The correctness core and the first public surface. Ships `Content::lsp(encoding)`
— the constructor, an `impl Content` block living in `lsp/bridge.rs` rather than
in `content.rs`, which keeps the crate's first feature gate out of a core file —
and **all five** conversion methods: `clamp`, `exact`, `locate`, `range` and `locate_range`. The two range
methods are easy to drop because Phase 4's batch methods look like they subsume
them — they do not. An app needs `range` to find the diagnostic under the cursor
and `locate_range` to fill `CodeActionParams.range`, and neither is reachable
through the batch methods.

- **One converter, three faces.** `resolve` is the real one and returns
  `Result<crate::Position, Reason>`. `clamp` and `exact` are derived from it.
  `Reason` is `pub(crate)`: `apply` is its only consumer and it needs the reason
  to build an `Error` variant — without it `apply` would re-validate by hand,
  duplicating what "one converter" exists to prevent.
- `clamp` rounds a mid-character column **down**; `exact` refuses. Both are
  needed; see *Divergences*.
- **`line == line_count` clamps on both paths.** It is the end-of-document
  idiom, not an error, and `exact` returning `None` there would no-op every
  whole-file format. The `character` is ignored on that line.
- **`Utf16` is the `Default`.** An unrecognised `positionEncoding` means UTF-16
  per spec, not an error.
- **`Replacement` ships here**, not with `apply`. It is three plain fields, and
  Phase 4's `Hint.text_edits` needs it — without this, Phase 4 would depend on
  Phase 5 and could not be built in parallel with it.
- **ASCII fast path, per line, not per position.**
- **Batch conversion walks each line once**: sort positions by
  `(line, character)`, scan each line a single time, scatter back through the
  permutation. Calling a per-position converter in a loop is
  O(positions × line length) — one minified bundle, 100 KB on one line, 2000
  diagnostics, is ~800 MB of scanning per publish, and a per-position ASCII
  check makes it *worse* by adding a second scan. Its only callers arrive in
  Phase 4, so it carries `#[allow(dead_code)]` here with a comment naming
  Phase 4 as the remover; `clippy --all-targets -D warnings` would otherwise
  fail this phase's own exit.
- **A persistent line index is rejected**, but not for the reason an earlier
  draft gave. `Bridge<'a>` borrows `Content`, so the text cannot change for the
  bridge's lifetime — **the bridge's lifetime is the invalidation scope**, and a
  cache inside it would need none. It is rejected because the single-pass walk
  already gets the win.

**Exit:** a round-trip over every char boundary of the same strings as the
multibyte fixture in `geometry.rs:316-340` (that fixture is a `#[cfg(test)]`
local and is not reachable from `lsp/`), in all three encodings; a
**mid-character** case per encoding asserting `clamp` rounds down and `exact`
returns `None`; `(u32::MAX, u32::MAX)`; an empty buffer; a line at, and beyond,
`line_count`.

### Phase 3 — Revision counting, and what a converted position means — **DONE**

*Landed as planned. The `pub(super)` on both tuple elements was the predicted
trap and the plan's warning held: the accessor sits in `lsp/bridge.rs`, so a
bare `u64` would have been `error[E0616]`. Five mutations, all caught by name.*

*One correction for later phases: `mouse::click::Kind` is **not** re-exported at
`iced::mouse`, which carries only `Button`, `Cursor`, `Event`, `Interaction` and
`ScrollDelta`. It is at `iced::advanced::mouse::click::Kind`.*

*The revision tests are gated on the feature even though the field and the bump
are not, because `revision()` is the only way to observe either.*

The smallest phase, and the one `apply` depends on for honesty.

`Content` gains a **second tuple element**, `Content(pub(super) RefCell<text::Editor>, pub(super) u64)`.
**Both elements need `pub(super)`**: the accessor lives in `lsp/bridge.rs`, a
*sibling* of `content` rather than a descendant, so a bare `u64` is private to
`content` and `self.1` is `error[E0616]`. Verified by compiling. It must be a
tuple element, not a named field: `Content` is a tuple struct
(`content.rs:17`) and `self.content.0` appears at `widget.rs:443, 499, 606, 930,
1075` plus roughly sixteen test sites, so converting it to a named-field struct
would edit `widget.rs` — which constraint 1 defines as the design having broken.

It is a plain `u64`, **not a `Cell`**: every mutator takes `&mut self`, so
interior mutability buys nothing and would permit a bump through `&Content`
while a `Bridge` is alive. The field and its bump are unconditional;
only `revision()` is `#[cfg(feature = "lsp")]`, which keeps `content.rs` free of
the crate's first feature gate in spirit as well as in letter.

- **Bump iff `action.is_edit()`.** iced ships the predicate
  (`core/src/text/editor.rs:148-152`), and it correctly catches `Undo`/`Redo`.
  Bumping on every `perform` would be wrong: the widget publishes *every* action
  for the app to feed back (`widget.rs:524`), so `Scroll`, `Click`, `Drag`,
  `Move` and `Select*` all arrive there, and the revision would change on every
  mouse-move of a drag-select — making `apply` return `Stale` for a buffer whose
  text never changed.
- **`Clone` resets to 0.** `Content::clone` routes through `with_text`, and
  either choice breaks monotonicity across a clone; resetting is the safe
  direction, because a recorded revision then reads as *stale* rather than
  falsely fresh. `revision()` is meaningful only within one `Content` instance,
  and the doc says so.

The point: **a server's response describes the document at some earlier
version**, and matcha does not own the LSP version counter — the app sends
`didChange`. So the app records `(lsp_version, revision)` when it sends and
hands the revision back when the response arrives. That mapping is the only
thing matcha cannot do for it, which is also why `apply` ignores
`document::Edit.version` entirely.

This phase also writes the documentation that makes the decoration path honest:
**converted positions are a snapshot.** They do not track edits and do not
anchor. Drift between publishes is expected and self-correcting.

**Exit:** `revision()` changes across `perform(Action::Edit(_))`, including
`Undo` and `Redo`; it does **not** change across
`Move`/`Select*`/`Click`/`Drag`/`Scroll`, `move_to`, or any read; a clone starts
at 0. The doc section exists.

`apply` does not exist until Phase 5, so its effect on the counter is **Phase
5's** exit criterion, not this one. (It bumps once per underlying `perform`, so
the counter advances by the number of edits — which is why only equality against
`expected` is meaningful.)

### Phase 4 — Diagnostics and inlay hints — **DONE**

*The batch walk did **not** become a second scanner. `Columns` walks one line
answering columns in the order asked, `Bridge::resolve` asks it once, and the
batch asks it per line — so there is one implementation, not two that must
agree. The property that makes that safe is a test:
`converting_a_batch_answers_exactly_what_converting_one_at_a_time_does`, over
every column of every line of the multibyte fixture, in all three encodings,
in both orders.*

***The sort in `clamp_all` is a pure optimization and cannot be
mutation-tested.*** *Removing it changes no observable behaviour, because
`byte_at` restarts when asked to go backwards. That is the right design — order
cannot affect an answer — but it means the one-pass property is not pinned by
any test, and a correctness test never will pin it. Anyone tempted to "simplify"
the sort away should know it is load-bearing for speed alone.*

*Naming deviation: `Code`, `Tag` and `Related` are `lsp::diagnostic::*` rather
than flat, and `Kind` is `lsp::hint::Kind`. The table above said flat for the
first three. Module-path naming is the crate's rule and `lsp::Tag` does not say
what it tags. `diagnostic` and `hint` are therefore public modules while
`position`, `encoding` and `replacement` stay private: a module goes public
exactly when it has satellites that read badly flat.*

*`Reason` gained `Debug, Clone, Copy, PartialEq, Eq` so tests can assert on it.
It is `pub(crate)`, so this commits to nothing.*

*For the integration tests: the editor takes focus from a click, so `typewrite`
before one produces no messages at all.*

`lsp::Diagnostic`, `lsp::Code`, `lsp::Tag`, `lsp::Related`, `lsp::Hint`,
`lsp::hint::Kind`, and `Bridge`'s two batch methods. Removes Phase 2's
`#[allow(dead_code)]`.

- **Severity reuses `decoration::diagnostic::Severity`.** A second identical
  enum would buy an identity `From` and nothing else.
- **Absent severity means `Error`**, and so does an out-of-range one (`0`, `5`,
  `99` are legal JSON). helix picks `Warning`; zed and VS Code pick `Error`.
  `Error` is right here because the severities differ only by squiggle colour
  and under-reporting a real error is the worse mistake.
- **`data` is opaque JSON text**, never parsed. It must be echoed back in
  `CodeActionContext.diagnostics` or rust-analyzer resolves no quickfix.
- **`tags` are carried and never drawn.** `Unnecessary` is dim and `Deprecated`
  is strikethrough — text-rendering effects that would mean touching
  `widget.rs`. Settled here, not left to the phase.
- **When `Bridge::range` yields `None`, `diagnostics()` still emits an entry**
  — it is total, and the index correspondence is load-bearing. Emit a
  `TextRange` **both of whose endpoints sit on the out-of-range line**, so
  `range_fragments`' own filter (`geometry.rs:51-56`) drops it and nothing
  draws. Do **not** fall back to `TextRange::new(clamp(start), clamp(end))`:
  that is exactly the full-width start-to-EOF squiggle the `None` prevents.
- **`related` is kept whole**, URI and all. matcha has no notion of a file, so
  it *cannot* decide whether a related location points into this buffer.
- **Hint labels are flattened**, deliberately and irreversibly: dropping
  `InlayHintLabelPart.location` gives up ctrl-clickable hints.
- **An empty flattened label is dropped.** `LabelParts(vec![])` is legal, and the widget sizes a chip as
  `min_bounds() + padding` guarding only on `chips.is_empty()`
  (`widget.rs:800-850`), so a zero-width label paints an opaque box over the
  code and shifts the next chip on that row.
- **Hints are never sorted.** `widget.rs:806` already sorts by shaped geometry,
  which handles wrapped rows; sorting here would break index correspondence for
  nothing.
- **`padding_left` / `padding_right` are carried for echo-back and do not change
  the drawn chip.** `decoration::inlay::Hint` is `{ position, label }` only
  (`decoration/inlay.rs:18-24`) and constraint 1 forbids adding a field. The
  widget's own `inlay::Style::offset.x = 2.0` already clears the annotated glyph,
  so the common case reads correctly without them.
- **`text_edits` is carried** — "accept this hint" — using the `Replacement`
  from Phase 2.

**Exit:** `diagnostics()` is total and index-correspondent; `hints()` drops only
empty labels and preserves order; a stale line neither draws nor panics;
`tests/lsp.rs` drives a real widget with converted decorations, which is what
proves `Vec<Hint<'static>>` satisfies the widget's `&'a [Hint<'a>]` bound.

### Phase 5 — Applying edits: validation and commit — **DONE**

*Every measured rule held, and mutation testing found two things the 23 tests
did not.*

***The `Lf` fallback gap.*** *The plan's rule — when the buffer shows no line
ending, do not normalise — was implemented correctly, and the test could not
tell it from the wrong answer. It inserted `"a\nb"` into a fresh buffer, which
`Lf` normalisation maps to itself. The case that distinguishes them is `\r\n`
into a buffer with no convention, where guessing `Lf` silently converts a DOS
file. Test added.*

***`last_end = end.max(last_end)` is dead logic here.*** *The plan took it from
helix. Given the batch is sorted by `(start, end)` and reversed ranges are
already refused, `end < covered` while `start >= covered` implies
`start >= covered > end >= start`. Removed, with the invariant stated instead.*

*A third survivor was a bad mutation rather than a gap: it rewrote the
line-ending walk into something equivalent. Re-run against a naive
`replace('\n', ..)` it is caught by the idempotence test, which is the one that
matters — a server that knows the file is DOS already sends `\r\n`.*

*`Content::apply` scopes the bridge in a block rather than a helper. The block
boundary is constraint 8 made visible: `RefCell::borrow_mut` takes `&self`, so a
bridge still alive at the commit would compile and panic at run time.*

`lsp::Change`, `lsp::Snippet`, `lsp::document::Edit`, `lsp::Error`, and
`Content::apply`. The caret is left where `Edit::Paste` puts it; Phase 6 fixes
that.

- **Sort key is `(start, end)`, stable ascending, iterated in reverse, carrying
  the original index.** Every word earns its place:
  - *`(start, end)` not `start`* — LSP permits "any number of inserts followed
    by a single remove or replace" at one position and says the array need not
    be ordered. Measured on `"XYZ"` with `[replace(0..2,"Q"), insert(0,"A")]`,
    which wants `"AQZ"`: sorting by `start` alone yields **`"QYZ"`** — the
    insert lost, the wrong range replaced — *and* falsely trips the overlap
    guard. `(start, end)` is correct in either array order.
  - *ascending then reversed, not descending* — a stable **descending** sort
    preserves array order among equal keys and therefore **reverses** several
    inserts at one position: `"XY"` + `A`,`B` gives `"XBAY"` not `"XABY"`; with
    three inserts, `"321Z"` not `"123Z"`. Measured.
  - *reversed* — back-to-front keeps earlier offsets valid **and** makes the
    final `perform` the topmost edit, which matters because
    `topmost_line_changed` is overwritten across performs
    (`iced/graphics/src/text/editor.rs:535`) and consumed once per frame
    (`:753-756`). Ascending leaves syntax highlighting stale above the last edit.
  - *original index* — solely so `Error` can name the failing edit by its
    position in the caller's array. The stable sort already handles ordering.
- **Conversion goes through `Bridge::resolve`**, and each `Reason` plus the
  edit's original index becomes an `Error` variant.
- **The overlap check runs in byte coordinates, after conversion**, with strict
  `<` and `last_end = end.max(last_end)`. `<=` would reject the legal
  several-inserts-at-one-position case (`helix-lsp/src/lib.rs:458`).
- **Line-ending normalization: split `new_text` on `\r\n | \n | \r` and rejoin
  with the target.** Idempotent by construction, which is what matters —
  rust-analyzer already emits `\r\n` for a DOS file (`to_proto.rs:175-182`), so
  "already CRLF" is the common path and a naive `replace('\n', "\r\n")` gives
  `\r\r\n`.
- **When the target ending is unknown, do not normalize at all.**
  `Content::line_ending()` reads line 0 (`content.rs:93-95`) and returns
  `Option<LineEnding>`, so there are two `None`s to handle and constraint 6
  forbids `unwrap`. A single-line buffer — **including `Content::new()`** —
  reports `LineEnding::None`, whose `as_str()` is `""`; normalizing against that
  deletes every newline (`"a\nb"` → `"ab"`, measured). Defaulting to `Lf`
  instead is *also* wrong: a one-line buffer carries no evidence of the file's
  convention, so an edit from a DOS file would have its `\r\n` rewritten.
  Insert verbatim and let cosmic-text's `LineIter` record what arrives.
- **`apply` refuses a stale buffer** with `Err(Stale { expected, actual })`.
- **A `Change::Snippet` fails the batch** with `Err(Unsupported { edit })`.
  Applying snippet syntax as literal text would insert `$0` into the buffer.
- **`apply` with an empty `edits` vector is `Ok`, a no-op, pushing no undo
  entry** (`finish_change` only pushes non-empty items, `editor.rs:598-602`).
- **One undo step per edit.** A 50-edit batch is 50 Ctrl-Z presses; iced's
  `History` is private, so this is documented, not fixed.
- **`Editor::overwrite` is a trap.** One reshape, but no `topmost_line_changed`
  (stale colours) and no recorded `Change`, so a later undo runs `delete_range`
  with cursors into the old text.

**Exit:** the case table passes against the real editor — several inserts at one
position; `[replace, insert]` in both array orders; a **pure deletion**
(`new_text == ""`, which works only because `insert_string` calls
`delete_selection()` before `insert_at`'s empty-data early return —
load-bearing and otherwise untested); **multi-line `new_text`**; already-CRLF
`new_text`; a single-line and a `Content::new()` buffer; a reversed range; a
column past end-of-line; a mid-character column; a mid-grapheme char boundary
inside the 25-byte ZWJ family emoji; an edit at EOF; a stale revision; a successful batch advancing `revision()` by its edit count; a
snippet.

### Phase 6 — Applying edits: caret and selection

Self-contained arithmetic over the list Phase 5 already builds, with its own
tests. Not optional, and **not** the whole-document diff that is out of scope:
that preserves anchors across an arbitrary rewrite; this restores one caret and
one anchor.

Without it the commonest batch — a whole-document format — lands the caret at
EOF, scrolls the viewport to the bottom and destroys the selection, because
`Edit::Paste` sets the cursor to the end of inserted text and `Action::Edit`
ends in `shape_until_cursor`.

**Read `cursor()` once, before the first `move_to`** — `apply`'s own `move_to`
destroys it — and adjust per edit in the same reverse order, which composes
because the edits are disjoint and each adjustment uses only its own edit's
geometry.

#### Three definitions that change the answer

Each of these was wrong or missing in the first draft of this phase, and each
is invisible in the case the phase is named for.

- **Tie-break: a caret exactly at an insertion point ends up *after* the
  inserted text.** VS Code's behaviour, and what "accept this inlay hint" should
  feel like. It is a convention rather than arithmetic, so it is named here
  instead of being derived.
- **"Line count" means `split('\n').count()`, not `.lines().count()`.** `"x\n"`
  is **two** lines, not one. A formatter that adds a trailing newline is the
  common case, and `.lines()` would mis-shift every caret below the edit.
- **The line delta is signed.** A reformat that shrinks the file makes it
  negative; computed in `usize` it panics in debug and wraps in release.

#### The rule

Let `new_lines = new_text.split('\n')`, `n = new_lines.len()`, and

```text
end_col = if n == 1 { start.index + new_text.len() } else { new_lines[n-1].len() }
```

Three branches, with the comparisons stated exactly, because this is where the
first draft went wrong:

1. **`caret < start`, strictly** → untouched.

2. **`start <= caret < end`** — note `end` is *exclusive*; an inclusive `<= end`
   sends an insert-at-caret down this branch and lands the caret *before* the
   inserted text, contradicting the tie-break above. Keep the offset into the
   replaced region:
   - `rel = caret.line - start.line`.
   - If `rel >= n` that line is gone: fall back to `(start.line + n - 1, end_col)`.
     **Fall back *or* clamp, never both** — an earlier draft said both, and they
     give different answers. (A clamp would also have been off by one: `n` is a
     count, `n - 1` the last index.)
   - Column when `rel == 0`: `start.index + min(caret.index - start.index, new_lines[0].len())`.
     **The `start.index` base is load-bearing and the first draft dropped it.**
     It is invisible in a whole-file format, where `start.index` is 0, and wrong
     for every single-line edit — which is every quickfix and every accepted
     inlay hint.
   - Column when `rel > 0`: `min(caret.index, new_lines[rel].len())`.

3. **`caret >= end`** → shift.
   - `delta = (n as isize - 1) - (end.line as isize - start.line as isize)`
   - `line = caret.line as isize + delta`
   - Column, when `caret.line == end.line`: `end_col + (caret.index - end.index)`.
     Otherwise unchanged.

**Then round the resulting column down to a char boundary of the line it landed
on.** The column is an *old* byte column clamped onto a *new* line, so nothing
above guarantees a boundary; `move_to` stores whatever it is given
(`iced/graphics/src/text/editor.rs:606-627`), and the next keystroke reaches
`String::split_off` and panics. This is the same rule `Bridge::clamp` applies,
for the same reason, and the first draft of this phase omitted it.

The selection anchor takes the whole rule independently; drop the selection only
if it collapses.

#### Verified

The rule above was implemented and checked against an independent oracle —
absolute byte offsets, where the arithmetic is obviously correct, converted back
to line and column. **All thirteen cases agree**, and the probe is the shape the
unit tests should take:

| case | result |
|---|---|
| replace, caret after it | agrees |
| insert exactly at caret | agrees (caret lands after, per the tie-break) |
| insert mid-line at caret | agrees |
| caret exactly at `end` | agrees (branch 3, not branch 2) |
| caret inside, non-zero `start.index` | agrees — **this is the one that catches the dropped base** |
| multi-line → multi-line | agrees |
| multi-line → single line (shrink) | agrees — **catches the unsigned delta** |
| caret strictly before | agrees |
| trailing newline in `new_text` | agrees — **catches `.lines()` vs `split('\n')`** |
| whole file, fewer lines | agrees |
| whole file, more lines | agrees |
| caret lands mid-character on the new line | agrees — **catches the missing boundary round** |
| multibyte with valid boundaries | agrees |

One consequence to document rather than fix: `move_to` does not scroll and
`Content` exposes no scroll accessor, so the viewport stays at the topmost edit
while the caret returns to where the user left it. Defensible — you want to see
what changed — but surprising if unstated.

**Exit:** the thirteen cases above, each as a named test; a selection whose
anchor and head fall on opposite sides of one edit; a whole-document reformat
that preserves line structure leaves the caret on the same logical line and
never at EOF.

### Phase 7 — Workspace edits
`workspace::Edit`, `workspace::Step`, `workspace::Operation`,
`workspace::Annotation`, and the accessors.

- **Normalize at construction, not on access.** `document_changes` supersedes
  `changes`; when only `changes` is present its entries are synthesized with
  `version: None`. This happens in the **inbound `From` impls** of Phases 9 and
  11 — inbound is `From`, `TryFrom` is outbound — which gives the precedence
  rule an owner and is what lets the accessors hand out borrows — an accessor
  that synthesized values could not return references to them.
- **`steps()` is the primary accessor**, over one ordered sequence of edits and
  operations. Two flat accessors would erase the interleaving that justifies
  keeping order at all: create A, edit A, rename A→B, edit B is a normal rename
  refactor.
- **One URI may appear more than once**, each time against a different version.
  Merging them would fold two batches computed against two different documents
  into one — precisely the corruption Phase 3 exists to catch.
- **`changes` is a `HashMap`**, so a round-trip test must be order-insensitive
  *across* documents while preserving order *within* one.
- **`workspace::Edit::new` is `pub(crate)`.** The fields are private so the
  accessors can borrow, and Rust field privacy is module-scoped — so
  `lsp::from_lsp_types`, a *sibling*, cannot build one without it. Phases 9 and
  11 are its only callers.
- **`annotation(id)` resolves a `Replacement::annotation_id`.** Without it the
  id is carried and unusable and `needs_confirmation` is unreachable. matcha
  never prompts; the app partitions the batch before calling `apply`.

**Exit:** a hand-built multi-document edit with interleaved operations and
annotations preserves step order, resolves its annotations, and keeps one URI's
two entries separate. (An *inbound conversion* test — real wire fixtures into
this type — ships in Phase 9; the full round trip needs outbound and is
Phase 10's.)

### Phase 8 — Code actions and the message envelope
`CodeAction`, `Command`, `Offer`, `Message`.

- **`Offer` is the union.** `textDocument/codeAction` answers with
  `Vec<CodeActionOrCommand>`; a bare `Vec<CodeAction>` has nowhere for a
  `Command`.
- **`Message` carries the envelope.** `PublishDiagnosticsParams` is
  `{ uri, diagnostics, version }` — an app with two buffers cannot route a
  variant that keeps only the middle field. `workspace/applyEdit` is a *request*
  whose params carry a `label`. A convenience type that loses what the direct
  path keeps is worse than no type.
- **Everything is `#[non_exhaustive]`** — `Message`, `Offer`, `Change`,
  `workspace::Edit`, `workspace::Step`, `workspace::Operation`, `Error`,
  `OutboundError` — or every future protocol
  addition is a breaking release.
- **`disabled` is carried with its reason**, not filtered: hiding versus greying
  out is policy and belongs to the app.
- **`kind` matching is hierarchical**: `refactor.extract` matches `refactor`,
  `refactory` does not. Document the rule beside the field.
- **`edit` and `command` may both be present**, and the edit applies first.
- **`Command.arguments` is a `Vec<String>`, one raw JSON value per argument**,
  so the app can reassemble `workspace/executeCommand` without matcha parsing
  anything.
- **`data` is carried on `CodeAction`** so an action survives
  `codeAction/resolve` going out and coming back enriched.

**Exit:** a `Message` round-trips its envelope; `Offer` holds both shapes.

### Phases 9 and 11 — inbound conversions
`From<lsp_types::X> for lsp::X` (Phase 9) and the same for `gen_lsp_types`
(Phase 11), plus `PositionEncodingKind → Encoding`. Each phase adds its own
`[features]` entry, its optional dependency and the `docs.rs` metadata.

**Reading a URI is the opposite operation in the two crates**, and this is the
single easiest thing to get wrong:

- **`lsp-types`: `as_str()`.** It is allocation-free and works at all three
  versions — `Url::as_str` at 0.95, a `Deref` to `fluent_uri::Uri::as_str` at
  0.96+. Note precisely what does *not* work at 0.96+: `Display` is not
  implemented on `Uri` itself, so `format!("{}", uri)` and
  `ToString::to_string` as a function reference both fail. `uri.to_string()` as
  a method call **does** compile, via that same `Deref` — so a reader who tests
  the naive claim finds it false. `as_str()` is still the rule; this is why.
- **`gen-lsp-types`: `to_string()`.** The default `Uri(pub String)` has no
  inherent `as_str` and no `Deref`. `AsRef<str>` works for the default and for
  the `url` form but not for `fluent-uri`; `Display` is the only accessor common
  to all three shapes (`fluent_uri` impls it at `fluent-uri/src/fmt.rs:48`), so
  **no accessor-selecting matcha feature is needed** — that contingency is
  settled, not open.
- The `url` / `fluent-uri` features remain an app-level hazard: they are
  mutually exclusive by `compile_error!` (`generated/common.rs:68-71`) and
  features are additive across a graph, so a transitive dependency can break a
  build the app cannot patch. Document it; there is nothing matcha can do.

**Integer enums:** `lsp-types` matches the associated consts with a `_` arm
(the inner `i32` is private); `gen-lsp-types` routes through `u32::from`, never
an exhaustive match, because 0.11 added `Custom(u32)`.

**Exit (each):** builds and lints at all three versions of its crate, restoring
`Cargo.lock` after each. Phase 9 additionally ships the **inbound** `workspace::Edit`
test Phase 7 could not build fixtures for, asserting the
`document_changes`-over-`changes` precedence and step order. The round trip
needs outbound and is Phase 10's exit.

### Phases 10 and 12 — outbound conversions and client capabilities
`TryFrom<lsp::X> for lsp_types::X` (Phase 10) and for `gen_lsp_types`
(Phase 12).

**Outbound is `TryFrom`, not `From`, and the reason is URIs.** At 0.96+ a `Uri`
is constructible only through `FromStr` (`lsp-types-0.97.0/src/uri.rs:42-53`);
at 0.95 the field is a `url::Url`, also `FromStr`. Both can fail, and with
`unwrap`/`expect` banned that makes every type carrying a URI fallible —
`Diagnostic` (its `code_description` is a `CodeDescription { href }` — easy to
miss, and it rides inside `CodeAction.diagnostics`), `Related`,
`document::Edit`, `workspace::Edit`, `Operation`, `CodeAction` and `Message`.
The spelling that compiles at all three, verified:

```rust
// The type is never named -- it is `Url` at 0.95 and `Uri` at 0.96+, so it
// cannot be. `parse()` infers it from the struct field, and the error types
// differ too, so they are erased.
let loc = lsp_types::Location { uri: s.parse().map_err(|_| uri::Error)?, range };

let mut changes = HashMap::new();   // key type inferred from the literal below
changes.insert(s.parse().map_err(|_| uri::Error)?, edits);
let edit = lsp_types::WorkspaceEdit { changes: Some(changes), ..Default::default() };
```

**Two more things can fail outbound, and both need a stated rule:**

- **`Change::Snippet` has no `lsp-types` analogue** — a document's edits there
  are `Vec<OneOf<TextEdit, AnnotatedTextEdit>>`. Converting a batch containing
  one is an error, not a silent drop, for the same reason `apply` refuses it.
- **`data` and `Command.arguments` are `LSPAny` on the wire**, not `String`
  (`lsp-types-0.97.0/src/lib.rs:223`; `gen-lsp-types` spells the alias
  `LspAny`). Reading is `value.to_string()`; **writing is
  `s.parse().map_err(..)?` with the type inferred from the struct field** — the
  same never-name-the-type trick as URIs. Do **not** reach for `serde_json`
  directly: neither crate re-exports it, so naming it is an undeclared
  dependency that the features table cannot fix.

`gen-lsp-types` is simpler: `Uri::from(String)` at default features, infallible —
but the `TryFrom` shape is kept for symmetry, because the `url` feature makes it
fallible again.

**Which URIs are valid differs across the `lsp-types` range.** Measured:
`"C:\\Users\\x\\a.rs"` parses at 0.95 and is rejected at 0.97. So the range
unifies *types*, not *behaviour*; say so next to the unification promise.

**Outbound is not optional.** Every justification for carrying `data`, `code`,
`source` and `tags` is an echo-back — into `CodeActionContext.diagnostics`, into
`codeAction/resolve`, into any request built from `Bridge::locate`. Inbound-only,
an app must keep the original `lsp_types` value alongside matcha's, at which
point matcha's copy is dead weight.

**Each phase also ships `client_capabilities()`**, in its own module
(`lsp::from_lsp_types::client_capabilities`) — two free functions of that name
would collide under `--all-features`. Prefilled to exactly what the bridge
understands, because every optional field arrives *only* if the app advertised
it: `workspaceEdit.documentChanges`, `.resourceOperations`,
`.changeAnnotationSupport`, `.normalizesLineEndings` (matcha **does** normalize —
helix advertises `false` and reproduces the corruption),
`.failureHandling: textOnlyTransactional` (honestly what per-document
all-or-nothing gives), `publishDiagnostics.{tagSupport, codeDescriptionSupport,
dataSupport, relatedInformation, versionSupport}` and `inlayHint.resolveSupport`.
This is the highest-value single item in the feature, and the core stays
zero-dependency because the types live here.

**Exit (each):** a `lsp_types::X → lsp::X → lsp_types::X` round trip compares
equal for `Replacement`, `workspace::Edit` and `CodeAction`. **`Hint` and
`gen-lsp-types`' `Diagnostic` are deliberately lossy** — hint labels are
flattened (Phase 4) and `gen-lsp-types` carries `Diagnostic.message` as a
`String | MarkupContent` union — so for those the test asserts *what survives*
and names what does not, rather than equality. Writing an equality test for them
would mean choosing a fixture that dodges the lossy branch, which is exactly the
vacuous test this plan warns about; an unparseable URI returns `Err`, never panics; builds and lints
at all three versions.

### Phase 13 — The mock-LSP example and the docs
The crate's first `[[example]]` block (forced by `required-features`), and
`serde_json` added to `[dev-dependencies]` — the example deserializes real
notification payloads, which is worth saying beside *Out of scope: serialization*
so the one-dependency claim stays honest. The example shows both the `Message`
path and the direct-payload path.

`lib.rs:102-110` needs more than a touch-up: it is the contract the bridge is
the counterexample to. Rewrite it to distinguish *"not currently visible"* from
*"no longer valid"* — the distinction the *Divergences* section turns on.

**Exit:** `cargo run --example lsp --features lsp-types` builds (a human runs
it); README and `lib.rs` no longer claim the protocol is out of scope.

### Execution order

```
1 ─▶ 2 ─▶ 3 ─▶ 5 ─┬─▶ 6
     │            └─▶ 7 ─┐
     └────▶ 4 ───────────┴─▶ 8 ─┬─▶ 9 ──▶ 10 ─┐
                                └─▶ 11 ─▶ 12 ─┴─▶ 13
```

Phase 2 is the gate. Beyond it there are three parallel opportunities, and the
plan is shaped to keep them:

- **Phase 4 (decorations) and Phase 5 (edits) are independent**, which is why
  `Replacement` ships in Phase 2 rather than with `apply` — putting it in
  Phase 5 would have made Phase 4 wait on it for one type.
- **Phase 6 is a leaf.** Caret restoration refines Phase 5 and nothing depends
  on it, so it can land whenever.
- **Phase 7 needs only Phase 5**, for `document::Edit` — not Phase 6, and not
  Phase 4.

Phase 8 is the join: `CodeAction` carries both `Vec<Diagnostic>` (Phase 4) and
`workspace::Edit` (Phase 7). Phases 9–10 and 11–12 are independent pairs, each
family's outbound phase needing its own inbound. Phase 13 needs Phase 10; the `12 → 13` edge is sequencing, not dependency — the example builds with `--features lsp-types` alone.

---

## Evidence — what was run rather than reasoned about

Probes are in the session scratchpad. Four of these overturned a decision in
this plan.

| claim | how it was checked | result |
|---|---|---|
| A permissive range unifies and compiles | one source built against `lsp-types` 0.95.0/0.96.0/0.97.0 and `gen-lsp-types` 0.9.0/0.10.0/0.11.0 | all six compile |
| The structs the bridge reads are stable | `md5sum` on `inlay_hint.rs`; struct-level diff of `Diagnostic`, `DiagnosticSeverity`, `TextEdit`, `Position`, `Range` | identical; only `Url`→`Uri` and added `Hash` derives differ |
| Column↔byte conversion is correct | 11-case table plus a round-trip over every char boundary × 3 encodings × 5 fixtures | passes, **after fixing a mid-surrogate bug** |
| A module under `code_editor` reaches `Content`'s `pub(super)` field | compiled a model of the module tree | yes — zero-alloc line borrow available |
| `move_to` + `Edit::Paste` replaces a range atomically | 8 tests against the real `Content` | all pass |
| A CRLF buffer plus bare `\n` corrupts endings | ran it | `"a\r\nb\r\nc"` → `"a\r\nX\nY\r\nc"`, mixed |
| `lsp-types` URIs are readable across the range | built the full `WorkspaceEdit` traversal at all three | `as_str()` works everywhere. `Display` is **not** on `Uri` at 0.96+, so `format!("{}", uri)` fails — but `uri.to_string()` **does** compile via `Deref`. Rule is `as_str()`, for allocation not availability |
| `gen-lsp-types` URIs are readable across the range | built at 0.9.0 and 0.11.0 | no inherent `as_str`, no `Deref`. `AsRef<str>` works for the default and `url` forms; **`Display` is the only accessor common to all three**, so `to_string()` — the opposite rule |
| `gen-lsp-types` integer enums are readable across the range | built unions and `WorkspaceEdit` at all three | only via `u32::from()` — 0.11 added `Custom(u32)` |
| matcha's line splitting agrees with LSP's | 9 terminator cases through `Content` | agrees on `\n`, `\r\n`, bare `\r`; correctly does *not* split U+2028/U+2029/VT/FF/NEL. **One divergence: `\n\r`** |
| A stable *descending* sort reverses same-position inserts | ran both formulations | `"XY"`+`[A,B]` → `"XBAY"` vs `"XABY"`; three inserts → `"321Z"` vs `"123Z"` |
| Sorting by `start` alone corrupts text | ran both sort keys, both array orders | `[replace, insert]` → **`"QYZ"`** instead of `"AQZ"`, *and* a false overlap flag. `(start, end)` is correct either way |
| A URI can be WRITTEN back across the range | compiled `Location` and `WorkspaceEdit` construction at 0.95/0.96/0.97 | yes, via `s.parse().map_err(..)?` with the type inferred from the struct field. **Outbound must be `TryFrom`** — and `"C:\\Users\\x\\a.rs"` parses at 0.95 and is rejected at 0.97 |
| `line_ending()` is unsafe to normalize against | 6 buffer shapes | `Content::new()` and any single-line buffer report `LineEnding::None`, `as_str() == ""` |
| The widget already sorts hints | read `widget.rs:800-812` | sorts by shaped geometry — better than logical position, and makes a bridge-side sort redundant |
| Caching a `Ref` in `Bridge` is a compile error, not a panic | compiled both the external and crate-internal cases | external is E0502; **the runtime panic is confined to `Content::apply`'s own `borrow_mut`** |

### The `\n\r` divergence, and what it costs

cosmic-text has a `LineEnding::LfCr` variant and reads `\n\r` as **one**
terminator; LSP reads it as two. This is not a local error: a single such line
shifts **every subsequent line number** in the document between server and
client, desynchronizing the rest of the file silently. `\n\r` is a RISC OS
convention and vanishingly rare in source code, so it is accepted — but it is
accepted knowingly, and that is why it is written down.

---

## Conversion rules that keep the ranges working

The ranges hold **only** if the conversions are written these ways. Each was
found by compiling against every version in the range, and the obvious
formulation fails in each case.

| | `lsp-types` (`>=0.95, <0.98`) | `gen-lsp-types` (`>=0.9, <0.12`) |
|---|---|---|
| read a URI | `as_str()` — allocation-free at all three; `Display` is absent on `Uri` itself at 0.96+ | `to_string()` — no inherent `as_str`, and `Display` is the only accessor common to all three `Uri` shapes |
| **write a URI** | `s.parse().map_err(\|_\| uri::Error)?`, type inferred from the struct field — it is `Url` at 0.95 and `Uri` at 0.96+ and so can never be spelled. **Fallible, hence `TryFrom`** | `Uri::from(String)`, infallible at default features — but kept `TryFrom` for symmetry, since the `url` feature makes it fallible |
| read an integer enum | match the associated consts, `_` arm — the inner `i32` is private | `u32::from()` — 0.11 added `Custom(u32)` |
| workspace traversal | nested: `Option<DocumentChanges>` → `Edits \| Operations` → `Op \| Edit` → `Create \| Rename \| Delete` | flat `Vec<DocumentChange>`, 4 variants |
| a document's edits | `Vec<OneOf<TextEdit, AnnotatedTextEdit>>` | `Vec<Edit>` — 3 cases, incl. `SnippetTextEdit` |
| unions | one generic `OneOf<A, B>` | 88 hand-named `#[serde(untagged)]` enums (87 at 0.9) |
| `Diagnostic.message` | `String` | a `Message` union needing `MarkupContent` flattening |
| document identifier | `id.uri` | `id.text_document_identifier.uri` |

Build `CodeAction` and `Command` with **struct literals, never positional
constructors** — field order differs between the families and `gen-lsp-types`
carries extra `tags` / `tooltip` fields.

---

## Measured against the field

Compared against helix (`helix-lsp`), zed (`crates/project`, `crates/language`),
lapce (`lapce-core`) and rust-analyzer (`lib/line-index`). All four solve this
exact problem; none of them agree with each other.

### Where the plan is ahead, and of whom

Recorded so none of these gets "simplified" away by someone assuming the big
editors must be right.

| | matcha | the field |
|---|---|---|
| validate-then-commit | yes | helix filters inline and has a standing TODO wishing for it (`helix-view/src/handlers/lsp.rs:60-61`); zed clips and merges |
| normalize `new_text` endings | yes | helix does **not**, and advertises `normalizesLineEndings: false` (`client.rs:615`) — it reproduces the exact corruption measured here |
| all three encodings negotiated | yes | lapce hardcodes UTF-16; zed hardcodes it in the type system. Only helix also negotiates |
| mid-character rounds down | yes | correct in helix and zed; **lapce rounds up** (`lapce-core/src/encoding.rs:70-78`) — the bug Round 0 caught here |
| outbound conversion is fallible | `Option` | helix's `pos_to_lsp_pos` **panics** (`lib.rs:222`); rust-analyzer `.unwrap()`s (`to_proto.rs:47`) |
| no dependency in the core | yes | helix **vendored all of `lsp-types`** (36 files) to escape the same version split |
| line splitting matches LSP | yes | helix has a standing FIXME that it does not (`lib.rs:178-182`). Verified by probe that matcha's does |
| refuses a stale buffer | yes | helix re-requests; only zed (via anchors) and VS Code (via `modelVersionId`) handle it at all |

**rust-analyzer's `line-index` is the canonical fast implementation and it has a
hole.** `to_utf8` (`lib/line-index/src/lib.rs:164-178`) returns an offset *inside*
a character for a mid-surrogate column, and its round-trip test only walks
`char_indices()`, so it never exercises that case. Phase 2's exit adds exactly
the missing case. Do not copy `to_utf8` verbatim.

### Deliberately not copied

- **helix's whole-document diff** (`lib.rs:420-432`), which preserves anchors
  across an arbitrary rewrite. matcha holds no anchors and rebuilds decorations
  every frame. Note this is *not* the same as restoring the caret, which Phase 6
  does do — conflating the two was a framing error in an earlier draft.
- **zed's `Unclipped<T>` newtype.** matcha gets the same guarantee from keeping
  `lsp::Position` and `crate::Position` distinct, which is now an invariant.

---

## Testing strategy

- **Unit, in `#[cfg(test)] mod tests`** beside each new module.
- **Integration, `tests/lsp.rs`**, gated `#![cfg(feature = "lsp")]`, arriving in
  **Phase 4** rather than at the end — it is what proves the converted types
  satisfy the widget's `&'a [Hint<'a>]` bound, and that fails at integration
  time or not at all.
- **Doctests.** The `lsp` module gets a `no_run` example.
- **Snapshots: none added.** The feature draws nothing.

**Mutation-test every load-bearing line**: break it, confirm a *named* test
fails. Six vacuous tests were caught this way on a predecessor plan.

A plain `cargo test` exercises none of this code, so the matrix is the point:

```sh
cargo test                                                        # must stay green
cargo test --features lsp
cargo test --features lsp-types
cargo test --features gen-lsp-types
cargo test --all-features
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features lsp -- -D warnings
cargo clippy --all-targets --features lsp-types -- -D warnings
cargo clippy --all-targets --features gen-lsp-types -- -D warnings
cargo clippy --all-targets --all-features -- -D warnings   # the only run that
                                                           # catches a
                                                           # client_capabilities
                                                           # collision
cargo fmt --all -- --check
cargo doc --no-deps --all-features
```

---

## Out of scope

- **Transport.** No process spawning, no JSON-RPC framing, no request
  correlation, no async runtime. Confirmed with the user.
- **The initialize handshake.** matcha *contributes* `client_capabilities()`
  (Phases 10 and 12) but never sends it, and never decides the `positionEncoding`.
- **Applying workspace-level operations.** Represented, never performed.
- **Prompting for annotated edits.** `needs_confirmation` is carried; matcha has
  no UI and will not grow one. The app partitions before calling `apply`.
- **Serialization.** matcha never deserializes, which is why the `lsp` core
  needs no `serde`. `data` rides as opaque text.
- **Incremental `didChange`.** matcha cannot report *what* changed: iced's
  `History` and `Internal` are private and `perform` exposes no change stream.
  An app can therefore only send `TextDocumentSyncKind::Full`, and
  `Content::text()` rebuilds the whole buffer as a fresh `String`
  (`iced/core/src/text/editor.rs:93-110`) — roughly 600 KB of allocation per
  keystroke on a 20k-line file. That dwarfs everything the encoding layer does
  and is the real performance cliff in this feature. Fixing it is an upstream
  iced change. The app must know, so this is documented rather than silent.
- **Pull diagnostics** (`textDocument/diagnostic`, LSP 3.17). A genuinely
  different shape — `Full | Unchanged { result_id }` plus `related_documents` —
  and where the ecosystem is heading. Additive later; named here so its absence
  reads as a decision.
- **Semantic tokens, completion, hover, signature help, formatting requests.**
  The widget cannot draw or host any of them.
- **Range-limited inlay-hint requests.** `textDocument/inlayHint` takes a range
  so a client can ask for the viewport; matcha knows the visible rows only in
  `geometry::visible_line_rows`, which constraint 1 keeps closed. Hints are
  therefore whole-file. Revisit by relaxing the constraint for one read-only
  accessor, not by reaching into the widget.
- **Coalescing a batch into one undo step.** iced's `History` is private.
- **Diffing a whole-document replacement.** See *Deliberately not copied*.

---

## Files touched

| phase | files |
|---|---|
| 1 | `Cargo.toml`, `src/lib.rs`, `src/code_editor.rs`, **new** `lsp.rs`, `lsp/position.rs`, `lsp/encoding.rs` |
| 2 | **new** `lsp/bridge.rs`, `lsp/replacement.rs` |
| 3 | `src/code_editor/content.rs` *(the revision counter — the only pre-existing file whose behaviour changes)*, `lsp/bridge.rs` |
| 4 | `lsp/bridge.rs` *(the batch methods, and removing Phase 2's `allow(dead_code)`)*, **new** `lsp/diagnostic.rs`, `lsp/hint.rs`, `tests/lsp.rs` |
| 5 | **new** `lsp/document.rs`, `lsp/apply.rs`, `lsp/error.rs` |
| 6 | `lsp/apply.rs` |
| 7 | **new** `lsp/workspace.rs` |
| 8 | **new** `lsp/action.rs`, `lsp/message.rs` |
| 9, 11 | `Cargo.toml`, **new** `lsp/from_lsp_types.rs` / `lsp/from_gen_lsp_types.rs` |
| 10, 12 | the same two files, plus their `client_capabilities`; Phase 10 also adds **new** `lsp/uri.rs` and declares it in `lsp.rs` |
| 13 | `Cargo.toml`, `README.md`, `src/lib.rs`, **new** `examples/lsp.rs` |

Every phase that introduces a module edits `src/code_editor/lsp.rs` to declare
it — Phases 3, 6 and 12 add no module and do not. The
`impl Content` blocks live in `lsp/bridge.rs` and `lsp/apply.rs`, not in
`content.rs`, so `content.rs` appears exactly once — in Phase 3, for the
revision counter.

**`src/code_editor/widget.rs` and `src/code_editor/geometry.rs` appear nowhere.**
If a phase wants to edit either, the design has been broken — stop and report.

---

## TODO before dispatch

- [ ] Re-read `.claude/map/inlay-hint-chips/goon.yaml` style and workflow rules
- [ ] Re-read `src/lib.rs:71-110` — the invariants the bridge must keep true
- [ ] Verify every file:line citation still resolves
- [ ] Confirm `cargo test`, `clippy --all-targets -D warnings`, `fmt --check` green

Decisions settled above that are easy to lose in transcription. A phase agent
that gets any of these wrong writes text-corrupting code:

- [ ] Sort key is **`(start, end)`, stable ascending, iterated in reverse**
- [ ] `line > line_count` **passes through** on the decoration path; `line == line_count` clamps on **both**
- [ ] Unknown line ending means **do not normalize**, not "default to `Lf`"
- [ ] `clamp` rounds down; `exact` refuses; both derive from `resolve`, which carries the reason
- [ ] The revision bumps on **`action.is_edit()`**, not on every `perform`
- [ ] A caret inside a replaced range **keeps its relative offset**; clamping to the end puts every caret at EOF on a whole-file format
- [ ] Reversed range: swap for decorations, **reject** for edits; ship `collapsed()`
- [ ] `hints()` is not total; `diagnostics()` is; neither sorts
- [ ] `as_str()` for `lsp-types`, `to_string()` for `gen-lsp-types` — opposite rules
- [ ] Outbound is **`TryFrom`**, because a URI can fail to parse
- [ ] `client_capabilities()` is per-family and namespaced, or `--all-features` collides
- [ ] The `cargo update --precise` carve-out, with `git checkout -- Cargo.lock`

---

## Critique resolution log

Kept so a future session can see why each decision is what it is, and which
were reversed.

### Round 0 — pre-critique verification

Eight assumptions checked by running code. Two were wrong: mid-surrogate columns
rounded the wrong way (fixed to round down), and `lsp-types` as a hard dependency
was killed by the `async-lsp @ ^0.95` vs `0.97` disjointness — which is what led
to the user's call that matcha should ship both families' conversions rather
than leave clients to write the glue.

### Round 1 — comparison against helix, zed, lapce, rust-analyzer

Changed "drop an out-of-range line" to "clamp it", on the grounds that
`line == lineCount` is how LSP spells end-of-document. **Round 3 showed this
over-corrected** — see below. Also added `data` to diagnostics (without it
rust-analyzer resolves no quickfix), split inverted-range handling by consumer,
pinned the overlap comparison to strict `<`, and added the missing protocol
fields.

### Round 2 — plan critique

Three text-corrupting bugs and a naming collision. `Content::apply` had no
`Encoding` and so could only ever have been correct for UTF-8. The sort prose
("sort stably … descending") inverts same-position inserts, though the algorithm
verified in Round 0 was right — **the code was correct and the sentence
describing it was not**, which is the most instructive miss of the exercise.
Line-ending normalization against `LineEnding::None` deletes newlines.
`lsp::Edit` collided with the already-public `matcha::Edit`. And the
composite-name exception was being applied inconsistently, which is what
produced the uniform naming table.

### Round 3 — expert critique and verification pass

The largest round, and it reversed two Round-1/Round-2 "fixes":

1. **The sort key was still wrong.** `(start)` is not enough; it must be
   `(start, end)`. Measured: `[replace(0..2,"Q"), insert(0,"A")]` over `"XYZ"`
   yields `"QYZ"` instead of `"AQZ"` *and* falsely trips the overlap guard. LSP
   says the array need not be ordered and helix's code shows Omnisharp ships
   reversed arrays.
2. **Round 1's clamp over-corrected.** Clamping a line beyond the buffer paints
   a false squiggle at the end of the last line, because `range_fragments`
   widens a zero-width range (`geometry.rs:56-80`), and it breaks the invariant
   `lib.rs:102-110` states outright. Now: clamp `line == line_count` only; let
   anything beyond pass through so the widget drops it silently.
3. **Round 2's `LineEnding::None` → `Lf` fix was wrong in its only case.** A
   one-line buffer carries no evidence of the file's convention, so defaulting
   to `Lf` rewrites a DOS file's `\r\n`. Now: do not normalize at all when there
   is no evidence.
4. **The `Ref` constraint's rationale was inverted.** Compiled: the external
   case is E0502, a compile error. The runtime panic is confined to
   `Content::apply`'s own `borrow_mut`. The conclusion was right and the reason
   pointed at the one place the hazard is not.
5. **The versioning hole.** Converting against the current buffer while the
   server described an older one is fine for decorations and silent corruption
   for edits. Added `Content::revision()`, `Error::Stale`, and Phase 3.
6. **The caret decision was closed, not shipped open.** A whole-document format
   — the case Round 1 rescued — would land the caret at EOF, scroll to the
   bottom and destroy the selection. Every editor in the comparison preserves it.
7. **No outbound conversion existed**, which made `data`, `code`, `source` and
   `tags` unusable for the echo-back they were added for.
8. **`gen-lsp-types` needs `Display`, not `as_str()`** — the opposite of
   `lsp-types`. Verified. The plan would not have compiled in Phase 9.
9. **`lsp::Severity` duplicated `decoration::diagnostic::Severity`**; deleted.
10. **`Message` dropped its routing envelope** (`uri`, `version`, `label`),
    making it unusable for the multi-buffer dispatch it was justified by.
11. **The widget already sorts hints by geometry** (`widget.rs:806`), so the
    planned bridge-side sort was redundant *and* broke index correspondence.
12. **`edits_for(uri) -> &[Replacement]` was structurally impossible** — one URI
    may appear several times against different versions, and flattening merges
    batches. Replaced with an ordered iterator.
13. **Client capabilities** added to the conversion phases (Phases 10 and 12
    under the final numbering): every modelled field arrives
    only if the app advertised it, and prefilling that is the most valuable
    single thing this feature can ship.

### Round 4 — verification of the rewrite

Six blockers, and the recurring failure mode recurred: the rewrite's own large
edits left the surface block, the files table, the DAG and `goon.yaml`
disagreeing with the phase prose.

1. **The revision bump was specified as "every mutation"**, but the widget
   publishes *every* action for the app to feed back, so `Scroll`, `Click` and
   `Drag` all reach `perform` — the revision would change on every mouse-move of
   a drag-select and `apply` would return `Stale` for unchanged text. Now
   `action.is_edit()`.
2. **`exact` threw away the reason**, so `apply` could not build the three
   data-carrying `Error` variants without re-validating by hand. Added
   `resolve -> Result<_, Reason>` as the real converter.
3. **Phase 4 depended on Phase 5** via `Hint.text_edits: Vec<Replacement>`,
   contradicting the DAG. `Replacement` moved to Phase 2.
4. **Outbound conversion cannot be `From`.** A `Uri` is constructible only
   through `FromStr` and can fail — and which URIs are valid *differs across the
   range*: a Windows path parses at 0.95 and is rejected at 0.97. Now `TryFrom`,
   with the spelling that compiles at all three.
5. **`client_capabilities()` collided** under `--all-features`. Namespaced per
   family.
6. **`Change::Snippet(Unsupported)` named a type that was never defined**, and
   `apply` had no way to refuse it.
7. **The caret rule produced the failure it existed to prevent.** "Clamp to the
   end of the replacement" puts *every* caret at EOF for a whole-file format,
   since every caret is inside that one edit. Now: preserve the relative offset,
   with the boundary comparisons and the column arithmetic spelled out.
8. **`goon.yaml`'s style section still granted the old naming exception**, so an
   agent reading it as configuration would have written `InlayHint`.
9. **Two Evidence rows were imprecise.** `uri.to_string()` *does* compile at
   0.97 via `Deref`; what fails is `format!`. And `AsRef<str>` *does* read a
   `gen-lsp-types` URI — the plan's own probe used it. Both rules survive; both
   justifications were rewritten.

Also: `document_edits()` could not return borrows to values it synthesized, so
`workspace::Edit` now normalizes at construction and exposes an ordered
`steps()`; `Cell<u64>` became a plain `u64`; `Clone`'s revision reset specified;
Phase 2's dead-code window acknowledged; and the phase count went 10 → 13,
splitting the two oversized edit and conversion phases.

### Round 5 — verification of the restructure

Phase 6 — the caret arithmetic added in Round 4, the only wholly unreviewed
material — was wrong in four places. It was **rewritten from a working
implementation** rather than re-argued: the rule was coded and checked against
an independent oracle (absolute byte offsets, converted back to line and
column), and all thirteen cases now agree. What that caught:

1. **The `end` boundary was inclusive**, so an insert at the caret took the
   preserve-offset branch and landed the caret *before* the inserted text —
   contradicting the phase's own stated consequence and its exit criterion.
   It is `start <= caret < end`.
2. **The column arithmetic dropped the `start.index` base**, which is only
   correct when `start.index == 0` — true of a whole-file format, and false of
   every quickfix and every accepted inlay hint. The headline case masked it.
3. **Nothing rounded the restored column to a char boundary.** It is an old byte
   column clamped onto a new line; `move_to` stores it verbatim and the next
   keystroke panics in `String::split_off`.
4. **"Clamp to the replacement's line count" and "fall back to its end" were
   given as one instruction** for the same input, and they disagree; the clamp
   was also off by one.

Plus two definitions that silently change answers and were never stated: the
line delta must be **signed** (a shrinking reformat makes it negative), and
"line count" means `split('\n')`, not `.lines()` — `"x\n"` is two lines, and a
formatter adding a trailing newline is the common case.

Elsewhere, ten targeted corrections. The load-bearing ones: `Reason` needed a
payload on `NotACharBoundary` or `clamp` could not derive its round-down from
`resolve` (two converters again); `Reason::Line`'s field is unread until Phase 5
and would have failed Phase 2's own clippy gate; `Content` is a **tuple** struct,
so the revision counter is a second tuple element — a named field would have
meant editing `widget.rs` and tripping the plan's own wire; `workspace::Edit`'s
private fields are unreachable from the sibling conversion modules without a
`pub(crate)` constructor; normalization was assigned to `TryFrom` when inbound is
`From`; outbound needs an error type that was used three times and defined
nowhere; and a range with a valid start but a stale *end* squiggles from the
start to EOF, which the per-position pass-through rule did not cover.

`goon.yaml` had re-introduced Round 4's blocker as configuration — "Content::apply
uses exact" — which is the version that cannot build three of the error
variants.

(The 8 → 10 restructure below belongs to **Round 3**, not this round; it is
kept here because it is where the reasoning was recorded.) The phase count went 8 → 10, `Bridge` moved into Phase 2 so that phase has
a testable public surface, `tests/lsp.rs` moved to Phase 4, `Change::Snippet`
given a home, `#[non_exhaustive]` applied, and the public-surface block
regenerated from the phases after it drifted out of sync with them.

### Round 6 — contradiction audit

One compile error, three unreconciled signatures, and a crop of stale
cross-references from Round 5's own edits.

1. **The revision counter did not compile.** `Content(pub(super) RefCell<..>, u64)`
   leaves the second element private to `content`, and the accessor lives in
   `lsp/bridge.rs` — a *sibling*, not a descendant. `self.1` is `error[E0616]`.
   Verified both ways; it needs `pub(super)` on both elements. The Round 5 fix
   landed one modifier short.
2. **`Bridge::range`'s signature was stale.** Round 5 added the rule that a
   range with a stale *end* must yield nothing, and did not regenerate the
   surface block — which still returned a total `TextRange`.
3. **Nothing said what `diagnostics()` emits when `range()` is `None`**, and the
   obvious fallback re-creates the start-to-EOF squiggle the `None` exists to
   prevent. Now stated: both endpoints on the out-of-range line, so the widget's
   own filter drops it.
4. **`Content::apply`'s return type disagreed** between the plan (`Result<u64>`)
   and its phase doc (`Result<()>`). `Result<u64, Error>` wins — the caller
   needs the new revision to pair with its next `didChange`.
5. **Outbound had an error type for URIs and none for snippets**, so a snippet
   failure would have been reported as a URI failure. Added `OutboundError`.
6. **The naming table mis-mapped `workspace::Operation`** (it is `ResourceOp`;
   `Step` is `DocumentChangeOperation`) and omitted `Encoding`, `Snippet` and
   the three resource types.
7. **`Diagnostic` was missing from the fallible-outbound list** — its
   `code_description` is a `CodeDescription { href }`, and it rides inside
   `CodeAction.diagnostics`, which is on Phase 10's round-trip path.
8. **`goon.yaml` forbade `cargo update` flatly** while its own `range_check`
   required it, sent the doc rewrite to Phase 8, and built without linting.

The audit also confirmed Phase 6's rewritten arithmetic agrees line-for-line
between the plan and its phase doc, and that the Round-5 DAG correction holds.
