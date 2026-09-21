//! A code editor widget for [iced], with diagnostic underlines, inlay-hint
//! chips, and a line-number gutter.
//!
//! [`CodeEditor`] edits, selects, wraps, scrolls, and highlights exactly as
//! `iced::widget::TextEditor` does. What it adds is that it owns its shaped
//! text, so the three decorations can be placed against the glyph geometry the
//! editor actually laid out rather than against a guess at it.
//!
//! # Example
//!
//! ```no_run
//! use iced::{Color, Element, Fill};
//!
//! use matcha::decoration::{TextRange, diagnostic, inlay};
//! use matcha::{Action, Content, Position, code_editor, gutter};
//!
//! struct Editor {
//!     content: Content,
//!     diagnostics: Vec<diagnostic::Diagnostic>,
//!     hints: Vec<inlay::Hint<'static>>,
//! }
//!
//! #[derive(Debug, Clone)]
//! enum Message {
//!     Edit(Action),
//! }
//!
//! impl Editor {
//!     fn new() -> Self {
//!         Self {
//!             content: Content::with_text("fn main() {\n    let answer = 42;\n}\n"),
//!             diagnostics: vec![diagnostic::Diagnostic {
//!                 range: TextRange::new(
//!                     Position { line: 1, index: 8 },
//!                     Position { line: 1, index: 14 },
//!                 ),
//!                 severity: diagnostic::Severity::Warning,
//!             }],
//!             hints: vec![inlay::Hint {
//!                 position: Position { line: 1, index: 14 },
//!                 label: ": i32".into(),
//!             }],
//!         }
//!     }
//!
//!     fn update(&mut self, message: Message) {
//!         match message {
//!             Message::Edit(action) => self.content.perform(action),
//!         }
//!     }
//!
//!     fn view(&self) -> Element<'_, Message> {
//!         code_editor(&self.content)
//!             .height(Fill)
//!             .on_action(Message::Edit)
//!             .diagnostics(&self.diagnostics)
//!             .inlay_hints(&self.hints)
//!             .gutter(gutter::Style {
//!                 color: Color::from_rgb8(0x92, 0x83, 0x74),
//!                 spacing: 12.0,
//!             })
//!             .into()
//!     }
//! }
//!
//! fn main() -> iced::Result {
//!     iced::application(Editor::new, Editor::update, Editor::view).run()
//! }
//! ```
//!
//! # Positions are UTF-8 byte indices
//!
//! A [`Position`] is a line number and a **byte** offset into that line. Not a
//! UTF-16 code unit, not a grapheme, and not a column: `Position { line: 0,
//! index: 3 }` is three bytes into line 0, which is one `é` and one `l`.
//!
//! The Language Server Protocol counts in UTF-16 by default. The widget still
//! takes positions in the units the text is stored in and knows nothing about
//! the protocol; converting happens beside it, in `matcha::lsp`, behind a feature
//! that is off unless asked for.
//!
//! # Inlay hints are opaque overlays
//!
//! A [`Hint`](decoration::inlay::Hint) is a label on a filled, outlined chip,
//! drawn in a layer above the text — so a hint anchored inside a line hides the
//! code there rather than tangling with it, which is what keeps both legible.
//! Hints read best shown momentarily for that reason.
//!
//! Showing them is the application's call, and what it hands
//! [`inlay_hints`](CodeEditor::inlay_hints) is the whole of the control:
//! `&hints` to show them, `&[]` to show none. Binding that choice to a held
//! modifier is how the momentary peek is built. The widget offers no reveal of
//! its own, because it would duplicate state the application already has and
//! pick a key on its behalf.
//!
//! Hiding code is the only thing a chip does to it. Nothing reserves room,
//! reflows a line, moves the caret, or takes part in hit-testing, so a click
//! through a chip lands on the character beneath it exactly as if the chip were
//! not there. Text that pushes code aside instead is *virtual text*, which is a
//! different feature and not this one.
//!
//! # Stale positions are ignored, never fatal
//!
//! A decoration naming a line the buffer does not have, or a byte past the end
//! of the line it does name, is silently not drawn. This is deliberate:
//! decorations come from something that read an older version of the text — a
//! language server answering a request the next keystroke already invalidated —
//! so a position that no longer exists is the ordinary case rather than a bug,
//! and one that panicked would make every round-trip a race to lose. Nothing
//! has to be clamped, checked, or shifted before being handed over.
//!
//! Two things get called stale and only one of them is this. A position that
//! **no longer resolves** is the case above: harmless, and gone by the next
//! publish. A position that **still resolves and now means something else** is
//! not, because nothing about the result looks wrong afterwards. That is
//! survivable for a decoration, which is replaced whole on the next round trip,
//! and is not survivable for an edit, which changes the wrong bytes. So
//! `Content::apply` takes the `Content::revision` the request was sent at and
//! refuses a buffer that has moved since, while decorations go on being
//! ignored.
//!
//! # Speaking to a language server
//!
//! Off by default. The `lsp` feature adds `matcha::lsp`, which converts a
//! server's positions into the editor's and applies its edits; `lsp-types` and
//! `gen-lsp-types` add conversions to and from those crates, so an application
//! does not write the mapping itself.
//!
//! ```toml
//! matcha = { git = "...", features = ["lsp-types"] }
//! ```
//!
//! The two crates are asked for by range rather than by version, so Cargo
//! settles on whichever copy an application already has. `cargo run --example
//! lsp --features lsp-types` shows real notification payloads going through it.
//!
//! # Requirements
//!
//! **iced has to be the pinned git revision.** matcha is built against
//! unreleased iced, and an application that resolves a different revision gets
//! a second copy of `iced_core` whose `Element` and `Renderer` will not unify
//! with this crate's:
//!
//! ```toml
//! [dependencies]
//! iced = { git = "https://github.com/iced-rs/iced", rev = "fa3bae52874274c012a27d4bf11a83c49e1709ae" }
//! matcha = { git = "https://github.com/NickelWenzel/matcha" }
//! ```
//!
//! **A renderer backend is mandatory.** `wgpu` and `tiny-skia` are both default
//! features of iced; with both turned off, `iced::Renderer` resolves to `()`,
//! whose `text::Renderer::Editor` is `()` as well, and [`CodeEditor`]'s
//! `Renderer::Editor = graphics::text::Editor` bound stops holding. What that
//! looks like is an unsatisfied trait bound at the call site rather than a
//! missing item, so it is worth knowing where to look.
//!
//! [iced]: https://github.com/iced-rs/iced

#![warn(missing_docs)]

pub mod code_editor;

pub use code_editor::{
    Action, Binding, CodeEditor, Content, Cursor, Edit, KeyPress, Line, LineEnding, Motion,
    Position, Selection, code_editor, decoration, gutter,
};

#[cfg(feature = "lsp")]
pub use code_editor::lsp;
