//! What the widget does for someone outside the crate, with nothing but the
//! public API and `iced_test` to do it with.
//!
//! The unit tests in `src/code_editor/widget.rs` pin each decoration on its own
//! against a recording renderer they can only reach from inside. What is left
//! for here is the configuration a user actually builds — a gutter, diagnostics
//! and inlay hints all at once — and the behaviour that survives it.

use iced::keyboard::key;
use iced::widget::text;
use iced::{Element, Pixels, Point, Rectangle};
use iced_test::{selector, simulator};

use matcha::decoration::{TextRange, diagnostic, inlay};
use matcha::{Action, Content, Cursor, Position, code_editor, gutter};

/// The lines the editors below hold. Their byte lengths are what the caret
/// assertions are written against.
const LINES: [&str; 4] = ["alpha bravo charlie", "delta echo", "", "foxtrot golf"];

/// The line height the editors below are laid out at.
///
/// Absolute, and written down here rather than left to the renderer's default,
/// so that the row a click at a given `y` lands in is a number this file knows.
const LINE_HEIGHT: f32 = 20.0;

const TEXT_SIZE: f32 = 14.0;

const GUTTER: gutter::Style = gutter::Style {
    color: iced::Color::BLACK,
    spacing: 8.0,
};

const EDITOR: &str = "code-editor";

/// An `x` to the right of every line in [`LINES`], where a click can only
/// resolve to the end of the row it lands in — whatever the gutter has done to
/// the text origin.
const PAST_THE_END: f32 = 900.0;

#[derive(Debug, Clone)]
enum Message {
    Edit(Action),
}

/// Builds the editor every test below drives.
///
/// No padding of its own, so a row's `y` is `LINE_HEIGHT` times its index; no
/// wrapping, so a line stays on one row and a click's column is a function of
/// its `x` alone. The default font rather than a monospace one, because the
/// bundled Fira Sans is what keeps shaping off whatever the host machine
/// happens to have installed.
fn editor<'a>(
    content: &'a Content,
    gutter: Option<gutter::Style>,
    diagnostics: &'a [diagnostic::Diagnostic],
    hints: &'a [inlay::Hint<'a>],
) -> Element<'a, Message> {
    let editor = code_editor(content)
        .id(EDITOR)
        .padding(0.0)
        .size(TEXT_SIZE)
        .line_height(Pixels(LINE_HEIGHT))
        .wrapping(text::Wrapping::None)
        .on_action(Message::Edit)
        .diagnostics(diagnostics)
        .inlay_hints(hints);

    match gutter {
        Some(style) => editor.gutter(style),
        None => editor,
    }
    .into()
}

/// What a language server would send back for [`LINES`]: a token mid-line, an
/// insert-here of no width, and a mark on the blank line.
fn diagnostics() -> Vec<diagnostic::Diagnostic> {
    let mark = |line, start, end, severity| diagnostic::Diagnostic {
        range: TextRange::new(
            Position { line, index: start },
            Position { line, index: end },
        ),
        severity,
    };

    vec![
        mark(0, 6, 11, diagnostic::Severity::Error),
        mark(1, 5, 5, diagnostic::Severity::Hint),
        mark(2, 0, 0, diagnostic::Severity::Warning),
    ]
}

/// Hints in the shapes that would move a glyph if they were text rather than
/// overlays: inside a line, on a blank one, and at the end of the last.
fn hints() -> Vec<inlay::Hint<'static>> {
    let hint = |line, index, label: &'static str| inlay::Hint {
        position: Position { line, index },
        label: label.into(),
    };

    vec![
        hint(0, 5, ": usize"),
        hint(2, 0, "→ ()"),
        hint(3, 12, "  // and a label wider than the line it hangs off"),
    ]
}

/// The editor-relative point the widget resolved each of `probes` to.
///
/// `Action::Click` carries that point, which is the click-to-caret mapping
/// itself rather than the caret it happens to leave behind. Nothing is
/// performed on `content`, so every probe starts from the same buffer.
fn click_points(
    content: &Content,
    gutter: Option<gutter::Style>,
    diagnostics: &[diagnostic::Diagnostic],
    hints: &[inlay::Hint<'_>],
    probes: &[Point],
) -> Vec<Point> {
    probes
        .iter()
        .map(|at| {
            let mut ui = simulator(editor(content, gutter, diagnostics, hints));

            ui.point_at(*at);
            let _ = ui.simulate(simulator::click());

            let actions: Vec<Action> = ui
                .into_messages()
                .map(|Message::Edit(action)| action)
                .collect();

            match actions.as_slice() {
                [Action::Click(at, _)] => *at,
                other => panic!("a click should publish exactly one action, not {other:?}"),
            }
        })
        .collect()
}

/// Clicks at `at`, works the keyboard, and returns the widget's bounds along
/// with every action it published.
fn interact(
    content: &Content,
    gutter: Option<gutter::Style>,
    diagnostics: &[diagnostic::Diagnostic],
    hints: &[inlay::Hint<'_>],
    at: Point,
) -> (Rectangle, Vec<Action>) {
    let mut ui = simulator(editor(content, gutter, diagnostics, hints));

    ui.point_at(at);
    let _ = ui.simulate(simulator::click());
    let _ = ui.typewrite(" héllo");
    let _ = ui.tap_key(key::Named::Enter);
    let _ = ui.typewrite("世界");
    let _ = ui.tap_key(key::Named::ArrowUp);
    let _ = ui.tap_key(key::Named::Home);

    let bounds = ui
        .find(selector::id(EDITOR))
        .expect("the editor carries that id")
        .bounds();

    let actions = ui
        .into_messages()
        .map(|Message::Edit(action)| action)
        .collect();

    (bounds, actions)
}

#[test]
fn a_gutter_shifts_every_click_by_one_amount_and_decorations_shift_none() {
    let content = Content::with_text(&LINES.join("\n"));

    // Inside the first line and well right of its origin, so each probe has a
    // character of its own to resolve to rather than a clamp at either end.
    let probes = [40.0, 80.0, 120.0].map(|x| Point::new(x, LINE_HEIGHT / 2.0));

    // Every decoration has to name text the buffer actually has: one that does not is
    // dropped before it could affect anything, which would leave this vacuous.
    for hint in hints() {
        assert!(
            content
                .line(hint.position.line)
                .is_some_and(|line| hint.position.index <= line.len()),
            "{hint:?} is anchored outside the buffer"
        );
    }

    for diagnostic in diagnostics() {
        assert!(
            content
                .line(diagnostic.range.end().line)
                .is_some_and(|line| diagnostic.range.end().index <= line.len()),
            "{diagnostic:?} covers text the buffer does not have"
        );
    }

    let bare = click_points(&content, None, &[], &[], &probes);
    let numbered = click_points(&content, Some(GUTTER), &[], &[], &probes);
    let decorated = click_points(&content, Some(GUTTER), &diagnostics(), &hints(), &probes);

    assert!(
        bare[0].x < bare[1].x && bare[1].x < bare[2].x,
        "the probes have to land on three different columns, not {bare:?}"
    );

    // Diagnostics and hints are read while drawing and nowhere else, so they cannot
    // move a click by any amount at all — not even a constant one.
    assert_eq!(decorated, numbered);

    let offsets: Vec<f32> = bare
        .iter()
        .zip(&numbered)
        .map(|(bare, numbered)| bare.x - numbered.x)
        .collect();

    assert!(
        offsets[0] > 0.0,
        "the gutter has to take room from the text"
    );

    // The gutter is folded into the editor's left padding and nothing else, so it
    // costs every click the same and costs none of them any height.
    assert!(
        offsets
            .iter()
            .all(|offset| (offset - offsets[0]).abs() < 0.01),
        "the gutter should shift each click alike, not by {offsets:?}"
    );
    assert!(
        bare.iter()
            .zip(&numbered)
            .all(|(bare, numbered)| bare.y == numbered.y)
    );
}

#[test]
fn a_fully_decorated_editor_types_exactly_what_a_bare_one_does() {
    let source = LINES.join("\n");

    let mut bare = Content::with_text(&source);
    let mut decorated = Content::with_text(&source);

    // Right of every line, so both editors clamp the click to the same character
    // however far apart the gutter has pushed their text origins.
    let at = Point::new(PAST_THE_END, LINE_HEIGHT / 2.0);

    let (bare_bounds, bare_actions) = interact(&bare, None, &[], &[], at);
    let (decorated_bounds, decorated_actions) =
        interact(&decorated, Some(GUTTER), &diagnostics(), &hints(), at);

    // `layout` shrinks its limits by the gutter and expands the node by the same
    // amount, so the gutter comes out of the text and never out of the widget.
    assert_eq!(decorated_bounds, bare_bounds);

    let (
        [Action::Click(bare_at, _), bare_rest @ ..],
        [Action::Click(decorated_at, _), decorated_rest @ ..],
    ) = (bare_actions.as_slice(), decorated_actions.as_slice())
    else {
        panic!("the click has to have landed for this to test anything");
    };

    assert!(
        bare_at.x > decorated_at.x,
        "the gutter has to take room from the text"
    );
    assert_eq!(bare_at.y, decorated_at.y);

    // Everything after the click is a keystroke, which knows nothing about where the
    // text begins — so the two editors have to agree on all of it, byte for byte.
    assert_eq!(decorated_rest, bare_rest);

    for action in bare_actions {
        bare.perform(action);
    }

    for action in decorated_actions {
        decorated.perform(action);
    }

    assert_ne!(decorated.text(), source, "the typing has to have landed");
    assert_eq!(decorated.text(), bare.text());
    assert_eq!(decorated.cursor(), bare.cursor());
}

#[test]
fn a_click_places_the_caret_on_the_row_it_lands_in() {
    let source = LINES.join("\n");

    for (line, text) in LINES.iter().enumerate() {
        // The top, the middle and the bottom of the row, which pins where one row
        // ends and the next begins to within a pixel of the line height.
        for offset in [1.0, LINE_HEIGHT / 2.0, LINE_HEIGHT - 1.0] {
            let mut content = Content::with_text(&source);

            // Past the right edge of every line, so the caret can only be the end of
            // the line the click's `y` picked out.
            let at = Point::new(PAST_THE_END, LINE_HEIGHT * line as f32 + offset);

            let actions: Vec<Action> = {
                let mut ui = simulator(editor(&content, None, &[], &[]));

                ui.point_at(at);
                let _ = ui.simulate(simulator::click());

                ui.into_messages()
                    .map(|Message::Edit(action)| action)
                    .collect()
            };

            for action in actions {
                content.perform(action);
            }

            assert_eq!(
                content.cursor(),
                Cursor {
                    position: Position {
                        line,
                        index: text.len(),
                    },
                    selection: None,
                },
                "a click at {at:?}"
            );
        }
    }
}

#[test]
fn an_editor_reports_itself_focused_once_it_has_been_clicked() {
    let content = Content::with_text(&LINES.join("\n"));
    let mut ui = simulator(editor(&content, None, &[], &[]));

    let bounds = ui
        .find(selector::id(EDITOR))
        .expect("the editor carries that id")
        .bounds();

    assert!(
        ui.find(selector::is_focused()).is_err(),
        "a fresh editor is not focused"
    );

    ui.point_at(Point::new(40.0, LINE_HEIGHT / 2.0));
    let _ = ui.simulate(simulator::click());

    // `operate` hands the editor's own focus state to the operation, which is what
    // lets `focus_next` and the rest of iced's focus machinery reach the widget.
    let focused = ui
        .find(selector::is_focused())
        .expect("a click should focus the editor");

    assert_eq!(focused.bounds(), bounds);
}

#[test]
fn an_editor_reports_its_text_to_an_operation() {
    let source = LINES.join("\n");
    let content = Content::with_text(&source);
    let mut ui = simulator(editor(&content, None, &[], &[]));

    // The `&str` selector matches a text input by what it holds, so `operate`'s other
    // half is what lets a test outside this crate find an editor by its contents.
    let found = ui
        .find(source.as_str())
        .expect("the editor should report the text it holds");

    let bounds = ui
        .find(selector::id(EDITOR))
        .expect("the editor carries that id")
        .bounds();

    assert_eq!(found.bounds(), bounds);
}
