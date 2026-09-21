//! Wire payloads from a language server, drawn by the widget.
//!
//! The messages below are the JSON a server actually sends, kept as text and
//! read the way an application would read them. Nothing here is a transport:
//! each answer arrives when a key is pressed rather than from a process, which
//! puts the timing in the reader's hands. That is the point. An answer
//! describes the text as it was when the question was asked, so being able to
//! edit between the two is what there is to see.
//!
//! Two paths are shown, because both are supported. Diagnostics go through
//! [`lsp::Message`], which is what an application with one place everything
//! arrives would use. Inlay hints go straight to the bridge, which is what an
//! application that already knows what it asked for would do.
//!
//! Press **1** for diagnostics, **2** for inlay hints, **3** for a code action,
//! and **a** to apply it. Edit the buffer between **3** and **a** and the
//! action is refused: its edits describe text that has moved.

use iced::widget::{column, row, space, text};
use iced::{Center, Element, Fill, Font, Subscription, Theme, keyboard};

use matcha::decoration::{diagnostic, inlay};
use matcha::lsp::{self, Encoding};
use matcha::{Action, Content, code_editor, gutter};

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view)
        .subscription(State::subscription)
        .theme(|_state: &State| Theme::GruvboxDark)
        .font(Font::MONOSPACE)
        .run()
}

const SOURCE: &str = r#"fn main() {
    let path = "input.txt";
    let contents = std::fs::read_to_string(path).unwrap();

    println!("{} bytes", contents.len());
}
"#;

/// A `textDocument/publishDiagnostics` notification, as it comes off the wire.
///
/// Its columns are UTF-16 code units, which is what the editor's byte offsets
/// have to be worked out from.
const DIAGNOSTICS: &str = r#"{
  "uri": "file:///input.rs",
  "version": 1,
  "diagnostics": [
    {
      "range": {"start": {"line": 2, "character": 49}, "end": {"line": 2, "character": 57}},
      "severity": 2,
      "source": "clippy",
      "code": "unwrap_used",
      "message": "used `unwrap()` on a `Result` value"
    }
  ]
}"#;

/// A `textDocument/inlayHint` response, with one label sent in parts.
const HINTS: &str = r#"[
  {
    "position": {"line": 1, "character": 12},
    "label": [{"value": ": "}, {"value": "&str"}],
    "kind": 1,
    "paddingLeft": false
  },
  {
    "position": {"line": 2, "character": 16},
    "label": ": String",
    "kind": 1
  }
]"#;

/// A `textDocument/codeAction` response: replace the `unwrap` with a `?`.
const ACTION: &str = r#"[
  {
    "title": "Replace unwrap() with ?",
    "kind": "quickfix",
    "isPreferred": true,
    "edit": {
      "documentChanges": [
        {
          "textDocument": {"uri": "file:///input.rs", "version": 1},
          "edits": [
            {
              "range": {"start": {"line": 2, "character": 49}, "end": {"line": 2, "character": 57}},
              "newText": "?"
            }
          ]
        }
      ]
    }
  }
]"#;

struct State {
    content: Content,
    /// What the server counts columns in. An application reads this from the
    /// server's answer to `initialize`; there is no server here, so it is the
    /// protocol's default.
    encoding: Encoding,
    diagnostics: Vec<diagnostic::Diagnostic>,
    hints: Vec<inlay::Hint<'static>>,
    /// The action, and the revision the server described.
    ///
    /// The pair is the only thing matcha cannot work out for itself: it does
    /// not send `didChange`, so only an application knows which of its versions
    /// a buffer is at.
    action: Option<(lsp::CodeAction, u64)>,
    status: String,
}

#[derive(Debug, Clone)]
enum Message {
    Edit(Action),
    DiagnosticsPublished,
    HintsAnswered,
    ActionOffered,
    ActionApplied,
}

impl State {
    fn new() -> Self {
        Self {
            content: Content::with_text(SOURCE),
            encoding: Encoding::default(),
            diagnostics: Vec::new(),
            hints: Vec::new(),
            action: None,
            status: "waiting for the server".to_owned(),
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Edit(action) => self.content.perform(action),

            // Through the envelope: one path for anything that arrives.
            Message::DiagnosticsPublished => {
                let params: lsp_types::PublishDiagnosticsParams =
                    serde_json::from_str(DIAGNOSTICS).expect("the payload above is valid JSON");

                if let lsp::Message::Diagnostics {
                    uri, diagnostics, ..
                } = lsp::Message::from(params)
                {
                    self.diagnostics = self.content.lsp(self.encoding).diagnostics(&diagnostics);
                    self.status = format!("{} diagnostics for {uri}", diagnostics.len());
                }
            }

            // Straight to the bridge: an application that knows what it asked
            // for has no envelope to unwrap.
            Message::HintsAnswered => {
                let hints: Vec<lsp_types::InlayHint> =
                    serde_json::from_str(HINTS).expect("the payload above is valid JSON");
                let hints: Vec<lsp::Hint> = hints.into_iter().map(lsp::Hint::from).collect();

                self.hints = self.content.lsp(self.encoding).hints(&hints);
                self.status = format!("{} hints", self.hints.len());
            }

            Message::ActionOffered => {
                let offered: Vec<lsp_types::CodeActionOrCommand> =
                    serde_json::from_str(ACTION).expect("the payload above is valid JSON");

                if let Some(lsp::Offer::Action(action)) =
                    offered.into_iter().map(lsp::Offer::from).next()
                {
                    // Recorded now, checked when the action is applied.
                    self.action = Some((action, self.content.revision()));
                    self.status = "press a to apply the quickfix".to_owned();
                }
            }

            Message::ActionApplied => {
                let Some((action, revision)) = self.action.clone() else {
                    return;
                };
                let Some(edit) = action.edit.as_ref() else {
                    return;
                };

                // matcha applies one document. Which buffer each edit belongs
                // to, and everything that is not an edit, is the application's.
                for document in edit.document_edits() {
                    self.status = match self.content.apply(document, self.encoding, revision) {
                        Ok(_) => {
                            self.action = None;
                            self.diagnostics.clear();
                            "applied".to_owned()
                        }
                        Err(lsp::Error::Stale { .. }) => {
                            "the buffer moved; ask the server again".to_owned()
                        }
                        Err(refused) => refused.to_string(),
                    };
                }
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let status = row![
            text("1 diagnostics   2 hints   3 action   a apply").size(13),
            space::horizontal(),
            text(&self.status).size(13),
            space::horizontal(),
            text(format!("revision {}", self.content.revision())).size(13),
        ]
        .align_y(Center);

        column![
            code_editor(&self.content)
                .height(Fill)
                .on_action(Message::Edit)
                .diagnostics(&self.diagnostics)
                .inlay_hints(&self.hints)
                .gutter(gutter::Style {
                    color: iced::color!(0x92_83_74),
                    spacing: 12.0,
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

    fn subscription(&self) -> Subscription<Message> {
        // A key rather than a timer, so that the reader decides when each
        // answer lands and can edit the buffer between two of them.
        keyboard::listen().filter_map(|event| {
            let keyboard::Event::KeyPressed { key, .. } = event else {
                return None;
            };
            let keyboard::Key::Character(pressed) = key.as_ref() else {
                return None;
            };

            match pressed {
                "1" => Some(Message::DiagnosticsPublished),
                "2" => Some(Message::HintsAnswered),
                "3" => Some(Message::ActionOffered),
                "a" => Some(Message::ActionApplied),
                _ => None,
            }
        })
    }
}
