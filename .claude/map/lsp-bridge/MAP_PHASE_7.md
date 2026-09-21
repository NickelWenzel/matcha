# Phase 7 — Workspace edits

The container a code action's edits arrive in. matcha **represents** it in full
and **applies** only the single-document parts; routing, resource operations and
confirmation belong to the application, which is the only thing that knows about
files.

## Prerequisites

Phase 5: `Replacement`, `Change`, `document::Edit`.

**Not** Phase 6 (caret restoration) and **not** Phase 4 (decorations) — this
phase can run in parallel with both.

## Goal and exit criteria

`workspace::Edit` exists, preserves the order of edits and resource operations,
resolves annotations, and keeps two entries for one URI separate.

## Step 1 — the type, normalized at construction

**This is not a mirror of the wire shape.** The protocol offers the same
information two ways — a `changes` map and a `document_changes` list, with
`document_changes` superseding it when both are present. Something has to
implement that precedence or two applications using matcha will disagree, so it
is done **once, on the way in**, in the inbound `From` impls of Phases 9 and 11.
Inbound is `From`; `TryFrom` is the outbound direction.

Normalizing at construction is also what lets the accessors hand out borrows: an
accessor that synthesized entries on the fly could not return references to them.

```rust
/// Everything a server wants changed.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    steps: Vec<Step>,
    annotations: HashMap<String, Annotation>,
}

/// One thing to do, in the order the server asked for it.
///
/// Edits and resource operations share one sequence because the interleaving
/// is load-bearing: *create A, edit A, rename A→B, edit B* is an ordinary
/// rename refactor, and two separate accessors would lose the order between
/// them.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Edits to one document.
    Document(document::Edit),
    /// A file to create, rename or delete.
    Operation(Operation),
}
```

## Step 2 — the constructor Phases 9 and 11 need

The fields are private so the accessors can borrow. Rust field privacy is
**module-scoped**, and `lsp::from_lsp_types` is a *sibling* of `lsp::workspace`,
not a child — so without this it cannot build one, and Phase 9's exit criterion
is unreachable.

```rust
impl Edit {
    /// Builds one from already-normalized parts.
    ///
    /// `pub(crate)`: the conversion modules are its only callers, and the
    /// precedence rule has already been applied by the time they call it.
    pub(crate) fn new(steps: Vec<Step>, annotations: HashMap<String, Annotation>) -> Self {
        Self { steps, annotations }
    }
}
```

## Step 3 — the accessors

```rust
impl Edit {
    /// Every step, in the order the server sent them. The primary accessor.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// Just the document edits, for an application with no other file open.
    ///
    /// One URI may appear more than once, each time against a different
    /// version — that is what a rename refactor looks like — so these are
    /// yielded separately and never merged. Merging them would fold two
    /// batches computed against two different documents into one, which is
    /// exactly the corruption [`Content::revision`] exists to catch.
    pub fn document_edits(&self) -> impl Iterator<Item = &document::Edit> {
        self.steps.iter().filter_map(|step| match step {
            Step::Document(edit) => Some(edit),
            Step::Operation(_) => None,
        })
    }

    /// Resolves a [`Replacement::annotation_id`].
    ///
    /// Without this the id is carried and unusable. matcha never prompts; an
    /// application that honours `needs_confirmation` partitions the batch on
    /// this before calling [`Content::apply`].
    pub fn annotation(&self, id: &str) -> Option<&Annotation> {
        self.annotations.get(id)
    }
}
```

## Step 4 — operations and annotations

Their shapes are public API that Phases 9 and 11 must round-trip, so they are
fixed here rather than invented later.

```rust
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    /// Create a file.
    Create(Create),
    /// Rename or move one.
    Rename(Rename),
    /// Delete one.
    Delete(Delete),
}

/// A file to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Create {
    /// The file, as the server spelled it.
    pub uri: String,
    /// Replace it if it exists.
    pub overwrite: bool,
    /// Do nothing if it exists. Ignored when `overwrite` is set.
    pub ignore_if_exists: bool,
    /// The change annotation this belongs to, if any.
    pub annotation_id: Option<String>,
}

// `Rename { old_uri, new_uri, overwrite, ignore_if_exists, annotation_id }`
// and `Delete { uri, recursive, ignore_if_not_exists, annotation_id }` follow
// the same shape.

/// A label a server attached to a group of changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    /// Shown to the user.
    pub label: String,
    /// The client must ask before applying. **matcha never asks** — it has no
    /// UI. An application that wants this partitions the batch itself.
    pub needs_confirmation: bool,
    /// A longer description.
    pub description: Option<String>,
}
```

## Verification

```sh
cargo test --features lsp
cargo clippy --all-targets --features lsp -- -D warnings
```

## Spot checks

Built by hand — the `From`-and-back round trip needs Phase 9 to construct
fixtures and ships there.

| case | expected |
|---|---|
| create A, edit A, rename A→B, edit B | `steps()` yields all four **in that order** |
| the same input | `document_edits()` yields **two** entries, not one merged |
| one URI, two edits, different versions | both survive, versions distinct |
| a `Replacement` with `annotation_id: Some("x")` | `annotation("x")` resolves |
| `annotation("nope")` | `None` |
| an `Edit` built only from `changes` | every entry has `version: None` |
| an `Edit` with both `changes` and `document_changes` | only `document_changes` appears |

The last two are the precedence rule and are the ones worth mutation-testing:
invert the precedence and a named test must fail.

Because `changes` is a `HashMap`, a round-trip test must be order-insensitive
**across** documents while preserving order **within** one. Order within a
document is load-bearing; order across them is not.

## What NOT to change

- **`widget.rs`, `geometry.rs`, `content.rs`.**
- **No applying.** matcha never performs a resource operation and never prompts.
- **Do not expose the fields.** They are private so the accessors can borrow.
- **Do not add a `edits_for(uri)` accessor.** It cannot represent a URI that
  appears twice, and flattening those merges two documents' versions.
