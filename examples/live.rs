//! Rebuilds every decoration from the text on each edit, the way a language
//! server delivers them.
//!
//! Nothing here is patched in place or shifted to follow an edit: [`analyze`]
//! and [`infer_hints`] run over the whole buffer and return fresh vectors, and
//! the widget is handed those. The one decoration that is *not* re-derived is
//! pinned to the end of the buffer as it was loaded, so that deleting the last
//! line strands it — which is what a reply to a request that lost a race looks
//! like, and which has to come out as nothing drawn rather than as a panic.

use iced::keyboard;
use iced::widget::{button, column, row, space, text};
use iced::{Center, Element, Fill, Font, Subscription, Theme, color};

use matcha::decoration::{TextRange, diagnostic, inlay};
use matcha::{Action, Content, Edit, Motion, Position, code_editor, gutter};

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view)
        .subscription(State::subscription)
        .theme(|_state: &State| Theme::GruvboxDark)
        .font(Font::MONOSPACE)
        .run()
}

const SOURCE: &str = r#"fn main() {
    // TODO: take the path from the command line
    let path = "input.txt";
    let contents = std::fs::read_to_string(path).unwrap();
    let limit = 80;

    for line in contents.lines() {
        // TODO: report the offenders rather than printing them
        if line.len() > limit {
            println!("{line}");
        }
    }
}
"#;

struct State {
    content: Content,
    diagnostics: Vec<diagnostic::Diagnostic>,
    hints: Vec<inlay::Hint<'static>>,
    /// The end of the buffer as it was loaded, decorated once and never
    /// re-derived.
    ///
    /// Deleting the last line leaves it naming text that is gone. Both
    /// decorations anchored here then stop drawing — silently, and without
    /// anything in this example or in the widget checking for it.
    pinned: Position,
}

#[derive(Debug, Clone)]
enum Message {
    Edit(Action),
    LastLineDeleted,
    Restored,
}

impl State {
    fn new() -> Self {
        let content = Content::with_text(SOURCE);

        let mut state = Self {
            pinned: Position {
                line: content.line_count() - 1,
                index: 0,
            },
            content,
            diagnostics: Vec::new(),
            hints: Vec::new(),
        };

        state.reanalyze();
        state
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Edit(action) => {
                let is_edit = action.is_edit();

                self.content.perform(action);

                if is_edit {
                    self.reanalyze();
                }
            }
            Message::LastLineDeleted => {
                // Emptying the last line and then joining it to the one above is what
                // strands the pinned position: the buffer loses a line and the position
                // keeps naming the one it had.
                for action in [
                    Action::Move(Motion::DocumentEnd),
                    Action::Edit(Edit::BackspaceLine),
                    Action::Edit(Edit::Backspace),
                ] {
                    self.content.perform(action);
                }

                self.reanalyze();
            }
            Message::Restored => {
                self.content = Content::with_text(SOURCE);
                self.reanalyze();
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let controls = row![
            button(text("Delete the last line")).on_press(Message::LastLineDeleted),
            button(text("Restore")).on_press(Message::Restored),
            space::horizontal(),
            text(format!(
                "{} diagnostics, {} hints",
                self.diagnostics.len(),
                self.hints.len()
            )),
        ]
        .spacing(10)
        .align_y(Center);

        let pinned_is_live = self
            .content
            .line(self.pinned.line)
            .is_some_and(|line| self.pinned.index <= line.len());

        let pinned = text(format!(
            "pinned at line {}, byte {} — {}",
            self.pinned.line + 1,
            self.pinned.index + 1,
            if pinned_is_live {
                "drawn"
            } else {
                "stale, and silently not drawn"
            }
        ));

        column![
            controls,
            code_editor(&self.content)
                .height(Fill)
                .on_action(Message::Edit)
                .diagnostics(&self.diagnostics)
                .inlay_hints(&self.hints)
                .gutter(gutter::Style {
                    color: color!(0x928374),
                    spacing: 12.0,
                })
                .highlight_with::<iced::highlighter::Parser>(
                    iced::highlighter::Settings {
                        token: "rs".to_owned(),
                    },
                    iced::Code::highlight,
                ),
            pinned,
        ]
        .spacing(10)
        .padding(10)
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        keyboard::listen().filter_map(|event| match event {
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            } => Some(Message::Restored),
            _ => None,
        })
    }

    /// Throws the decorations away and derives them again from the text.
    fn reanalyze(&mut self) {
        let text = self.content.text();

        self.diagnostics = analyze(&text);
        self.hints = infer_hints(&text);

        self.diagnostics.push(diagnostic::Diagnostic {
            range: TextRange::new(self.pinned, self.pinned),
            severity: diagnostic::Severity::Information,
        });

        self.hints.push(inlay::Hint {
            position: self.pinned,
            label: "  ← pinned to the end of the original buffer".into(),
        });
    }
}

/// Stands in for a language server: every `TODO` is a warning and every
/// `unwrap()` an error.
///
/// Trivial on purpose. What matters is that it reads the whole text and returns
/// a fresh vector, so that nothing drawn outlives the edit that invalidated it.
fn analyze(text: &str) -> Vec<diagnostic::Diagnostic> {
    let flag = |line: usize, at: usize, found: &str, severity| diagnostic::Diagnostic {
        range: TextRange::new(
            Position { line, index: at },
            Position {
                line,
                index: at + found.len(),
            },
        ),
        severity,
    };

    text.lines()
        .enumerate()
        .flat_map(|(line, source)| {
            let todos = source
                .match_indices("TODO")
                .map(move |(at, found)| flag(line, at, found, diagnostic::Severity::Warning));

            let unwraps = source
                .match_indices("unwrap()")
                .map(move |(at, found)| flag(line, at, found, diagnostic::Severity::Error));

            todos.chain(unwraps)
        })
        .collect()
}

/// Stands in for the same server's inlay hints: a type after every `let`
/// binding, guessed from the initializer.
fn infer_hints(text: &str) -> Vec<inlay::Hint<'static>> {
    text.lines()
        .enumerate()
        .filter_map(|(line, source)| {
            let binding = source.find("let ")? + "let ".len();
            let index = binding + source[binding..].find(" =")?;
            let initializer = source[index..].trim_start_matches([' ', '=']);

            Some(inlay::Hint {
                position: Position { line, index },
                label: if initializer.starts_with('"') {
                    ": &str"
                } else if initializer.starts_with(|first: char| first.is_ascii_digit()) {
                    ": i32"
                } else {
                    ": _"
                }
                .into(),
            })
        })
        .collect()
}
