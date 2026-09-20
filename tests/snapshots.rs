//! Pixel baselines for the decorations.
//!
//! Every test here is `#[ignore]`d, so `cargo test` skips them. They compare
//! rendered pixels, which move with the renderer, the fonts and the platform,
//! and they are the only tests in this crate that can fail for a reason that is
//! not a bug.
//!
//! **There are no baselines in this repository yet, and running these tests is
//! how the first ones get written.** `Snapshot::matches_image` creates the file
//! and returns `Ok(true)` whenever it does not already exist
//! (`iced/test/src/simulator.rs:261-301`), so the first run records whatever
//! renders that day — a bug included. Nothing in this crate has ever been
//! checked by eye: six phases of decoration geometry were verified by a
//! recording renderer and by construction. So, in order:
//!
//! 1. `cargo run --example showcase`, and confirm the decorations look right.
//! 2. `cargo test -- --ignored`, which writes `snapshots/*-tiny-skia.png`.
//! 3. Open each of those files and confirm it shows what its test name claims.
//! 4. Commit them. From then on these tests compare rather than record.
//!
//! Images rather than the hashes iced itself commits, because step 3 is the
//! whole point of the sequence and a `.sha256` file cannot be looked at. The
//! viewport is small for the same reason: a baseline stays a few kilobytes and
//! a reviewer can still see a squiggle. `.cargo/config.toml` pins the test
//! renderer to tiny-skia and `Snapshot::path` suffixes the file name with it,
//! so a baseline can never be mistaken for one from another backend.

use iced::widget::text;
use iced::{Color, Element, Pixels, Settings, Size, Theme};
use iced_test::{Error, Simulator};

use matcha::decoration::{TextRange, diagnostic, inlay};
use matcha::{Action, Content, Position, code_editor, gutter};

/// Small enough that a baseline is a few kilobytes and large enough that every
/// line of [`SOURCE`] is on screen.
const SIZE: Size = Size::new(480.0, 200.0);

const SOURCE: &str = "fn main() {\n    let answer = compute();\n\n    println!(\"{answer}\");\n}\n";

const GUTTER: gutter::Style = gutter::Style {
    color: Color::from_packed_rgb8(0x928374),
    spacing: 8.0,
};

/// Lays [`SOURCE`] out with the decorations the caller asks for.
///
/// Nothing here ever reads a message, so the editor publishes its
/// [`Action`]s as they are rather than wrapping them in a message type no
/// assertion would look at.
///
/// The default font rather than a monospace one: the bundled Fira Sans is what
/// keeps shaping — and therefore every pixel below — off whatever the host
/// machine happens to have installed.
fn interface<'a>(
    content: &'a Content,
    gutter: Option<gutter::Style>,
    diagnostics: &'a [diagnostic::Diagnostic],
    hints: &'a [inlay::Hint<'a>],
) -> Simulator<'a, Action> {
    let editor = code_editor(content)
        .size(Pixels(14.0))
        .line_height(Pixels(20.0))
        .wrapping(text::Wrapping::None)
        .on_action(|action| action)
        .diagnostics(diagnostics)
        .inlay_hints(hints);

    let editor = match gutter {
        Some(style) => editor.gutter(style),
        None => editor,
    };

    // Nothing is clicked before a snapshot is taken, so the editor stays unfocused and
    // no blinking caret lands in the baseline.
    Simulator::with_size(Settings::default(), SIZE, Element::<Action>::from(editor))
}

/// An error over `compute()`, and a zero-width warning on the blank line.
fn diagnostics() -> Vec<diagnostic::Diagnostic> {
    let mark = |line, start, end, severity| diagnostic::Diagnostic {
        range: TextRange::new(
            Position { line, index: start },
            Position { line, index: end },
        ),
        severity,
    };

    vec![
        mark(1, 17, 26, diagnostic::Severity::Error),
        mark(2, 0, 0, diagnostic::Severity::Warning),
    ]
}

/// A type after `answer`, where it paints over the code, and a return type on
/// the blank line, where it does not.
fn hints() -> Vec<inlay::Hint<'static>> {
    let hint = |line, index, label: &'static str| inlay::Hint {
        position: Position { line, index },
        label: label.into(),
    };

    vec![hint(1, 14, ": u32"), hint(2, 0, "→ ()")]
}

#[test]
#[ignore = "records a baseline on its first run; see this file's docs"]
fn squiggles_run_under_the_glyphs_they_mark() -> Result<(), Error> {
    let content = Content::with_text(SOURCE);
    let diagnostics = diagnostics();
    let mut ui = interface(&content, None, &diagnostics, &[]);

    assert!(
        ui.snapshot(&Theme::Dark)?
            .matches_image("snapshots/squiggles")?
    );

    Ok(())
}

#[test]
#[ignore = "records a baseline on its first run; see this file's docs"]
fn hints_sit_beside_the_code_they_annotate() -> Result<(), Error> {
    let content = Content::with_text(SOURCE);
    let hints = hints();
    let mut ui = interface(&content, None, &[], &hints);

    assert!(
        ui.snapshot(&Theme::Dark)?
            .matches_image("snapshots/hints")?
    );

    Ok(())
}

#[test]
#[ignore = "records a baseline on its first run; see this file's docs"]
fn the_gutter_numbers_every_line_once() -> Result<(), Error> {
    let content = Content::with_text(SOURCE);
    let mut ui = interface(&content, Some(GUTTER), &[], &[]);

    assert!(
        ui.snapshot(&Theme::Dark)?
            .matches_image("snapshots/gutter")?
    );

    Ok(())
}

#[test]
#[ignore = "records a baseline on its first run; see this file's docs"]
fn every_decoration_renders_at_once() -> Result<(), Error> {
    let content = Content::with_text(SOURCE);
    let diagnostics = diagnostics();
    let hints = hints();
    let mut ui = interface(&content, Some(GUTTER), &diagnostics, &hints);

    assert!(
        ui.snapshot(&Theme::Dark)?
            .matches_image("snapshots/everything")?
    );

    Ok(())
}
