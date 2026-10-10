//! `scratch` — the development window used to review the editor by eye.
//!
//! ```text
//! cargo run -p matcha-iced --features lsp,tree-sitter --example scratch                              # open the window
//! cargo run -p matcha-iced --features lsp,tree-sitter --example scratch -- --capture out.png         # headless PNG
//! cargo run -p matcha-iced --features lsp,tree-sitter --example scratch -- --capture-folds out.png   # …all folds collapsed
//! cargo run -p matcha-iced --features lsp,tree-sitter --example scratch -- --capture-find out.png    # …find+replace bar open
//! cargo run -p matcha-iced --features lsp,tree-sitter --example scratch -- --capture-whitespace out.png  # whitespace marks
//! cargo run -p matcha-iced --features lsp,tree-sitter --example scratch -- --large 100               # a 100 MB document
//! ```
//!
//! The editor highlights with tree-sitter and gets its language intelligence from the demo
//! language server (`shared/demo_server.rs`), which runs in-process on the client's memory
//! transport, so the window behaves the same natively and in a browser. It answers from the
//! document's text: an error on each `FIXME` and a warning on each `TODO`, completion, hover,
//! signature help, F12, F2 rename, Shift+Alt+F formatting, and inlay hints.
//!
//! In the window, Ctrl+Shift+W (Cmd+Shift+W on macOS) shows or hides the whitespace marks:
//! `·` for a space, `→` for a tab. Ctrl+I (Cmd+I) turns the inlay hints off and on.
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
use iced::widget::{column, container, text};
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
    /// Whether inlay hints show; Ctrl+I flips it.
    hints: bool,
    /// Whether the editor draws whitespace marks; Ctrl+Shift+W flips it.
    show_whitespace: bool,
}

/// The client and the server it talks to.
struct Lsp {
    client: lsp::Client,
    server: Server,
}

enum Server {
    /// The in-process demo server, which answers only when stepped.
    Demo { scripted: demo_server::Scripted },
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
}

impl Lsp {
    /// Lets the server answer what the client sent; returns whether it had anything to answer.
    fn step(&mut self) -> bool {
        match &mut self.server {
            Server::Demo { scripted } => scripted.step(),
        }
    }
}

impl App {
    /// The demo editor over `text`, talking to the demo server, settled at `now`: the
    /// handshake is done, and the first diagnostics and hints have landed. Run the returned
    /// stream (or [`settle`](Self::settle) on it) to keep the conversation going.
    pub fn demo(text: &str, now: Instant) -> (Self, lsp::client::Events) {
        let (near, far) = lsp::lsp_server::Connection::memory();
        let (mut client, mut events) = lsp::Client::builder().root(uri("file:///demo/")).memory(near);
        let mut editor = Self::editor(text, rust());
        editor.open_lsp(&mut client, &uri(DEMO_URI), "rust").expect("the client has no other document");
        let mut app = Self::with(editor, Some(Lsp { client, server: Server::Demo { scripted: demo_server::Scripted::new(far) } }));
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
        Self { editor, lsp, status: String::new(), hints: true, show_whitespace: false }
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
                if let Some(Lsp { client, .. }) = &mut self.lsp {
                    let mut jumps = Vec::new();
                    for update in client.receive(event) {
                        match update {
                            lsp::Update::Document(document) => {
                                let applied = self.editor.apply_lsp(client, document);
                                if let Some(refusal) = applied.refused {
                                    self.status = format!("refused: {refusal}");
                                }
                                jumps.push(applied.jump);
                            }
                            lsp::Update::Status(status) => self.status = format!("demo server: {status:?}"),
                            lsp::Update::Error(error) => self.status = format!("error: {error}"),
                            lsp::Update::FileEdits(_)
                            | lsp::Update::Log(_)
                            | lsp::Update::Notification(_)
                            | lsp::Update::Trace(_) => {}
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

    /// The window: the editor over the status line.
    pub fn view(&self) -> Element<'_, Message> {
        let status = text(self.status.as_str()).size(12).font(matcha_iced::DEFAULT_FONT);
        column![container(self.editor_view()).width(Fill).height(Fill), container(status).padding([2, 8])].into()
    }

    /// The editor alone, as the captures render it.
    pub fn editor_view(&self) -> Element<'_, Message> {
        self.editor.view().map(Message::Editor)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([self.editor.subscription().map(Message::Editor), keyboard::listen().filter_map(chord)])
    }
}

/// The app's chords, each with Ctrl (Cmd on macOS), without Alt and not repeated: Shift+W
/// toggles the whitespace marks and I the inlay hints. The editor binds neither, so they reach
/// `keyboard::listen`.
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
        _ => None,
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

    let app = iced::application::timed(boot, App::update, App::subscription, App::view)
        .title("matcha — scratch")
        .theme(theme);
    // Register every font the widget requires (fold chevrons + find-bar icons)
    // through the one owner, so the set can't be loaded piecemeal and leave tofu.
    app.fonts(matcha_iced::required_fonts().iter().copied()).run()
}

/// The window's app: `--large`'s corpus, or the sample on the demo server, whose events the
/// runtime delivers from then on.
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
            Some(Lsp { server: Server::Demo { scripted }, .. }) => scripted,
            None => panic!("the demo app has a server"),
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

    /// Ctrl+Shift+W toggles the whitespace marks and Ctrl+I the hints (Cmd on macOS); nothing
    /// else does.
    #[test]
    fn chord_matches_ctrl_shift_w_and_ctrl_i_only() {
        let press = |c: &str, modifiers: Modifiers, repeat: bool| keyboard::Event::KeyPressed {
            key: Key::Character(c.into()),
            modified_key: Key::Character(c.into()),
            physical_key: Physical::Code(if c.eq_ignore_ascii_case("w") { Code::KeyW } else { Code::KeyI }),
            location: Location::Standard,
            modifiers,
            text: None,
            repeat,
        };
        assert!(matches!(chord(press("W", Modifiers::COMMAND | Modifiers::SHIFT, false)), Some(Message::ToggleWhitespace)));
        assert!(matches!(chord(press("i", Modifiers::COMMAND, false)), Some(Message::ToggleHints)));
        for (c, modifiers, repeat, what) in [
            ("W", Modifiers::COMMAND, false, "Ctrl+W"),
            ("W", Modifiers::COMMAND | Modifiers::SHIFT | Modifiers::ALT, false, "Ctrl+Shift+Alt+W"),
            ("W", Modifiers::COMMAND | Modifiers::SHIFT, true, "a repeated Ctrl+Shift+W"),
            ("W", Modifiers::SHIFT, false, "a plain W"),
            ("I", Modifiers::COMMAND | Modifiers::SHIFT, false, "Ctrl+Shift+I"),
            ("i", Modifiers::COMMAND | Modifiers::ALT, false, "Ctrl+Alt+I"),
            ("i", Modifiers::COMMAND, true, "a repeated Ctrl+I"),
            ("i", Modifiers::empty(), false, "a plain i"),
        ] {
            assert!(chord(press(c, modifiers, repeat)).is_none(), "{what} is no chord");
        }
    }
}
