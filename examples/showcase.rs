//! Edits a buffer that stresses wrapping, multibyte shaping, and scrolling.

use std::ops::Range;

use iced::widget::{column, pick_list, row, space, text, toggler};
use iced::{Center, Element, Fill, Font, Theme};

use matcha::decoration::{TextRange, diagnostic, inlay};
use matcha::{Action, Content, Position, code_editor, gutter};

pub fn main() -> iced::Result {
    iced::application(Showcase::new, Showcase::update, Showcase::view)
        .theme(Showcase::theme)
        .font(Font::MONOSPACE)
        .run()
}

/// Long lines, scripts with wide and combining glyphs, and blank lines — the
/// shapes the gutter and the decorations of later phases have to survive.
const SOURCE: &str = r#"//! A tour of the text this widget has to shape.

use std::collections::BTreeMap;

/// Greets in a handful of scripts: héllo, こんにちは, Здравствуйте, 👋🏽.
#[derive(Debug, Default)]
pub struct Guestbook {
    names: BTreeMap<usize, String>,
}

impl Guestbook {
    pub fn sign(&mut self, name: &str) {
        let next = self.names.len();

        self.names.insert(next, name.to_owned());
    }

    pub fn greet(&self) -> String {
        let mut greeting = String::new();

        for (line, name) in &self.names {
            // A deliberately long line, so that toggling word wrap reflows it and a diagnostic spanning it has to render as several fragments rather than one.
            greeting.push_str(&format!("{line}: hello, {name} — héllo 👋🏽, こんにちは, Здравствуйте\n"));
        }

        greeting
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

fn main() {
    let mut guestbook = Guestbook::default();

    guestbook.sign("world");
    guestbook.sign("世界");
    guestbook.sign("мир");

    print!("{}", guestbook.greet());
}
"#;

struct Showcase {
    content: Content,
    theme: Theme,
    word_wrap: bool,
    diagnostics: Vec<diagnostic::Diagnostic>,
    hints: Vec<inlay::Hint<'static>>,
}

#[derive(Debug, Clone)]
enum Message {
    Edit(Action),
    WordWrapToggled(bool),
    ThemeSelected(Theme),
}

impl Showcase {
    fn new() -> Self {
        let at = |line, bytes: Range<usize>, severity| diagnostic::Diagnostic {
            range: TextRange::new(
                Position {
                    line,
                    index: bytes.start,
                },
                Position {
                    line,
                    index: bytes.end,
                },
            ),
            severity,
        };

        let hint = |line, index, label: &'static str| inlay::Hint {
            position: Position { line, index },
            label: label.into(),
        };

        Self {
            content: Content::with_text(SOURCE),
            theme: Theme::SolarizedLight,
            word_wrap: true,
            // The shapes a squiggle has to take: a token mid-line, a range long enough to
            // cross a wrap, a blank line, an "insert here" of no width at all, and a
            // multibyte cluster. The positions are byte offsets into `SOURCE`, so editing
            // the text above moves them.
            diagnostics: vec![
                at(7, 4..9, diagnostic::Severity::Error),
                at(21, 12..159, diagnostic::Severity::Warning),
                at(13, 0..0, diagnostic::Severity::Information),
                at(34, 21..21, diagnostic::Severity::Hint),
                at(37, 20..26, diagnostic::Severity::Error),
            ],
            // What a language server sends back, and the shapes a hint has to survive:
            // one inside a line, where it paints over the code and is expected to; one
            // past the end of a short line, in the empty space; one deep inside the long
            // line, so it rides a continuation row once word wrap is on; and one in a
            // script the font has to fall back for.
            hints: vec![
                hint(12, 16, ": usize"),
                hint(29, 29, " → bool"),
                hint(22, 128, ": &str"),
                hint(37, 19, "なまえ: "),
            ],
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Edit(action) => {
                self.content.perform(action);
            }
            Message::WordWrapToggled(word_wrap) => {
                self.word_wrap = word_wrap;
            }
            Message::ThemeSelected(theme) => {
                self.theme = theme;
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let controls = row![
            toggler(self.word_wrap)
                .label("Word Wrap")
                .on_toggle(Message::WordWrapToggled),
            space::horizontal(),
            pick_list(Some(&self.theme), Theme::ALL, Theme::to_string)
                .on_select(Message::ThemeSelected),
        ]
        .spacing(10)
        .align_y(Center);

        let cursor = self.content.cursor();

        let status = text(format!(
            "{}:{}",
            cursor.position.line + 1,
            cursor.position.index + 1
        ));

        column![
            controls,
            code_editor(&self.content)
                .height(Fill)
                .placeholder("Type something here...")
                .on_action(Message::Edit)
                .diagnostics(&self.diagnostics)
                .inlay_hints(&self.hints)
                .gutter(gutter::Style {
                    // The editor's own text color, dimmed, so the eye reads the code first
                    // and the numbers still follow the theme.
                    color: self
                        .theme
                        .palette()
                        .background
                        .weakest
                        .text
                        .scale_alpha(0.5),
                    spacing: 12.0,
                })
                .wrapping(if self.word_wrap {
                    text::Wrapping::Word
                } else {
                    text::Wrapping::None
                })
                .highlight_with::<iced::highlighter::Parser>(
                    iced::highlighter::Settings {
                        token: "rs".to_owned(),
                    },
                    iced::Code::highlight,
                ),
            status,
        ]
        .spacing(10)
        .padding(10)
        .into()
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }
}
