//! `scratch` — the development window used to review the editor by eye.
//!
//! ```text
//! cargo run -p matcha-iced --features lsp,tree-sitter --example scratch                              # rust-analyzer on a scratch crate
//! cargo run -p matcha-iced --features lsp,tree-sitter --example scratch -- path/to/file.rs          # …or on your file
//! cargo run -p matcha-iced --features lsp,tree-sitter --example scratch -- --capture out.png         # headless PNG
//! cargo run -p matcha-iced --features lsp,tree-sitter --example scratch -- --capture-folds out.png   # …all folds collapsed
//! cargo run -p matcha-iced --features lsp,tree-sitter --example scratch -- --capture-find out.png    # …find+replace bar open
//! cargo run -p matcha-iced --features lsp,tree-sitter --example scratch -- --capture-whitespace out.png  # whitespace marks
//! cargo run -p matcha-iced --features lsp,tree-sitter --example scratch -- --large 100               # a 100 MB document
//! ```
//!
//! The editor highlights with tree-sitter. The native window runs rust-analyzer over stdio, so
//! rust-analyzer must be on `PATH` (`rustup component add rust-analyzer`). With a path, the
//! workspace root is the nearest directory above it holding a `Cargo.toml`. Without one, a
//! scratch crate whose `src/main.rs` is the sample is written to the system temp directory and
//! that file opened. When rust-analyzer does not start, the window runs the demo server instead
//! and the status line says why.
//!
//! In the window, Ctrl+S (Cmd+S on macOS) writes the file and tells the server; rust-analyzer
//! then re-runs `cargo check`, and the button by the status line turns that off and on through
//! `Client::configure`. Ctrl+Shift+W (Cmd+Shift+W) shows or hides the whitespace marks: `·` for
//! a space, `→` for a tab. Ctrl+I (Cmd+I) turns the inlay hints off and on. The line under the
//! status shows the server's latest log line. Closing the window shuts the server down first.
//!
//! The captures, `record_showcase` and the browser window use the demo language server
//! (`shared/demo_server.rs`), which runs in-process on the client's memory transport. It answers
//! from the document's text: an error on each `FIXME` and a warning on each `TODO`, completion,
//! hover, signature help, F12, F2 rename, Shift+Alt+F formatting, and inlay hints.
//!
//! The captures render the editor alone, against the demo server with a frozen clock: every
//! request it would make is answered before the frame, so a capture is identical on every run.
//!
//! `--capture-folds` is the fold-geometry verification harness: it collapses
//! every collapsible pair before rendering, so a refactor of the fold/display
//! projections can be byte-diffed against a baseline PNG (chips, collapsed
//! tails, gutter fold gaps all on screen at once).
//!
//! `--capture-whitespace` renders a dedicated sample with the marks on: tabs
//! after 0–3 cells, runs of spaces, trailing spaces, and a folded block and
//! inline array, so the marks sit beside a chip and on a header's tail.
//!
//! `--large <MB>` opens a synthetic document of that size with the syntect grammar and no
//! language server: the large-document stress case for the syntect worker pool.

// On Windows, a release build is a GUI app with no console window; debug builds
// keep the console so panics and the `--capture` messages stay visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[path = "shared/capture.rs"]
mod capture;
#[path = "shared/demo_server.rs"]
mod demo_server;

use iced::futures::{FutureExt, StreamExt};
use iced::keyboard::{self, Key};
use iced::time::Instant;
use iced::widget::{column, container, text, Row};
use iced::{Element, Fill, Subscription, Task, Theme};

use matcha_core::{Grammar, Granularity, SyntaxDef, TreeSitterDef};
use matcha_iced::{lsp, Action, CodeEditor, Event};

/// The `--large <MB>` corpus, generated in `main` before the app starts.
static LARGE_DOC: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Where the demo server files the document.
const DEMO_URI: &str = "file:///demo/scratch.rs";

/// Synthetic Rust-shaped text for `--large <MB>` — the large-document stress
/// corpus. Duplicates the bench generator's SHAPE (benches can't be
/// imported across crates): nested `fn` / `match` blocks, comments, `return`
/// needles, ~9 lines and 8 bracket pairs per ~250 B block.
fn gen_large(mb: usize) -> String {
    let target = mb * 1_048_576;
    let mut rng = 7u64;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    let mut s = String::with_capacity(target + 512);
    let mut i = 0usize;
    while s.len() < target {
        s.push_str(&format!("// block {i}: canned readings for id 0x{:02X}\n", next() & 0xFF));
        s.push_str(&format!("fn read_{i}(id: u8) -> u8 {{\n    match id {{\n"));
        for _ in 0..3 {
            s.push_str(&format!("        0x{:02X} => {{ return {}; }}\n", next() & 0xFF, next() % 200));
        }
        s.push_str("        _ => { return 0; }\n    }\n}\n\n");
        i += 1;
    }
    s
}

/// A document taller than the window, so scrolling is visible. Line numbers are
/// embedded in the text so it's obvious which rows the viewport is showing.
pub fn long_sample() -> String {
    // A small, self-contained Rust program chosen to exercise the editor: nested
    // foldable blocks (impl / fn / match), inline literals, generics, doc comments,
    // and the hover / signature / completion vocabulary.
    r#"//! A fixed-capacity window over the most recent sensor readings, with a
//! rolling average — a small, self-contained example.

use std::collections::VecDeque;

/// The most recent readings, oldest first, capped at `capacity`.
pub struct Samples {
    values: VecDeque<f64>,
    capacity: usize,
}

impl Samples {
    /// Create an empty window that keeps at most `capacity` readings.
    pub fn new(capacity: usize) -> Self {
        Samples {
            values: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Record one reading, evicting the oldest once the window is full.
    pub fn push(&mut self, reading: f64) {
        if self.values.len() == self.capacity {
            self.values.pop_front();
        }
        self.values.push_back(reading);
    }

    /// The mean of the retained readings, or `None` when the window is empty.
    pub fn average(&self) -> Option<f64> {
        match self.values.len() {
            0 => None,
            n => {
                let total: f64 = self.values.iter().sum();
                Some(total / n as f64)
            }
        }
    }
}

fn main() {
    let mut window = Samples::new(4);
    for reading in [21.5, 22.0, 23.25, 24.0, 25.5] {
        window.push(reading);
        println!("average = {:?}", window.average());
    }
}
"#
    .to_string()
}

/// The `--capture-whitespace` document: every way whitespace lands in a row.
/// `fold_all` collapses both bracket pairs, so marks sit on the block header's
/// `} // end` tail and on both sides of the inline `[ … ]` chip.
const WHITESPACE_SAMPLE: &str = concat!(
    "// Whitespace marks: a dot per space, an arrow per tab.\n",
    "\tafter 0 cells\n",
    "a\tafter 1 cell\n",
    "ab\tafter 2 cells\n",
    "abc\tafter 3 cells\n",
    "let  spaced   out    = 4;\n",
    "let trailing = 1;   \n",
    "fn folded() {\n",
    "    let hidden = 2;\n",
    "} // end  \n",
    "let list = [1,  2,\t3];  \n",
    "\t \tmixed \t indent\n",
);


/// The demo application: one [`CodeEditor`] and, except for `--large`, the language server
/// behind it.
pub struct App {
    editor: CodeEditor,
    lsp: Option<Lsp>,
    /// The window's one-line status: the server's state and what the last request did.
    status: String,
    /// The server's latest stderr or `window/logMessage` line.
    log: String,
    /// Whether inlay hints show; Ctrl+I flips it.
    hints: bool,
    /// Whether the editor draws whitespace marks; Ctrl+Shift+W flips it.
    show_whitespace: bool,
    /// Where Ctrl+S writes the document.
    #[cfg(not(target_family = "wasm"))]
    file: Option<std::path::PathBuf>,
    /// Set by the close request: the window closes once the server has shut down.
    #[cfg(not(target_family = "wasm"))]
    closing: bool,
    /// Whether rust-analyzer runs `cargo check` on save; the button flips it.
    #[cfg(not(target_family = "wasm"))]
    check_on_save: bool,
}

/// The client and the server it talks to.
struct Lsp {
    client: lsp::Client,
    server: Server,
}

enum Server {
    /// rust-analyzer, a child process the client started.
    #[cfg(not(target_family = "wasm"))]
    RustAnalyzer,
    /// The in-process demo server, which answers only when stepped. `fallback` says why it
    /// stands in for rust-analyzer, when it does.
    Demo { scripted: demo_server::Scripted, fallback: Option<String> },
}

/// The app message: it only ever *maps* the editor's opaque [`Event`] (never
/// matches on it), plus the client's events and the app's own chords.
#[derive(Debug, Clone)]
pub enum Message {
    /// A message from the editor.
    Editor(Event),
    /// An event from the client's stream.
    Lsp(lsp::client::Event),
    /// Show or hide the whitespace marks.
    ToggleWhitespace,
    /// Turn inlay hints off or on.
    ToggleHints,
    /// Write the document to its file and tell the server.
    #[cfg(not(target_family = "wasm"))]
    Save,
    /// Turn `cargo check` on save off or on, reconfiguring rust-analyzer.
    #[cfg(not(target_family = "wasm"))]
    ToggleCheck,
    /// The window's close button: shut the server down first.
    #[cfg(not(target_family = "wasm"))]
    CloseRequested,
}

impl Lsp {
    /// Lets the server answer what the client sent; returns whether it had anything to answer.
    fn step(&mut self) -> bool {
        match &mut self.server {
            #[cfg(not(target_family = "wasm"))]
            Server::RustAnalyzer => false,
            Server::Demo { scripted, .. } => scripted.step(),
        }
    }
}

impl Server {
    fn name(&self) -> &'static str {
        match self {
            #[cfg(not(target_family = "wasm"))]
            Server::RustAnalyzer => native::SERVER,
            Server::Demo { .. } => "demo server",
        }
    }

    /// The status line for where this server stands, with why a fallback stands in.
    fn described(&self, status: &lsp::client::Status) -> String {
        let line = described(self.name(), status);
        match self {
            Server::Demo { fallback: Some(reason), .. } => format!("{line} — {reason}"),
            _ => line,
        }
    }
}

impl App {
    /// The demo editor over `text`, talking to the demo server, settled at `now`: the
    /// handshake is done, and the first diagnostics and hints have landed. Run the returned
    /// stream (or [`settle`](Self::settle) on it) to keep the conversation going.
    pub fn demo(text: &str, now: Instant) -> (Self, lsp::client::Events) {
        Self::demo_on(text, uri("file:///demo/"), &uri(DEMO_URI), None, now)
    }

    /// [`demo`](Self::demo) with the document filed at `file` in the workspace `root`, and
    /// `fallback` naming why the demo server stands in for another.
    fn demo_on(
        text: &str,
        root: lsp::lsp_types::Uri,
        file: &lsp::lsp_types::Uri,
        fallback: Option<String>,
        now: Instant,
    ) -> (Self, lsp::client::Events) {
        let (near, far) = lsp::lsp_server::Connection::memory();
        let (mut client, mut events) = lsp::Client::builder().root(root).memory(near);
        let mut editor = Self::editor(text, rust());
        editor.open_lsp(&mut client, file, "rust").expect("the client has no other document");
        let server = Server::Demo { scripted: demo_server::Scripted::new(far), fallback };
        let mut app = Self::with(editor, Some(Lsp { client, server }));
        app.status = "demo server".to_owned();
        app.settle(&mut events, now);
        (app, events)
    }

    /// `text` with the syntect grammar and no language server: the `--large` stress case.
    pub fn large(text: &str) -> Self {
        let grammar = SyntaxDef::from_sublime_syntax(include_str!("assets/rust.sublime-syntax"))
            .expect("bundled Rust grammar parses");
        let mut app = Self::with(Self::editor(text, grammar), None);
        app.status = "no language server".to_owned();
        app
    }

    fn with(editor: CodeEditor, lsp: Option<Lsp>) -> Self {
        Self {
            editor,
            lsp,
            status: String::new(),
            log: String::new(),
            hints: true,
            show_whitespace: false,
            #[cfg(not(target_family = "wasm"))]
            file: None,
            #[cfg(not(target_family = "wasm"))]
            closing: false,
            #[cfg(not(target_family = "wasm"))]
            check_on_save: true,
        }
    }

    /// The editor every mode shares. It sets no completion, hover or signature provider:
    /// those requests go to the language server.
    fn editor(text: &str, grammar: impl Into<Grammar>) -> CodeEditor {
        CodeEditor::new(text)
            .language(grammar)
            .line_comment(Some("//"))
            // Rust: `"` strings; char literals off (`'` is also a lifetime marker),
            // so brackets inside a string like `"{:?}"` are not matched/coloured.
            .bracket_lexing(vec![b'"'], None)
            .rename(true)
            .inlay_hints(true)
    }

    /// `now` is the instant iced stamps on the message (the app runs through
    /// `iced::application::timed`), so the app never reads the clock itself.
    pub fn update(&mut self, message: Message, now: Instant) -> Task<Message> {
        let task = match message {
            Message::Editor(event) => {
                let task = self.editor.update(event, now).map(Message::Editor);
                if let Some(Lsp { client, .. }) = &mut self.lsp {
                    let synced = self.editor.sync_lsp(client);
                    self.follow(synced.jump);
                }
                task
            }
            Message::Lsp(event) => {
                if let Some(Lsp { client, server }) = &mut self.lsp {
                    let mut jumps = Vec::new();
                    for update in client.receive(event) {
                        if let Some(line) = headline(&update) {
                            self.status = line;
                            continue;
                        }
                        match update {
                            lsp::Update::Document(document) => {
                                let applied = self.editor.apply_lsp(client, document);
                                if let Some(refusal) = applied.refused {
                                    self.status = format!("refused: {refusal}");
                                }
                                jumps.push(applied.jump);
                            }
                            lsp::Update::FileEdits(edits) => {
                                self.status = format!("rename skipped {}, which is not open", edits.uri().as_str());
                            }
                            lsp::Update::Status(status) => {
                                #[cfg(not(target_family = "wasm"))]
                                if self.closing && status == lsp::client::Status::Stopped(lsp::client::Reason::Shutdown) {
                                    return iced::exit();
                                }
                                // A dead server's progress never ends, so every status replaces
                                // the progress headline.
                                self.status = server.described(&status);
                            }
                            lsp::Update::Error(error) => self.status = format!("error: {error}"),
                            lsp::Update::Log(entry) => {
                                if let Some(line) = entry.text().lines().next() {
                                    self.log = line.to_owned();
                                }
                            }
                            lsp::Update::Notification(_) | lsp::Update::Trace(_) => {}
                        }
                    }
                    for jump in jumps {
                        self.follow(jump);
                    }
                }
                Task::none()
            }
            Message::ToggleWhitespace => {
                self.show_whitespace = !self.show_whitespace;
                self.editor.set_show_whitespace(self.show_whitespace);
                Task::none()
            }
            Message::ToggleHints => {
                self.hints = !self.hints;
                self.editor.set_inlay_hints(self.hints);
                self.status = format!("inlay hints {}", if self.hints { "on" } else { "off" });
                Task::none()
            }
            #[cfg(not(target_family = "wasm"))]
            Message::Save => {
                self.save();
                Task::none()
            }
            #[cfg(not(target_family = "wasm"))]
            Message::ToggleCheck => {
                self.toggle_check();
                Task::none()
            }
            #[cfg(not(target_family = "wasm"))]
            Message::CloseRequested => self.close(),
        };
        if let Some(lsp) = &mut self.lsp {
            lsp.step();
        }
        task
    }

    /// Select a definition the server located, or say where it is when no editor holds it.
    fn follow(&mut self, mut jump: Option<lsp::update::Jump>) {
        let Some(Lsp { client, .. }) = &mut self.lsp else { return };
        while let Some(next) = jump.take() {
            match next {
                lsp::update::Jump::Open(open) => match self.editor.jump(client, open) {
                    Ok(applied) => jump = applied.jump,
                    Err(refusal) => self.status = format!("jump refused: {refusal}"),
                },
                lsp::update::Jump::Unopened(unopened) => {
                    self.status = format!("definition in {}, which is not open", unopened.uri().as_str());
                }
            }
        }
    }

    /// Runs the conversation with the server to its end at `now`, without a runtime: the server
    /// answers, every event on `events` is handled, and the hint fetch the editor is waiting to
    /// make is made at once instead of after its delay. Captures, `record_showcase` and the
    /// tests call it after their scripted input.
    pub fn settle(&mut self, events: &mut lsp::client::Events, now: Instant) {
        loop {
            let stepped = self.lsp.as_mut().is_some_and(Lsp::step);
            let mut drained = false;
            while let Some(Some(event)) = events.next().now_or_never() {
                drained = true;
                let _ = self.update(Message::Lsp(event), now);
            }
            if stepped || drained {
                continue;
            }
            // A wake clears the wait it answers, and only an edit, a scroll or a toggle sets
            // another, none of which the loop makes, so this ends.
            match self.editor.pending_wake() {
                Some(wake) => {
                    let _ = self.update(Message::Editor(Event::Editor(Action::Wake(wake.generation))), now);
                }
                None => break,
            }
        }
    }

    /// The window: the editor over the status row and the server's latest log line.
    pub fn view(&self) -> Element<'_, Message> {
        let status = text(self.status.as_str()).size(12).font(matcha_iced::DEFAULT_FONT);
        let log = text(self.log.as_str()).size(12).font(matcha_iced::DEFAULT_FONT);
        let bar = Row::with_children(self.check_button()).push(status).spacing(8);
        column![
            container(self.editor_view()).width(Fill).height(Fill),
            container(bar).padding([2, 8]),
            container(log).padding([0, 8]),
        ]
        .into()
    }

    /// The check-on-save button, shown only for rust-analyzer.
    fn check_button(&self) -> Option<Element<'_, Message>> {
        #[cfg(not(target_family = "wasm"))]
        if matches!(self.lsp, Some(Lsp { server: Server::RustAnalyzer, .. })) {
            let label = if self.check_on_save { "check on save: on" } else { "check on save: off" };
            return Some(iced::widget::button(text(label).size(12)).on_press(Message::ToggleCheck).into());
        }
        None
    }

    /// The editor alone, as the captures render it.
    pub fn editor_view(&self) -> Element<'_, Message> {
        self.editor.view().map(Message::Editor)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            self.editor.subscription().map(Message::Editor),
            keyboard::listen().filter_map(chord),
            close_requests(),
        ])
    }
}

#[cfg(not(target_family = "wasm"))]
fn close_requests() -> Subscription<Message> {
    iced::window::close_requests().map(|_| Message::CloseRequested)
}

#[cfg(target_family = "wasm")]
fn close_requests() -> Subscription<Message> {
    Subscription::none()
}

/// The app's chords, each with Ctrl (Cmd on macOS), without Alt and not repeated: Shift+W
/// toggles the whitespace marks, I the inlay hints, and S saves. The editor binds none of them,
/// so they reach `keyboard::listen`.
fn chord(event: keyboard::Event) -> Option<Message> {
    let keyboard::Event::KeyPressed { key: Key::Character(c), modifiers, repeat: false, .. } = event else {
        return None;
    };
    if !modifiers.command() || modifiers.alt() {
        return None;
    }
    match (c.to_ascii_lowercase().as_str(), modifiers.shift()) {
        ("w", true) => Some(Message::ToggleWhitespace),
        ("i", false) => Some(Message::ToggleHints),
        #[cfg(not(target_family = "wasm"))]
        ("s", false) => Some(Message::Save),
        _ => None,
    }
}

/// The status line for where the server called `server` stands.
fn described(server: &str, status: &lsp::client::Status) -> String {
    match status {
        lsp::client::Status::Starting => format!("starting {server}…"),
        lsp::client::Status::Running => format!("{server} ready"),
        lsp::client::Status::Restarting { attempt } => {
            format!("{server} stopped; restarting (attempt {attempt})…")
        }
        lsp::client::Status::Stopped(reason) => format!("{server} stopped: {}", stopped(reason)),
    }
}

/// Why the server stopped, for a person.
fn stopped(reason: &lsp::client::Reason) -> String {
    match reason {
        lsp::client::Reason::Shutdown => "shut down".to_owned(),
        lsp::client::Reason::Closed => "it closed the connection".to_owned(),
        lsp::client::Reason::Initialize => "it failed to initialize".to_owned(),
        lsp::client::Reason::Failed(error) => format!("the connection failed: {error}"),
        lsp::client::Reason::Exited { signal: Some(signal), .. } => format!("it was killed by signal {signal}"),
        lsp::client::Reason::Exited { code: Some(code), .. } => format!("it exited with code {code}"),
        lsp::client::Reason::Exited { code: None, signal: None } => "it exited".to_owned(),
        lsp::client::Reason::Unresponsive => "it stopped reading its input".to_owned(),
        lsp::client::Reason::GaveUp => "it kept crashing, so it was given up on".to_owned(),
        lsp::client::Reason::Timeout => "it did not answer initialize in time".to_owned(),
    }
}

/// The status line an update earns: a message the server asked to show, and the title of a
/// `$/progress` report.
fn headline(update: &lsp::Update) -> Option<String> {
    match update {
        lsp::Update::Log(entry) if entry.is_shown() => Some(entry.text().to_owned()),
        lsp::Update::Notification(notification) if notification.method() == "$/progress" => {
            let value = notification.params()?.get("value")?;
            let title = value.get("title").and_then(|title| title.as_str());
            let message = value.get("message").and_then(|message| message.as_str());
            match (title, message) {
                (Some(title), Some(message)) => Some(format!("{title}: {message}")),
                (Some(line), None) | (None, Some(line)) => Some(line.to_owned()),
                (None, None) => None,
            }
        }
        lsp::Update::Document(_)
        | lsp::Update::FileEdits(_)
        | lsp::Update::Status(_)
        | lsp::Update::Log(_)
        | lsp::Update::Error(_)
        | lsp::Update::Notification(_)
        | lsp::Update::Trace(_) => None,
    }
}

fn uri(text: &str) -> lsp::lsp_types::Uri {
    text.parse().expect("the demo's URIs parse")
}

/// The tree-sitter Rust grammar.
fn rust() -> TreeSitterDef {
    TreeSitterDef::new(tree_sitter_rust::LANGUAGE, tree_sitter_rust::HIGHLIGHTS_QUERY)
        .expect("tree-sitter-rust's highlight query compiles")
}

/// An original dark theme for the demo's iced chrome (find bar, popups,
/// scrollbar), matching the `Matcha Dark` syntax theme.
pub fn matcha_dark() -> Theme {
    use iced::theme::palette::Seed;
    use iced::Color;
    Theme::custom(
        "Matcha Dark".to_string(),
        Seed {
            background: Color::from_rgb8(0x1c, 0x1e, 0x24), // #1C1E24
            text: Color::from_rgb8(0xdf, 0xe1, 0xe6),       // #DFE1E6
            primary: Color::from_rgb8(0xec, 0x6a, 0x88),    // rose accent (caret/selection)
            success: Color::from_rgb8(0xa3, 0xc7, 0x6d),    // green
            warning: Color::from_rgb8(0xe0, 0xb6, 0x58),    // amber
            danger: Color::from_rgb8(0xe0, 0x57, 0x5b),     // red
        },
    )
}

fn theme(_state: &App) -> Theme {
    matcha_dark()
}

/// The native window: rust-analyzer over stdio, on a file in its crate.
#[cfg(not(target_family = "wasm"))]
mod native {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use iced::time::Instant;
    use iced::Task;
    use serde_json::json;

    use super::{described, lsp, App, Lsp, Message, Server};

    /// The command that starts rust-analyzer, looked up on `PATH`.
    pub const SERVER: &str = "rust-analyzer";

    /// Where the scratch crate goes when no file is given, under the system temp directory.
    pub const SCRATCH: &str = "matcha-scratch-example";

    const SCRATCH_MANIFEST: &str =
        "[package]\nname = \"scratch\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n";

    /// The file to edit and the crate it belongs to.
    pub struct Workspace {
        root_uri: lsp::lsp_types::Uri,
        pub file: PathBuf,
        file_uri: lsp::lsp_types::Uri,
        text: String,
    }

    impl Workspace {
        /// The file named by the first of `args` (after the program's) that is neither a flag
        /// nor a flag's value, or the scratch crate holding the sample when there is none.
        pub fn from_args(args: &[String]) -> io::Result<Self> {
            match file_arg(args) {
                Some(file) => Self::open(Path::new(file)),
                None => Self::scratch(&std::env::temp_dir().join(SCRATCH), &super::long_sample()),
            }
        }

        /// `file`, in the crate of the nearest directory above it that holds a `Cargo.toml`.
        fn open(file: &Path) -> io::Result<Self> {
            let file = std::path::absolute(file)?;
            let text = std::fs::read_to_string(&file)
                .map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", file.display())))?;
            let root = file.ancestors().skip(1).find(|dir| dir.join("Cargo.toml").is_file()).ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, format!("no Cargo.toml above {}", file.display()))
            })?;
            let uri = |path: &Path| {
                lsp::uri::from_path(path).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, format!("{} is not UTF-8", path.display()))
                })
            };
            Ok(Self { root_uri: uri(root)?, file_uri: uri(&file)?, file, text })
        }

        /// Writes a crate whose `src/main.rs` is `main_rs` into `root`, replacing an earlier one,
        /// and opens that file.
        pub fn scratch(root: &Path, main_rs: &str) -> io::Result<Self> {
            std::fs::create_dir_all(root.join("src"))?;
            std::fs::write(root.join("Cargo.toml"), SCRATCH_MANIFEST)?;
            std::fs::write(root.join("src/main.rs"), main_rs)?;
            Self::open(&root.join("src/main.rs"))
        }
    }

    /// The first of `args` after the program's that is neither a flag nor a flag's value.
    pub fn file_arg(args: &[String]) -> Option<&str> {
        let mut rest = args.iter().skip(1).map(String::as_str).peekable();
        while let Some(arg) = rest.next() {
            if arg == "--large" {
                rest.next_if(|mb| mb.parse::<usize>().is_ok());
            } else if arg.starts_with("--capture") {
                rest.next();
            } else if !arg.starts_with("--") {
                return Some(arg);
            }
        }
        None
    }

    /// The app on the server `command` starts, opened on `workspace`, and its client's event
    /// stream, before anything runs it. When the server does not start, the demo server stands
    /// in and the status line says why.
    pub fn boot(workspace: &Workspace, command: Command) -> (App, lsp::client::Events) {
        let program = command.get_program().to_string_lossy().into_owned();
        let started = lsp::Client::builder()
            .root(workspace.root_uri.clone())
            .configuration(json!({ "rust-analyzer": { "checkOnSave": true } }))
            .stdio(command);
        let (mut app, events) = match started {
            Ok((mut client, events)) => {
                let mut editor = App::editor(&workspace.text, super::rust());
                // Before the handshake this only records the text; the didOpen follows `initialized`.
                editor.open_lsp(&mut client, &workspace.file_uri, "rust").expect("the client has no other document");
                let mut app = App::with(editor, Some(Lsp { client, server: Server::RustAnalyzer }));
                app.status = described(SERVER, &lsp::client::Status::Starting);
                (app, events)
            }
            Err(error) => {
                let reason = describe(&program, &error);
                eprintln!("scratch: {reason}; running the demo server instead");
                App::demo_on(&workspace.text, workspace.root_uri.clone(), &workspace.file_uri, Some(reason), Instant::now())
            }
        };
        app.file = Some(workspace.file.clone());
        (app, events)
    }

    /// Opens the window on `workspace`, with rust-analyzer or its fallback behind it.
    pub fn run(workspace: Workspace) -> iced::Result {
        let title = format!("matcha — scratch — {}", workspace.file.display());
        // iced's boot is `Fn`, so the app moves out of a cell the one time it runs.
        let booted = std::cell::Cell::new(Some(boot(&workspace, Command::new(SERVER))));
        super::window(title, move || {
            let (app, events) = booted.take().expect("iced boots the app once");
            (app, Task::run(events, Message::Lsp))
        })
    }

    /// Why `program` did not start, for a person.
    pub fn describe(program: &str, error: &lsp::client::builder::Error) -> String {
        // Not a match: the `websocket` feature adds variants this example never sees.
        if let lsp::client::builder::Error::Spawn(spawn) = error {
            if spawn.kind() == io::ErrorKind::NotFound {
                return format!("{program} is not on PATH (`rustup component add rust-analyzer` installs rust-analyzer)");
            }
        }
        format!("{program} did not start: {error}")
    }

    impl App {
        /// Writes the document to its file and tells the server.
        pub(super) fn save(&mut self) {
            let Some(file) = &self.file else {
                self.status = "no file to save to".to_owned();
                return;
            };
            let doc = self.editor.document();
            // The buffer holds LF; a CRLF file is written back as CRLF.
            if let Err(error) = std::fs::write(file, doc.serialize(doc.buffer().eol_flavor())) {
                self.status = format!("could not save {}: {error}", file.display());
                return;
            }
            let name = file.file_name().unwrap_or(file.as_os_str()).display().to_string();
            let mut checking = false;
            let mut jump = None;
            if let Some(Lsp { client, server }) = &mut self.lsp {
                jump = self.editor.save_lsp(client).jump;
                checking = matches!(server, Server::RustAnalyzer)
                    && self.check_on_save
                    && client.status() == &lsp::client::Status::Running;
            }
            self.follow(jump);
            self.status = if checking { format!("saved {name} — cargo check running…") } else { format!("saved {name}") };
        }

        pub(super) fn toggle_check(&mut self) {
            let Some(Lsp { client, server: Server::RustAnalyzer }) = &mut self.lsp else { return };
            self.check_on_save = !self.check_on_save;
            client.configure(json!({ "rust-analyzer": { "checkOnSave": self.check_on_save } }));
            self.status = format!("check on save {}", if self.check_on_save { "on" } else { "off" });
        }

        /// Shuts the server down; the window closes once it has, or at once when there is none
        /// left to shut down.
        pub(super) fn close(&mut self) -> Task<Message> {
            let Some(Lsp { client, .. }) = &mut self.lsp else { return iced::exit() };
            if matches!(client.status(), lsp::client::Status::Stopped(_)) {
                return iced::exit();
            }
            self.closing = true;
            for document in client.shutdown() {
                let _ = self.editor.apply_lsp(client, document);
            }
            Task::none()
        }
    }
}

/// The app a capture renders, at `now`: `--large`'s corpus without a server, else the demo
/// server on `text`, with the stream [`App::settle`] needs.
fn headless(text: &str, now: Instant) -> (App, Option<lsp::client::Events>) {
    match LARGE_DOC.get() {
        Some(large) => (App::large(large), None),
        None => {
            let (app, events) = App::demo(text, now);
            (app, Some(events))
        }
    }
}

/// Settle `app` and render its editor to `path`.
fn capture(mut app: App, events: Option<lsp::client::Events>, now: Instant, path: &str) {
    if let Some(mut events) = events {
        app.settle(&mut events, now);
    }
    let (w, h) = capture::render_to_png(app.editor_view(), 900, 560, &matcha_dark(), &[], path);
    eprintln!("captured {w}x{h} -> {path}");
}

/// Opens the window titled `title` on the app `boot` makes.
fn window(title: String, boot: impl Fn() -> (App, Task<Message>) + 'static) -> iced::Result {
    let app = iced::application::timed(boot, App::update, App::subscription, App::view)
        .title(move |_: &App| title.clone())
        .theme(theme);
    // The close request shuts the server down before the window goes.
    #[cfg(not(target_family = "wasm"))]
    let app = app.exit_on_close_request(false);
    // Register every font the widget requires (fold chevrons + find-bar icons)
    // through the one owner, so the set can't be loaded piecemeal and leave tofu.
    app.fonts(matcha_iced::required_fonts().iter().copied()).run()
}

fn main() -> iced::Result {
    let args: Vec<String> = std::env::args().collect();
    // `--large <MB>` (default 10): open a synthetic Rust-shaped document of that
    // size instead of the sample — the large-document stress case (the parallel
    // highlight sweep colours it in behind the idle sweep).
    if let Some(i) = args.iter().position(|a| a == "--large") {
        let mb: usize = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(10);
        let _ = LARGE_DOC.set(gen_large(mb));
    }
    // Headless: no iced runtime stamps the messages, so one fixed instant
    // stands in for all of them.
    let now = Instant::now();
    if let Some(i) = args.iter().position(|a| a == "--capture") {
        let path = args.get(i + 1).map_or("scratch.png", String::as_str);
        let (app, events) = headless(&long_sample(), now);
        capture(app, events, now, path);
        return Ok(());
    }
    // Fold-geometry verification harness (see the module doc): collapse every
    // collapsible pair, then render one frame — maximal fold geometry on screen.
    if let Some(i) = args.iter().position(|a| a == "--capture-folds") {
        let path = args.get(i + 1).map_or("scratch-folds.png", String::as_str);
        let (mut app, events) = headless(&long_sample(), now);
        app.editor.fold_all();
        capture(app, events, now, path);
        return Ok(());
    }
    // Find+replace layout harness (see the module doc): open the bar with the
    // replace row expanded over a live query and an active match, then render one
    // frame — chevron, both inputs, the match count, and every button on screen.
    if let Some(i) = args.iter().position(|a| a == "--capture-find") {
        let path = args.get(i + 1).map_or("scratch-find.png", String::as_str);
        let (mut app, events) = headless(&long_sample(), now);
        let ev = |e| Message::Editor(e);
        let _ = app.update(ev(Event::OpenReplace), now); // find + the replace row out
        let _ = app.update(ev(Event::FindQuery("self".into())), now);
        let _ = app.update(ev(Event::ReplaceText("this".into())), now);
        // Latch two of the three options, so both the engaged and the resting
        // toggle style are on screen to compare.
        let _ = app.update(ev(Event::ToggleCase), now);
        let _ = app.update(ev(Event::ToggleWholeWord), now);
        let _ = app.update(ev(Event::TogglePreserveCase), now); // latch the replace box's AB
        // Scope find to a block of the sample, so the scope wash + its latched
        // toggle are on screen too.
        let _ = app.update(ev(Event::Editor(Action::DragSelect {
            granularity: Granularity::Char,
            origin: 0,
            head: 600,
        })), now);
        let _ = app.update(ev(Event::ToggleFindInSelection), now);
        let _ = app.update(ev(Event::FindNext), now); // activate a match ⇒ a real "N of M"
        capture(app, events, now, path);
        return Ok(());
    }

    // Whitespace-mark harness (see the module doc): the dedicated sample with
    // every pair folded and the marks on.
    if let Some(i) = args.iter().position(|a| a == "--capture-whitespace") {
        let path = args.get(i + 1).map_or("scratch-whitespace.png", String::as_str);
        let (mut app, events) = App::demo(WHITESPACE_SAMPLE, now);
        app.editor.fold_all();
        let _ = app.update(Message::ToggleWhitespace, now);
        capture(app, Some(events), now, path);
        return Ok(());
    }

    #[cfg(not(target_family = "wasm"))]
    if LARGE_DOC.get().is_none() {
        match native::Workspace::from_args(&args) {
            Ok(workspace) => return native::run(workspace),
            Err(error) => {
                eprintln!("scratch: {error}");
                std::process::exit(1);
            }
        }
    }
    window("matcha — scratch".to_owned(), boot)
}

/// The window's app without rust-analyzer: `--large`'s corpus, or the sample on the demo server,
/// whose events the runtime delivers from then on.
fn boot() -> (App, Task<Message>) {
    match LARGE_DOC.get() {
        Some(large) => (App::large(large), Task::none()),
        None => {
            let (app, events) = App::demo(&long_sample(), Instant::now());
            (app, Task::run(events, Message::Lsp))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::key::{Code, Physical};
    use iced::keyboard::{Location, Modifiers};
    use matcha_core::Severity;

    /// The demo app on the sample, and the instant every message in a test is stamped with.
    fn booted() -> (App, lsp::client::Events, Instant) {
        let now = Instant::now();
        let (app, events) = App::demo(&long_sample(), now);
        (app, events, now)
    }

    fn act(app: &mut App, action: Action, now: Instant) {
        let _ = app.update(Message::Editor(Event::Editor(action)), now);
    }

    /// Every diagnostic the editor shows, as `(span, severity)`.
    fn diagnostics(app: &App) -> Vec<(std::ops::Range<u32>, Severity)> {
        let document = app.editor.document();
        document.diagnostics_in(0..document.buffer().len()).map(|(span, severity, _)| (span, severity)).collect()
    }

    /// The labels of the hints the editor shows, in render order.
    fn hint_labels(app: &App) -> Vec<String> {
        let document = app.editor.document();
        document
            .inlays_in(0..document.buffer().len())
            .map(|shown| shown.hint().parts().iter().map(|part| part.text()).collect())
            .collect()
    }

    fn scripted(app: &App) -> &demo_server::Scripted {
        match &app.lsp {
            Some(Lsp { server: Server::Demo { scripted, .. }, .. }) => scripted,
            _ => panic!("the demo app has the demo server"),
        }
    }

    #[test]
    fn a_typed_fixme_is_flagged_after_a_settle() {
        let (mut app, mut events, now) = booted();
        assert!(diagnostics(&app).is_empty(), "the sample has no marker");
        let start = app.editor.document().buffer().len();
        act(&mut app, Action::PlaceCaret(start), now);
        for c in "FIXME".chars() {
            act(&mut app, Action::Type(c), now);
        }
        app.settle(&mut events, now);
        assert_eq!(diagnostics(&app), [(start..start + 5, Severity::Error)]);
    }

    /// Edits typed without a settle between them end, once settled, with the server holding the
    /// editor's text and the diagnostics describing it.
    #[test]
    fn a_burst_of_edits_settles_to_current_diagnostics() {
        let (mut app, mut events, now) = booted();
        let start = app.editor.document().buffer().len();
        act(&mut app, Action::PlaceCaret(start), now);
        for c in "// TODO FIXME".chars() {
            act(&mut app, Action::Type(c), now);
        }
        for _ in 0.."FIXME".len() {
            act(&mut app, Action::Backspace, now);
        }
        app.settle(&mut events, now);
        let text = app.editor.document().text().into_owned();
        assert_eq!(scripted(&app).text(DEMO_URI), Some(text.as_str()), "the server saw every edit");
        let todo = start + "// ".len() as u32;
        assert_eq!(diagnostics(&app), [(todo..todo + 4, Severity::Warning)], "only the TODO is left");
    }

    #[test]
    fn the_sample_gets_its_parameter_and_type_hints() {
        let (app, _events, _) = booted();
        assert_eq!(hint_labels(&app), [": Samples", "capacity:"], "`let mut window`, then `Samples::new(4)`");
    }

    #[test]
    fn f12_on_samples_selects_its_struct() {
        let (mut app, mut events, now) = booted();
        let sample = long_sample();
        let call = sample.find("Samples::new").expect("main calls Samples::new") as u32;
        act(&mut app, Action::PlaceCaret(call + 1), now);
        act(&mut app, Action::GotoDefinition, now);
        app.settle(&mut events, now);
        let name = (sample.find("struct Samples").expect("the sample defines Samples") + "struct ".len()) as u32;
        assert_eq!(app.editor.selection(), name..name + "Samples".len() as u32);
    }

    #[test]
    fn large_opens_no_server() {
        let app = App::large(&gen_large(1));
        assert!(app.lsp.is_none());
    }

    /// Ctrl+Shift+W toggles the whitespace marks, Ctrl+I the hints and, natively, Ctrl+S saves
    /// (Cmd on macOS); nothing else does.
    #[test]
    fn chord_matches_ctrl_shift_w_ctrl_i_and_ctrl_s_only() {
        let press = |c: &str, modifiers: Modifiers, repeat: bool| keyboard::Event::KeyPressed {
            key: Key::Character(c.into()),
            modified_key: Key::Character(c.into()),
            physical_key: Physical::Code(match c.to_ascii_lowercase().as_str() {
                "w" => Code::KeyW,
                "i" => Code::KeyI,
                _ => Code::KeyS,
            }),
            location: Location::Standard,
            modifiers,
            text: None,
            repeat,
        };
        assert!(matches!(chord(press("W", Modifiers::COMMAND | Modifiers::SHIFT, false)), Some(Message::ToggleWhitespace)));
        assert!(matches!(chord(press("i", Modifiers::COMMAND, false)), Some(Message::ToggleHints)));
        #[cfg(not(target_family = "wasm"))]
        assert!(matches!(chord(press("s", Modifiers::COMMAND, false)), Some(Message::Save)));
        for (c, modifiers, repeat, what) in [
            ("W", Modifiers::COMMAND, false, "Ctrl+W"),
            ("W", Modifiers::COMMAND | Modifiers::SHIFT | Modifiers::ALT, false, "Ctrl+Shift+Alt+W"),
            ("W", Modifiers::COMMAND | Modifiers::SHIFT, true, "a repeated Ctrl+Shift+W"),
            ("W", Modifiers::SHIFT, false, "a plain W"),
            ("I", Modifiers::COMMAND | Modifiers::SHIFT, false, "Ctrl+Shift+I"),
            ("i", Modifiers::COMMAND | Modifiers::ALT, false, "Ctrl+Alt+I"),
            ("i", Modifiers::COMMAND, true, "a repeated Ctrl+I"),
            ("i", Modifiers::empty(), false, "a plain i"),
            ("S", Modifiers::COMMAND | Modifiers::SHIFT, false, "Ctrl+Shift+S"),
            ("s", Modifiers::COMMAND | Modifiers::ALT, false, "Ctrl+Alt+S"),
            ("s", Modifiers::COMMAND, true, "a repeated Ctrl+S"),
            ("s", Modifiers::empty(), false, "a plain s"),
        ] {
            assert!(chord(press(c, modifiers, repeat)).is_none(), "{what} is no chord");
        }
    }

    /// What a fresh client makes of `message` from its server.
    fn updates(message: serde_json::Value) -> Vec<lsp::Update> {
        let (near, far) = lsp::lsp_server::Connection::memory();
        let (mut client, mut events) = lsp::Client::builder().memory(near);
        let message = serde_json::from_value(message).expect("the fixture is a message");
        far.sender.send(message).expect("the client end is open");
        let mut updates = Vec::new();
        while let Some(Some(event)) = events.next().now_or_never() {
            updates.extend(client.receive(event));
        }
        updates
    }

    /// The status lines `message` earns.
    fn headlines(method: &str, params: serde_json::Value) -> Vec<String> {
        updates(serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .iter()
            .filter_map(headline)
            .collect()
    }

    /// The status line shows a showMessage's text, and a progress report's title and message.
    #[test]
    fn headlines_come_from_show_message_and_progress() {
        assert_eq!(
            headlines("window/showMessage", serde_json::json!({ "type": 3, "message": "hello" })),
            ["hello"],
            "showMessage text"
        );
        assert_eq!(
            headlines(
                "$/progress",
                serde_json::json!({ "token": 1, "value": { "kind": "begin", "title": "Indexing", "message": "1/3" } }),
            ),
            ["Indexing: 1/3"],
            "progress title and message",
        );
        assert!(
            headlines("window/logMessage", serde_json::json!({ "type": 3, "message": "noise" })).is_empty(),
            "log messages stay off the status line"
        );
    }

    /// Every stop reason reads as a sentence on the status line.
    #[test]
    fn stopped_reasons_read_as_sentences() {
        let stop = |reason| described("rust-analyzer", &lsp::client::Status::Stopped(reason));
        assert_eq!(
            stop(lsp::client::Reason::Exited { code: Some(101), signal: None }),
            "rust-analyzer stopped: it exited with code 101",
            "an exit code"
        );
        assert_eq!(
            stop(lsp::client::Reason::Exited { code: None, signal: Some(11) }),
            "rust-analyzer stopped: it was killed by signal 11",
            "a signal"
        );
        assert_eq!(
            stop(lsp::client::Reason::GaveUp),
            "rust-analyzer stopped: it kept crashing, so it was given up on",
            "the policy gave up"
        );
        assert_eq!(
            described("rust-analyzer", &lsp::client::Status::Restarting { attempt: 2 }),
            "rust-analyzer stopped; restarting (attempt 2)…",
            "a restart"
        );
    }

    /// The window's native paths: the file argument, the fallback, and a real rust-analyzer.
    #[cfg(not(target_family = "wasm"))]
    mod native {
        use std::process::Command;
        use std::sync::mpsc;
        use std::time::Duration;

        use matcha_core::{Diagnostic, EditOp};

        use super::super::native::{boot, file_arg, Workspace, SCRATCH, SERVER};
        use super::*;

        /// The crate the real-server tests open: a type error, a function to hover and F12 to,
        /// and a call to retype for signature help.
        const SCRATCH_MAIN: &str = r#"/// Adds two numbers. Hover `add`, or press F12 on a call to it.
fn add(left: i32, right: i32) -> i32 {
    left + right
}

fn main() {
    let sum = add(1, 2);
    // Retype the `(` of this call for signature help.
    let doubled = add(sum, sum);
    // rust-analyzer flags this: an `i32` is not a `String`.
    let label: String = doubled;
    println!("{label}");
}
"#;

        /// A fresh scratch crate holding [`SCRATCH_MAIN`], under a directory named for `test`.
        fn workspace(test: &str) -> (std::path::PathBuf, Workspace) {
            let root = std::env::temp_dir().join(format!("{SCRATCH}-{test}-{}", std::process::id()));
            let workspace = Workspace::scratch(&root, SCRATCH_MAIN).expect("the scratch crate is written");
            (root, workspace)
        }

        #[test]
        fn the_file_argument_skips_flags_and_their_values() {
            let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
            assert_eq!(file_arg(&args(&["scratch"])), None);
            assert_eq!(file_arg(&args(&["scratch", "src/lib.rs"])), Some("src/lib.rs"));
            assert_eq!(file_arg(&args(&["scratch", "--capture", "out.png"])), None);
            assert_eq!(file_arg(&args(&["scratch", "--large", "5"])), None);
            assert_eq!(file_arg(&args(&["scratch", "--large", "a.rs"])), Some("a.rs"), "`--large` takes only a number");
            assert_eq!(file_arg(&args(&["scratch", "--capture-find", "x.png", "b.rs"])), Some("b.rs"));
        }

        #[test]
        fn a_missing_server_falls_back_to_the_demo_server() {
            let (root, workspace) = workspace("fallback");
            let (app, _events) = boot(&workspace, Command::new("matcha-no-such-server"));
            assert!(
                matches!(&app.lsp, Some(Lsp { server: Server::Demo { fallback: Some(_), .. }, .. })),
                "the demo server stands in"
            );
            assert!(app.status.contains("matcha-no-such-server"), "the status names the missing server: {}", app.status);
            assert_eq!(app.file.as_deref(), Some(workspace.file.as_path()), "Ctrl+S still writes the file");
            let _ = std::fs::remove_dir_all(&root);
        }

        fn client(app: &mut App) -> &mut lsp::Client {
            &mut app.lsp.as_mut().expect("the app has a server").client
        }

        /// Tells the server about the editor's edits and requests.
        fn sync(app: &mut App) {
            let Some(Lsp { client, .. }) = &mut app.lsp else { unreachable!("started checks the server") };
            let _ = app.editor.sync_lsp(client);
        }

        /// Starts rust-analyzer on `workspace` and boots the app on it, tracing, with its events
        /// handed over by a thread so a test can wait with a deadline.
        fn started(workspace: &Workspace) -> (App, mpsc::Receiver<lsp::client::Event>) {
            let (mut app, events) = boot(workspace, Command::new(SERVER));
            assert!(matches!(&app.lsp, Some(Lsp { server: Server::RustAnalyzer, .. })), "{}", app.status);
            client(&mut app).set_trace(lsp::trace::Mode::Messages);
            (app, forward(events))
        }

        /// Runs `events` on a thread and hands each event over, so a test can wait with a
        /// deadline.
        fn forward(events: lsp::client::Events) -> mpsc::Receiver<lsp::client::Event> {
            let (sender, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                iced::futures::executor::block_on(events.for_each(|event| {
                    let _ = sender.send(event);
                    std::future::ready(())
                }));
            });
            receiver
        }

        /// The next event folded into the app's client, or `None` once `deadline` passes.
        fn pump(app: &mut App, events: &mpsc::Receiver<lsp::client::Event>, deadline: Instant) -> Option<Vec<lsp::Update>> {
            let timeout = deadline.saturating_duration_since(Instant::now());
            let event = events.recv_timeout(timeout).ok()?;
            Some(client(app).receive(event))
        }

        /// Whether `updates` trace `method` going out.
        fn went_out(updates: &[lsp::Update], method: &str) -> bool {
            updates.iter().any(|update| {
                matches!(update, lsp::Update::Trace(entry)
                    if entry.direction() == lsp::trace::Direction::Outgoing
                        && entry.method() == Some(method))
            })
        }

        /// The diagnostic sets in `updates`, with their stamps, logged as they arrive.
        fn published(
            updates: &[lsp::Update],
            editor: &CodeEditor,
            started: Instant,
        ) -> Vec<(lsp::update::Stamp, Vec<Diagnostic>)> {
            let mut sets = Vec::new();
            for update in updates {
                let lsp::Update::Document(document) = update else {
                    continue;
                };
                assert_eq!(document.doc_id(), editor.document().doc_id(), "the update is for the opened file");
                if let lsp::update::Change::Diagnostics(list) = document.change() {
                    eprintln!(
                        "{:?}: publishDiagnostics at {:?} with {} entries: {:?}",
                        started.elapsed(),
                        document.stamp(),
                        list.len(),
                        list.iter().map(|d| d.message.as_str()).collect::<Vec<_>>(),
                    );
                    sets.push((document.stamp(), list.clone()));
                }
            }
            sets
        }

        /// Whether `list` holds the scratch crate's type error.
        fn mismatched(list: &[Diagnostic]) -> bool {
            list.iter().any(|d| d.message.contains("mismatched"))
        }

        /// Against a real rust-analyzer: the handshake completes, the scratch file's didOpen
        /// goes out, and a `publishDiagnostics` for it comes back through `Client::receive` as
        /// the type error. Once the error is fixed, written to disk and saved, a publish for the
        /// fixed text clears it, and it stays cleared. Then an orderly shutdown ends the process.
        #[test]
        #[ignore = "needs rust-analyzer on PATH and a Rust toolchain; run with --ignored"]
        fn rust_analyzer_reports_the_scratch_crates_type_error_and_clears_it_on_save() {
            let (root, workspace) = workspace("test");
            let (mut app, events) = started(&workspace);

            let started = Instant::now();
            let deadline = started + Duration::from_secs(120);
            let mut did_open = false;
            let mut diagnostics = None;
            while diagnostics.is_none() {
                let Some(updates) = pump(&mut app, &events, deadline) else {
                    break;
                };
                did_open |= went_out(&updates, "textDocument/didOpen");
                diagnostics = published(&updates, &app.editor, started)
                    .into_iter()
                    .map(|(_, list)| list)
                    .find(|list| !list.is_empty());
            }
            assert!(did_open, "the handshake completed and the didOpen went out");
            let diagnostics = diagnostics.expect("diagnostics arrived within the timeout");
            let label = SCRATCH_MAIN.find("label: String").expect("the scratch has the error");
            assert!(
                diagnostics.iter().any(|d| d.span.start as usize >= label && d.message.contains("mismatched")),
                "the type error is reported: {diagnostics:?}",
            );

            let fix = "let label: String = doubled;";
            let at = SCRATCH_MAIN.find(fix).expect("the scratch has the error") + fix.len() - 1;
            app.editor
                .try_edit(vec![EditOp::insert(u32::try_from(at).expect("small"), ".to_string()")])
                .expect("the fix applies");
            let doc = app.editor.document();
            std::fs::write(&workspace.file, doc.serialize(doc.buffer().eol_flavor())).expect("the fixed file is written");
            let fixed = lsp::update::Stamp::Revision(doc.revision());
            let Some(Lsp { client, .. }) = &mut app.lsp else { unreachable!("started checks the server") };
            let _ = app.editor.save_lsp(client);
            eprintln!("{:?}: saved", started.elapsed());

            let deadline = Instant::now() + Duration::from_secs(120);
            let mut did_save = false;
            let mut cleared = false;
            while !cleared {
                let Some(updates) = pump(&mut app, &events, deadline) else {
                    break;
                };
                did_save |= went_out(&updates, "textDocument/didSave");
                cleared = published(&updates, &app.editor, started)
                    .iter()
                    .any(|(stamp, list)| *stamp == fixed && !mismatched(list));
            }
            assert!(did_save, "rust-analyzer asks for saves, so the didSave goes out");
            assert!(cleared, "a publish for the fixed text drops the type error");
            let quiet = Instant::now() + Duration::from_secs(2);
            while let Some(updates) = pump(&mut app, &events, quiet) {
                for (_, list) in published(&updates, &app.editor, started) {
                    assert!(!mismatched(&list), "the type error stays cleared: {list:?}");
                }
            }

            shut_down(&mut app, &events);
            let _ = std::fs::remove_dir_all(&root);
        }

        /// Shut the server down, as closing the window does, and wait for the client to stop.
        fn shut_down(app: &mut App, events: &mpsc::Receiver<lsp::client::Event>) {
            let Some(Lsp { client, .. }) = &mut app.lsp else { unreachable!("started checks the server") };
            for document in client.shutdown() {
                let _ = app.editor.apply_lsp(client, document);
            }
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let updates = pump(app, events, deadline).expect("the client stops after shutdown");
                let stopped = updates.iter().any(|update| {
                    matches!(update, lsp::Update::Status(lsp::client::Status::Stopped(lsp::client::Reason::Shutdown)))
                });
                if stopped {
                    break;
                }
            }
        }

        /// The offsets of the hints `editor` shows.
        fn hint_offsets(editor: &CodeEditor) -> Vec<u32> {
            let document = editor.document();
            document.inlays_in(0..document.buffer().len()).map(|hint| hint.offset()).collect()
        }

        /// Drive the app's editor against the server until `done` holds, and say whether it did
        /// before `deadline`. Each round fires a scheduled hint fetch at once, as the widget
        /// would once its delay passed, then takes one event and lands its documents.
        fn settle_until(
            app: &mut App,
            events: &mpsc::Receiver<lsp::client::Event>,
            (started, deadline): (Instant, Instant),
            done: impl Fn(&CodeEditor) -> bool,
        ) -> bool {
            while !done(&app.editor) {
                if let Some(wake) = app.editor.pending_wake() {
                    let event = Event::Editor(Action::Wake(wake.generation));
                    let _ = app.editor.update(event, Instant::now());
                    sync(app);
                }
                let Some(updates) = pump(app, events, deadline) else {
                    return false;
                };
                for update in updates {
                    let lsp::Update::Document(document) = update else {
                        continue;
                    };
                    if let lsp::update::Change::Inlays(Some(hints)) = document.change() {
                        eprintln!("{:?}: {} hints at {:?}", started.elapsed(), hints.len(), document.stamp());
                    }
                    let Some(Lsp { client, .. }) = &mut app.lsp else { unreachable!("started checks the server") };
                    let _ = app.editor.apply_lsp(client, document);
                }
            }
            true
        }

        /// Against a real rust-analyzer: the scratch file's `let sum = add(1, 2);` gets its
        /// `: i32` hint after load; an edit above moves the hint before any refetch; a refetch
        /// at the new revision keeps it there; and a double-click inserts `: i32` once, with no
        /// hint left beside it, before or after the next refetch.
        #[test]
        #[ignore = "needs rust-analyzer on PATH and a Rust toolchain; run with --ignored"]
        fn rust_analyzer_hints_arrive_move_refresh_and_insert_once() {
            let (root, workspace) = workspace("hints-test");
            let (mut app, events) = started(&workspace);
            let started = Instant::now();
            let within = || (started, Instant::now() + Duration::from_secs(120));
            let current = |editor: &CodeEditor| {
                let document = editor.document();
                document.inlays_revision() == Some(document.revision())
            };

            let sum_end = SCRATCH_MAIN.find("let sum").expect("the scratch has `sum`");
            let sum_end = u32::try_from(sum_end + "let sum".len()).expect("small");
            let arrived = settle_until(&mut app, &events, within(), |e| hint_offsets(e).contains(&sum_end));
            assert!(arrived, "the `sum` hint arrived within the timeout");

            let moved = "// moved\n";
            let main_at = SCRATCH_MAIN.find("fn main").expect("the scratch has main");
            app.editor
                .try_edit(vec![EditOp::insert(u32::try_from(main_at).expect("small"), moved)])
                .expect("the edit applies");
            let typed = sum_end + 9;
            assert!(
                hint_offsets(&app.editor).contains(&typed),
                "the hint rides the edit before any refetch: {:?}",
                hint_offsets(&app.editor),
            );
            sync(&mut app);
            let refetched =
                settle_until(&mut app, &events, within(), |e| current(e) && hint_offsets(e).contains(&typed));
            assert!(refetched, "a refetch at the new revision keeps the hint after `sum`");

            let key = app
                .editor
                .document()
                .inlays_in(typed..typed)
                .find(|hint| hint.offset() == typed)
                .expect("the `sum` hint shows")
                .key();
            let insert = Event::Editor(Action::InlayInsert { key, offset: typed });
            let _ = app.editor.update(insert, Instant::now());
            sync(&mut app);
            let text = app.editor.document().text().into_owned();
            assert_eq!(text.matches("let sum: i32 = add(1, 2);").count(), 1, "the type is inserted once: {text}");
            let typed_end = typed + u32::try_from(": i32".len()).expect("small");
            let beside = |editor: &CodeEditor| {
                let offsets = hint_offsets(editor);
                offsets.contains(&typed) || offsets.contains(&typed_end)
            };
            assert!(!beside(&app.editor), "no hint is left beside the inserted type");

            let settled = settle_until(&mut app, &events, within(), current);
            assert!(settled, "the hints are refetched after the insert");
            assert!(
                !beside(&app.editor),
                "the refetch brings no hint beside the inserted type: {:?}",
                hint_offsets(&app.editor),
            );
            eprintln!("{:?}: done", started.elapsed());

            shut_down(&mut app, &events);
            let _ = std::fs::remove_dir_all(&root);
        }
    }
}
