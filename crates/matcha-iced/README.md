# matcha

[![CI](https://github.com/NickelWenzel/matcha/actions/workflows/ci.yml/badge.svg)](https://github.com/NickelWenzel/matcha/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/matcha-iced)](https://crates.io/crates/matcha-iced)
[![docs.rs](https://img.shields.io/docsrs/matcha-iced)](https://docs.rs/matcha-iced)

A from-scratch code-editor widget for [iced](https://iced.rs) — a headless
editing engine plus an `iced::advanced::Widget` that renders it.

matcha began as a fork of [scrive](https://github.com/robbym/scrive) by Robby Madruga.

![The matcha editor: syntax highlighting, multi-cursor rename, find and replace with match navigation and replace-all, code folding, and language-server features — inlay hints, hover, completion, signature help, diagnostics, goto definition and rename — then the whitespace marks](https://raw.githubusercontent.com/NickelWenzel/matcha/master/docs/showcase.gif)

<sub>Recorded deterministically, headless — the demo renders itself, no external tools:
`cargo run -p matcha-iced --features lsp,tree-sitter --example record_showcase --release -- docs/showcase.gif`</sub>

Three crates, with every dependency pointing one way:

- **`matcha-core`** — the headless engine: a rope-backed buffer, atomic
  multi-range transactions with mechanically-derived undo/redo, selections and
  multi-cursor, tracked-range decorations (diagnostics, find matches, snippet
  stops), an incremental syntax-highlight cache, folding, and the
  language-intelligence controllers. Imports no GUI crate.
- **`matcha-iced`** — an `iced::advanced::Widget` wrapping the core: gutter and
  line numbers, N-caret selections, syntax highlighting, bracket-pair colors and
  matching, indent guides, diagnostic squiggles, code folding, and the
  completion / hover / signature-help popups. Depends on `matcha-core`; the
  dependency never points back.
- **`matcha-lsp`** — a Language Server Protocol client. It owns the connection
  to the server (a child process, TCP, a WebSocket, or an in-process server),
  keeps the server's copy of each open document in sync, and turns its replies
  into per-document updates. Depends on `matcha-core` and no GUI crate;
  `matcha-iced` pulls it in behind its `lsp` feature.

## Features

- **Rope-backed buffer** with an O(1) snapshot for background work; the
  document-size envelope is bounded only by the u32 offset space, not a load cap.
- **Atomic transactions with undo/redo** — every edit is one transaction with a
  mechanically-derived inverse, and undo/redo flow through the same
  view-rebase path as forward edits.
- **Multi-cursor** editing, column (box) selection, and word-wise motion.
- **Syntax highlighting** from [syntect](https://github.com/trishume/syntect)
  `.sublime-syntax` grammars or [tree-sitter](https://tree-sitter.github.io)
  grammars, picked per editor. It is incremental and windowed to the viewport,
  and the app supplies the grammar and theme (the core ships neither).
- **Bracket-pair colorization, matching, and indent guides** — optionally
  comment/string-aware, so brackets inside line comments, strings, and char
  literals are skipped (opt-in per language; line-local).
- **Code folding** — foldable ranges, gutter chevrons, nested and inline
  (sub-line) collapse, and fold-aware movement.
- **Visible whitespace** (optional) — spaces drawn as `·` and tabs as `→`.
- **Find and replace** — literal, whole-word, or regex search (with `$1` capture
  groups in the replacement), find-in-selection, case-preserving replace, and
  replace / replace-all as a single undo step. The match set is repaired per
  edit, so results never go stale.
- **Language intelligence** — completion, snippets, signature help, and hover,
  exposed as trait seams the integrating application implements.
- **Language servers** (the `lsp` feature) — one client per server, many
  documents per client: incremental sync (including undo), diagnostics,
  completion, signature help, hover, goto definition, rename across files,
  formatting, and inlay hints. Late replies are dropped, never applied to text
  that moved. The client runs the server over stdio, TCP, a WebSocket, or
  in-process, restarts it when it crashes, and shuts it down cleanly.
- **Diagnostics** — squiggles, a diagnostic hover, and scrollbar overview marks.

Every derived position — a caret, a find match, a diagnostic, a snippet stop — is
moved through a windowed or O(log n) update per keystroke, so no editing
operation scales with the size of the document.

## Design

The load-bearing idea is **one fact, one owner**: every derived position moves
through a single mapping function on every edit, and every change is one atomic
transaction with a mechanically-derived inverse — so the class of bug where two
copies of one fact drift apart cannot occur, on the forward path or on undo.

The editor is a direct `iced::advanced::Widget` rather than a `canvas::Program`,
because only the low-level widget API participates in iced's focus/operation
protocol. The low-level `Editor` is a **controlled widget**: it borrows the
`Document` immutably for drawing and emits semantic actions (`Type`, `Move`,
`Undo`, …); the application owns the `Document` and applies the actions, so the
widget never mutates state behind the app's back. The `CodeEditor` tier wraps
that widget, owns the `Document` itself, and runs the plumbing for you.

## Quick start

`CodeEditor` owns the document and runs highlighting, find, focus, and language
intelligence internally — integrating is three wires plus registering the
bundled font:

```rust
use iced::time::Instant;
use iced::{Element, Subscription, Task};
use matcha_iced::{CodeEditor, Event};

struct App { editor: CodeEditor }

#[derive(Debug, Clone)]
enum Message { Editor(Event) }

impl App {
    fn new() -> Self { Self { editor: CodeEditor::new("fn main() {}\n") } }
    // `now` comes from iced: run the app with `iced::application::timed`.
    fn update(&mut self, m: Message, now: Instant) -> Task<Message> {
        match m { Message::Editor(e) => self.editor.update(e, now).map(Message::Editor) }
    }
    fn view(&self) -> Element<'_, Message> { self.editor.view().map(Message::Editor) }
    fn subscription(&self) -> Subscription<Message> {
        self.editor.subscription().map(Message::Editor)
    }
}
```

Attach a grammar with `.language(grammar)` (highlighting is coloured at load, no
scroll needed) and override defaults with `.theme(..)`, `.find(..)`,
`.completions(..)`, `.hover(..)`, and `.signature(..)`. For full control, drop to
the low-level `Editor` widget. The host **must** register the bundled font —
`iced::application(..).fonts([matcha_iced::CODICON_FONT])` — so the fold chevrons
and find-bar icons render.

## Syntax highlighting

Highlighting has two backends, each behind a cargo feature of the same name.
You pick one per editor, both produce the same spans, and one `TokenTheme`
colors both.

| feature       | default | grammar                      | the app supplies                                    |
|---------------|---------|------------------------------|-----------------------------------------------------|
| `syntect`     | on      | `matcha_core::SyntaxDef`     | a `.sublime-syntax` file                            |
| `tree-sitter` | off     | `matcha_core::TreeSitterDef` | a grammar crate's `LANGUAGE` and a highlights query |

Both features can be on at once. `matcha-iced` forwards them to `matcha-core`.
With `syntect`, a document of 2 MiB or more tokenizes on worker threads
(natively). A tree-sitter parse runs on the UI thread, a budgeted slice per
frame, and the parse tree stays in memory while the document is open. With
tree-sitter-rust it measured 28 times the text's size on matcha's own sources
and 36 times on a denser generated corpus.

```rust
use matcha_core::{SyntaxDef, TreeSitterDef};
use matcha_iced::CodeEditor;

// syntect
let grammar = SyntaxDef::from_sublime_syntax(include_str!("rust.sublime-syntax"))?;
let editor = CodeEditor::new(source).language(grammar);

// tree-sitter
let grammar = TreeSitterDef::new(tree_sitter_rust::LANGUAGE, tree_sitter_rust::HIGHLIGHTS_QUERY)?;
let editor = CodeEditor::new(source).language(grammar);
```

`tree_sitter_rust::LANGUAGE` goes in as it is, with no `.into()`. `.language`
takes anything that converts into a `matcha_core::Grammar`, and so does
`Document::set_syntax` when you drive the core without the widget. For a
tree-sitter-only build with no syntect in the dependency tree:

```bash
cargo add matcha-iced --no-default-features --features tree-sitter
cargo add matcha-core --no-default-features
cargo add tree-sitter-rust
```

The default theme is the bundled Matcha Dark. `.theme(..)` takes your own
`TokenTheme`, parsed from a `.tmTheme` (`TokenTheme::from_tm_theme`, which
needs `syntect`) or built in code:

```rust
use matcha_core::{Rgba, SpanStyle, TokenTheme};

let rgb = |r, g, b| Rgba { r, g, b, a: 0xff };
let plain = |fg| SpanStyle { fg, bold: false, italic: false };
let theme = TokenTheme::builder()
    .foreground(rgb(0xdf, 0xe1, 0xe6))
    .capture("keyword", plain(rgb(0xec, 0x6a, 0x88)))
    .capture("function", plain(rgb(0xe0, 0xb6, 0x58)))
    .build();
```

Capture names are tree-sitter's, and a dotted name falls back to its prefix:
`function.method` takes `function`'s style unless it has its own.
`TokenTheme::vocabulary()` lists the standard names. Each maps to TextMate
scopes, so a builder theme colors syntect too, and a `.tmTheme` colors
tree-sitter captures. `resolve(name)` returns the style a capture gets, and
`foreground()` the plain-text color.

### Tree-sitter on wasm32

The `tree-sitter` feature builds for `wasm32-unknown-unknown`, but a grammar
crate compiles C. Grammars generated with the tree-sitter CLI 0.26 template or
later build as they are. Older ones, such as tree-sitter-rust 0.24, fail on
`stdlib.h` unless their C compiler sees the libc headers that
`tree-sitter-language` ships. In this repository, run
`eval "$(scripts/wasm-cflags.sh)"` first. Elsewhere, set
`CFLAGS_wasm32_unknown_unknown="-isystem <tree-sitter-language source dir>/wasm/include"`.

## Language servers

Enable the `lsp` feature and `matcha_iced::lsp` re-exports the client. An
`lsp::Client` talks to one server over a connection it owns, and the builder's
last call picks the transport:

| call | transport | targets |
|------|-----------|---------|
| `.stdio(command)` | spawns the server and talks over its stdin and stdout | native |
| `.connect(address)` | TCP: dials a server that listens | native |
| `.listen(socket_addr)` | TCP: waits for the server to dial in | native |
| `.websocket(url)` | `ws://` or `wss://`, one JSON-RPC message per text frame (the `lsp-websocket` feature) | native and browser |
| `.memory(connection)` | an in-process server on an `lsp::lsp_server::Connection` | every target |

The call returns the client and a stream of events. Run the stream as a task
and hand each event back to the client:

```rust
// Boot: one client per server, one open_lsp per editor.
let (mut client, events) = lsp::Client::builder()
    .root(lsp::uri::from_path(&workspace).expect("an absolute path"))
    .stdio(Command::new("rust-analyzer"))?;
editor.open_lsp(&mut client, &uri, "rust")?;
let task = Task::run(events, Message::Lsp);

// After every editor update: sync. The client sends what the edit needs.
let task = editor.update(event, now).map(Message::Editor);
let synced = editor.sync_lsp(&mut client);
// synced.jump: a hint's label part in another tab — call that editor's jump().

// For every event from the stream.
for update in client.receive(event) {
    match update {
        lsp::Update::Document(document) => {
            // Route to the editor whose document().doc_id() == document.doc_id().
            let applied = editor.apply_lsp(&mut client, document);
            // applied.jump: a definition in another tab — call that editor's jump().
        }
        lsp::Update::Status(status) => {}   // Starting, Running, Restarting, Stopped
        lsp::Update::Log(entry) => {}       // stderr, window/logMessage, window/showMessage
        lsp::Update::Error(error) => {}     // a reply that didn't decode, a failed request
        lsp::Update::FileEdits(edits) => {} // a rename's edits to a file that isn't open
        lsp::Update::Notification(_) | lsp::Update::Trace(_) => {}
    }
}
```

When the server crashes or the connection drops, requests in flight settle
empty at once, and the client brings the server back with the editors still
attached: stdio spawns it again, `connect` and `websocket` dial again. By
default it gives up when a fifth loss falls within 3 minutes; `.restart(..)`
changes that, and `client.restart()` tries again by hand. An in-process server
and a `listen` client are never brought back.

`client.shutdown()` sends `shutdown` and `exit`. Over stdio, TCP and WebSocket
the client then gives the server a grace period to go before it kills the
process or closes the connection; an in-process server gets both messages
without any wait. Hand the documents `shutdown()` returns to `apply_lsp`, and
exit the app on `Status::Stopped(Reason::Shutdown)`. Dropping the client sends
the same goodbye; over stdio, TCP and WebSocket a worker thread runs it, and
the process exit can cut it short.

Inlay hints are opt-in per editor: `.inlay_hints(true)`, or `set_inlay_hints` at
runtime. The editor schedules the fetches itself, and `sync_lsp` sends them.
Hovering a hint shows its tooltip, Ctrl+click on a part jumps, and a
double-click inserts the hint's text. The library binds no toggle key.

`examples/lsp` runs two tabs against a scripted in-process server and shows
the traffic. `crates/matcha-lsp/README.md` covers the client in depth.

## Examples

```bash
cargo run -p matcha-iced --example minimal   # the CodeEditor quick start above
cargo run -p matcha-iced --features lsp,tree-sitter --example scratch   # rust-analyzer, on a scratch crate
cargo run -p matcha-iced --features lsp,tree-sitter --example scratch -- path/to/file.rs   # …or on your file
cargo run -p matcha-iced --features tree-sitter --example tree_sitter   # the quick start with a tree-sitter grammar
cargo run -p matcha-iced --features lsp --example lsp   # two tabs on a scripted language server
```

`scratch` opens a real editor on rust-analyzer — type, select, find (Ctrl+F),
fold, and undo/redo, with hover, completion, signature help, F12 and F2. Without
a path it opens a sample Rust document in a scratch crate it writes to the temp
directory. It needs rust-analyzer on `PATH`; without it, the window runs an
in-process demo server and the status line says why. It shows the server's
status and latest log line under the editor, restarts it if it crashes, and
shuts it down when the window closes. Ctrl+S (Cmd+S on macOS) saves, and
rust-analyzer's `cargo check` diagnostics refresh then; the button by the status
line turns check-on-save off and on. Ctrl+I toggles the inlay hints, and
Ctrl+Shift+W the whitespace marks: `·` for a space, `→` for a tab. `lsp` shows
the traffic panel next to the editor. Press F12 on `greet`, F2 to rename it
across both files, or Shift+Alt+F to format. Its main.rs shows inlay hints:
Ctrl+click `name:` to jump to the parameter, double-click `: String` to insert
it, and Ctrl+I to turn them off and on.

## Web (wasm32)

All three crates build for `wasm32-unknown-unknown`, and every example runs in
the browser; there `scratch` talks to the demo server. The web has no system
fonts, so on wasm32 the editor's default font is the bundled Fira Code ([`FIRA_CODE_FONT`](https://docs.rs/matcha-iced/latest/matcha_iced/constant.FIRA_CODE_FONT.html)), with its ligatures,
which `required_fonts()` includes there. Enable iced's `fira-sans` feature for
UI text such as the find bar, and `webgl` for browsers without WebGPU. There
are no threads, so a large document is highlighted on the UI thread instead of
the background pool. The LSP client works in the browser over a WebSocket
(`lsp-websocket`) or against an in-process server; stdio and TCP are native
only.

Tree-sitter grammars build for the browser too, with one caveat for older
grammar crates; see [Tree-sitter on wasm32](#tree-sitter-on-wasm32).

Serve an example as a web app with [Trunk](https://trunkrs.dev) (it fetches
the matching `wasm-bindgen` itself):

```bash
cd crates/matcha-iced
trunk serve --release --example minimal
trunk serve --release --example lsp --features lsp
eval "$(../../scripts/wasm-cflags.sh)"   # tree-sitter-rust needs the wasm libc headers
trunk serve --release --example scratch --features lsp,tree-sitter
```

## License

MIT.
