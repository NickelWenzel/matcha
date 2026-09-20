# matcha

A code editor widget for [iced](https://github.com/iced-rs/iced).

It edits, selects, wraps, scrolls, and highlights exactly as `iced::widget::TextEditor` does, and
adds the three things a code editor needs and the stock widget cannot draw:

- **diagnostic squiggles** — wavy underlines over a range of text, one per severity,
- **inlay hints** — small labels drawn among the code without displacing any of it,
- **a line-number gutter** — folded into the editor's own padding, so hit-testing stays exact.

It is a single library crate built on public iced APIs. No fork, no patch, and no dependency
other than iced itself.

To see it, run the showcase, which stresses wrapping, multibyte shaping, scrolling, and all three
decorations at once:

```sh
cargo run --example showcase
```

`cargo run --example live` is the other one: it rebuilds every decoration from the text on each
edit, the way a language server delivers them, and includes a position deliberately left to go
stale.

## Installation

matcha is built against unreleased iced and is not on crates.io. Depend on the same revision it
pins, or the two crates end up with two copies of `iced_core` whose types will not unify:

```toml
[dependencies]
iced = { git = "https://github.com/iced-rs/iced", rev = "fa3bae52874274c012a27d4bf11a83c49e1709ae" }
matcha = { git = "https://github.com/NickelWenzel/matcha" }
```

A renderer backend is mandatory. `wgpu` and `tiny-skia` are both default features of iced; with
both turned off, `iced::Renderer` resolves to `()` and the widget stops typechecking at the call
site.

## Usage

```rust
use iced::{Color, Element, Fill};

use matcha::decoration::{diagnostic, inlay};
use matcha::{Action, Content, code_editor, gutter};

struct Editor {
    content: Content,
    diagnostics: Vec<diagnostic::Diagnostic>,
    hints: Vec<inlay::Hint<'static>>,
}

#[derive(Debug, Clone)]
enum Message {
    Edit(Action),
}

impl Editor {
    fn update(&mut self, message: Message) {
        match message {
            Message::Edit(action) => self.content.perform(action),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        code_editor(&self.content)
            .height(Fill)
            .on_action(Message::Edit)
            .diagnostics(&self.diagnostics)
            .inlay_hints(&self.hints)
            .gutter(gutter::Style {
                color: Color::from_rgb8(0x92, 0x83, 0x74),
                spacing: 12.0,
            })
            .into()
    }
}
```

Three things to know before wiring a language server up to it:

1. **Positions are UTF-8 byte indices** into a line — not UTF-16 code units, and not columns.
   Converting from LSP's UTF-16 is your job; matcha has no `lsp-types` dependency.
2. **Inlay hints are overlays.** They may paint over source text, and they never change layout,
   wrapping, hit-testing, or the caret.
3. **Stale positions are ignored, never fatal.** A decoration naming a line the buffer no longer
   has is silently not drawn, so decorations can be replaced wholesale on every round-trip
   without being checked first.

## Not in scope

- **Virtual text.** Hints are overlays; nothing here pushes code aside to make room.
- **The LSP protocol,** including UTF-16 ↔ UTF-8 conversion. The widget takes positions already
  in its own units.
- **Center and right text alignment.** `text::Alignment::Default` only: the fork history of
  `Buffer::hit` is evidence enough that alignment makes hit-testing subtle, and the decoration
  geometry assumes a left origin throughout.
- **Opaque chips behind inlay hints.** Both backends draw every quad in a layer before any of its
  text, so a background quad would land beneath the code rather than behind the label.

## License

MIT. See [LICENSE](LICENSE).
