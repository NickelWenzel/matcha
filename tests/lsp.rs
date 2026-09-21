//! What a language server sends, drawn by the widget.
//!
//! The unit tests in `src/code_editor/lsp/bridge.rs` check the conversion
//! against positions it computes itself. What is left for here is that the
//! result is something the widget will take: [`Bridge::hints`] returns owned
//! labels, and the widget borrows the hints it draws, so the two only meet at
//! a call site.

#![cfg(feature = "lsp")]

use iced::widget::text;
use iced::{Element, Pixels, Point};
use iced_test::{simulator, simulator::click};

use matcha::lsp::{self, Bridge, Encoding};
use matcha::{Action, Content, code_editor};

/// Two bytes per character on the first line, three on the second.
const SOURCE: &str = "héllo wörld\n日本語のテキスト";

const TEXT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 20.0;

#[derive(Debug, Clone)]
enum Message {
    Edit(Action),
}

fn editor<'a>(
    content: &'a Content,
    diagnostics: &'a [matcha::decoration::diagnostic::Diagnostic],
    hints: &'a [matcha::decoration::inlay::Hint<'a>],
) -> Element<'a, Message> {
    code_editor(content)
        .padding(0.0)
        .size(TEXT_SIZE)
        .line_height(Pixels(LINE_HEIGHT))
        .wrapping(text::Wrapping::None)
        .on_action(Message::Edit)
        .diagnostics(diagnostics)
        .inlay_hints(hints)
        .into()
}

fn diagnostic(
    bridge: &Bridge<'_>,
    range: lsp::Range,
) -> Vec<matcha::decoration::diagnostic::Diagnostic> {
    bridge.diagnostics(&[lsp::Diagnostic {
        range,
        severity: matcha::decoration::diagnostic::Severity::Warning,
        message: "unused".to_owned(),
        source: Some("rustc".to_owned()),
        code: None,
        code_description: None,
        tags: vec![lsp::diagnostic::Tag::Unnecessary],
        related: Vec::new(),
        data: None,
    }])
}

fn at(line: u32, character: u32) -> lsp::Position {
    lsp::Position { line, character }
}

#[test]
fn utf16_columns_become_the_bytes_the_widget_draws_against() {
    let content = Content::with_text(SOURCE);
    let bridge = content.lsp(Encoding::Utf16);

    // "héllo": the ö of "wörld" is the seventh character and the eighth byte.
    let converted = diagnostic(
        &bridge,
        lsp::Range {
            start: at(0, 6),
            end: at(0, 11),
        },
    );

    assert_eq!(
        converted[0].range.start(),
        matcha::Position { line: 0, index: 7 },
        "one character before the range start is two bytes wide"
    );
    assert_eq!(
        converted[0].range.end(),
        matcha::Position { line: 0, index: 13 },
        "column 11 is the eleventh and last unit, so the range ends at the \
         thirteenth byte, which is the end of the line"
    );
}

#[test]
fn converted_decorations_are_something_the_widget_will_take() {
    let content = Content::with_text(SOURCE);
    let bridge = content.lsp(Encoding::Utf16);

    let diagnostics = diagnostic(
        &bridge,
        lsp::Range {
            start: at(1, 0),
            end: at(1, 3),
        },
    );

    let hints = bridge.hints(&[lsp::Hint {
        position: at(0, 5),
        label: ": String".to_owned(),
        kind: Some(lsp::hint::Kind::Type),
        padding_left: true,
        padding_right: false,
        tooltip: None,
        text_edits: Vec::new(),
        data: None,
    }]);

    // The labels are owned, so they outlive the call that made them and the
    // widget can borrow them from here. A conversion that borrowed its input
    // could not be held beside the content like this.
    let mut ui = simulator(editor(&content, &diagnostics, &hints));

    ui.point_at(Point::new(4.0, LINE_HEIGHT / 2.0));
    let _ = ui.simulate(click());
    let _ = ui.typewrite("x");

    let actions: Vec<Action> = ui
        .into_messages()
        .map(|Message::Edit(action)| action)
        .collect();

    assert!(
        actions.iter().any(Action::is_edit),
        "decorations must not stop the editor editing"
    );
}

#[test]
fn a_diagnostic_from_an_older_version_of_the_text_is_harmless() {
    let content = Content::with_text(SOURCE);
    let bridge = content.lsp(Encoding::Utf16);

    // A server that read a longer buffer than this one.
    let diagnostics = diagnostic(
        &bridge,
        lsp::Range {
            start: at(400, 0),
            end: at(400, 9),
        },
    );

    assert_eq!(diagnostics.len(), 1, "nothing is dropped on the way in");

    let mut ui = simulator(editor(&content, &diagnostics, &[]));

    ui.point_at(Point::new(4.0, LINE_HEIGHT / 2.0));
    let _ = ui.simulate(click());
    let _ = ui.typewrite("x");

    assert!(
        ui.into_messages()
            .any(|Message::Edit(action)| action.is_edit()),
        "a stale diagnostic must not stop the editor working"
    );
}
