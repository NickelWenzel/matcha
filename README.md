# matcha

A code editor widget for [iced](https://github.com/iced-rs/iced).

It edits, selects, wraps, scrolls, and highlights exactly as `iced::widget::TextEditor` does, and
adds the three things a code editor needs and the stock widget cannot draw:

- **diagnostic squiggles** — wavy underlines over a range of text, one per severity,
- **inlay hints** — small labels on opaque chips, laid over the code without moving any of it,
- **a line-number gutter** — folded into the editor's own padding, so hit-testing stays exact.

It is a single library crate built on public iced APIs. No fork, no patch, and no dependency
other than iced itself.

To see it, run the showcase, which stresses wrapping, multibyte shaping, scrolling, and all three
decorations at once, and carries a toggle for the hints:

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
   The widget takes them in its own units whatever else is switched on. Converting from the
   protocol's is [the `lsp` feature's job](#speaking-to-a-language-server), or yours.
2. **Inlay hints are opaque overlays.** Each label sits on a filled, outlined chip drawn in a
   layer above the text, so a hint inside a line hides the code there — which is why hints read
   best shown momentarily. Showing them is yours to decide: `&hints` or `&[]`, from a toggle or
   a held key, is the whole of the control. They never change layout, wrapping, hit-testing, or
   the caret.
3. **Stale positions are ignored, never fatal.** A decoration naming a line the buffer no longer
   has is silently not drawn, so decorations can be replaced wholesale on every round-trip
   without being checked first.

## Speaking to a language server

Off by default. The `lsp` feature converts a server's positions into the editor's and applies
its edits; `lsp-types` and `gen-lsp-types` add conversions to and from those crates, so an
application does not write the mapping itself.

```toml
matcha = { git = "https://github.com/NickelWenzel/matcha", features = ["lsp-types"] }
```

Both crates are asked for by range rather than by version, so Cargo settles on whichever copy
your application already has — which matters, because the two are incompatible and widely used
crates disagree about which to depend on.

The widget itself is unchanged by any of this. It still takes positions in its own units and
knows nothing about the protocol; the bridge sits beside it and hands it the same decorations an
application would build by hand.

```sh
cargo run --example lsp --features lsp-types
```

feeds real `publishDiagnostics`, `inlayHint` and `codeAction` payloads through it. Press **1**,
**2**, **3** to deliver them and **a** to apply the action — edit in between and it is refused,
because its edits describe text that has moved.

## Not in scope

- **Virtual text.** Hints are overlays; nothing here pushes code aside to make room.
- **Center and right text alignment.** `text::Alignment::Default` only: the fork history of
  `Buffer::hit` is evidence enough that alignment makes hit-testing subtle, and the decoration
  geometry assumes a left origin throughout.

## License

MIT. See [LICENSE](LICENSE).
