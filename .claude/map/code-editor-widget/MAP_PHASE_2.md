# Phase 2 — `CodeEditor` at parity with `TextEditor`

## Prerequisites

Phase 1 complete: `Content` exists and owns a `graphics::text::Editor`.

## Goal

A working widget that behaves **exactly** like `iced::widget::TextEditor` — editing, selection,
wrapping, scrolling, IME, focus, syntax highlighting. No decorations yet. This phase is a careful
port, not a redesign: every deviation from upstream is a future bug.

**Exit criteria:** `examples/showcase.rs` runs and is indistinguishable from the same app built
with `text_editor` — typing, arrow keys, ctrl+A/C/V/Z, double/triple click, drag-select, mouse
scroll, word wrap toggling, and Rust syntax highlighting all work.

## Reference

`/home/nickel/Programming/repos/iced/widget/src/text_editor.rs`, lines 94-620. **Read it before
writing.** The port is close to mechanical; the only substantive change is swapping
`self.content.0.borrow()` (private, 5 call sites) for our own `Content`.

## Step 1 — The widget struct

`src/code_editor/widget.rs`:

```rust
pub struct CodeEditor<'a, Parser, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Parser: text::Parser,
    Theme: text_editor::Catalog,
    Renderer: text::Renderer<Editor = graphics::text::Editor>,
{
    content: &'a Content,
    id: Option<widget::Id>,
    placeholder: Option<text::Fragment<'a>>,
    font: Option<Font>,
    text_size: Option<Pixels>,
    line_height: Option<LineHeight>,
    width: Length,
    height: Length,
    padding: Padding,
    wrapping: Wrapping,
    class: Theme::Class<'a>,
    key_binding: Option<Box<dyn Fn(KeyPress) -> Option<Binding<Message>> + 'a>>,
    on_edit: Option<Box<dyn Fn(Action) -> Message + 'a>>,
    parser_settings: Parser::Settings,
    highlighter: Option<Box<dyn text::Highlighter<Parser::Output, Theme> + 'a>>,
    last_status: Option<Status>,
}
```

The `Renderer: text::Renderer<Editor = graphics::text::Editor>` bound is the crux — it is what lets
us hand our concrete `Content` to `State::draw::<Renderer>` and reach `buffer()` in Phase 3. All
three backends satisfy it (wgpu, tiny-skia, and the fallback).

Defaults from upstream `:125-142`: `width: Length::Fill`, `height: Length::Fit` (**not** `Shrink`),
`padding: Padding::new(5.0)`, `parser_settings: ()`, `highlighter: None`.

## Step 2 — Constructor and helper

`new` is pinned to `parser::PlainText`, exactly as upstream (`:118-144`). Add the function helper
as the primary public entry point, matching iced's own convention:

```rust
/// Creates a [`CodeEditor`] for the given [`Content`].
pub fn code_editor<'a, Message, Theme, Renderer>(
    content: &'a Content,
) -> CodeEditor<'a, text::parser::PlainText, Message, Theme, Renderer>
where
    Theme: text_editor::Catalog + 'a,
    Renderer: text::Renderer<Editor = graphics::text::Editor>,
```

## Step 3 — Builders

Port `:195-314`: `id`, `placeholder`, `width`, `height`, `on_action`, `font`, `size`,
`line_height`, `padding`, `wrapping`, `key_binding`, `style`, and `highlight_with`. Two upstream
quirks to preserve: `width` takes `impl Into<Pixels>` (not `Into<Length>`) and does
`Length::from(width.into())`; `style` is `#[must_use]` and requires
`Theme::Class<'a>: From<StyleFn<'a, Theme>>`.

`highlight_with` changes the `Parser` type parameter, so it rebuilds the struct:

```rust
pub fn highlight_with<P: text::Parser>(
    self,
    settings: P::Settings,
    highlighter: impl text::Highlighter<P::Output, Theme> + 'a,
) -> CodeEditor<'a, P, Message, Theme, Renderer>
```

Skip upstream's `highlight()` convenience wrapper (`:146-186`) — it is pinned to `crate::Theme` and
gated on iced's internal `highlighter` feature. Examples call `highlight_with` directly with
`iced::highlighter::Settings { token: "rs".into() }` and `iced::Code::highlight`.

## Step 4 — Tree state

```rust
struct State<Parser: text::Parser> {
    editor: editor::State,
    parser: RefCell<Parser>,
    parser_settings: Parser::Settings,
    last_theme: RefCell<Option<String>>,
}
```

`editor::State` (`core/src/text/editor.rs:315`) carries focus, IME preedit, click history, drag
flag, and fractional scroll. `tag`/`state` port directly from `:330-341`.

## Step 5 — The `Widget` impl

Port each method from `:343-606`. Specifics that are easy to get wrong:

**`layout` (`:350-384`).** Order matters: re-`update` the parser if `parser_settings` changed,
shrink limits by padding, then `editor.update(..)` with **eight** arguments, then
`limits.resolve(width, height, editor.min_bounds())` and `Node::new(bounds.expand(self.padding))`.

```rust
internal.update(
    limits.bounds(),
    font,
    text_size,
    line_height,
    self.wrapping,
    text::Alignment::Default,     // v1 supports Default alignment only
    renderer.hint_factor(),
    state.parser.borrow_mut().deref_mut(),
);
```

**`update` (`:386-480`).** Return early when `on_edit` is `None`. The whole event loop is one
`state.editor.update(..)` call; its `Option<Update<Message>>` is translated by a local recursive
`fn apply_update` that maps `Copy`/`Paste`/`RedrawAt`/`Focus`/`Unfocus`/`InputMethod`/`Sequence`
onto shell calls. Port `apply_update` verbatim from `:405-436` — it is fiddly and there is no
upside to paraphrasing it. Note the default-binding coercion at `:446`:

```rust
self.key_binding.as_deref().unwrap_or(&Binding::from_key_press as _)
```

Upstream no longer calls `shell.capture_event()`; do not add it.

**`draw` (`:482-567`).** Sequence: borrow content mutably → downcast state → detect theme change
via `theme.name()` against `state.last_theme`, and on change call `state.parser.borrow_mut()
.change_line(0)` → `editor.highlight(font, parser, closure)` → `theme.style(..)` → background
`fill_quad` → placeholder if empty → `state.editor.draw(..)`.

The theme-change invalidation is not optional: without it, switching themes leaves stale token
colors until the next edit.

**`operate` (`:590-606`).** Signature changed upstream — it now takes `&mut self` and a
`_viewport: &Rectangle`. Calls `operation.focusable(..)` with `&mut state.editor` and
`operation.text_input(..)` with the editor. The blanket `impl<T: Editor> TextInput for T`
(`core/src/text/editor.rs:996`) means our editor satisfies the latter for free.

**`mouse_interaction` (`:569-588`)** is pure and ports as-is.

## Step 6 — Styling

Reuse iced's types rather than inventing any: `Theme: text_editor::Catalog`, and
`text_editor::{Status, Style, StyleFn, default}`. A `CodeEditor` then looks identical to a
`TextEditor` with zero theme work, and `Catalog`'s `theme::Base` supertrait is what supplies
`theme.name()` for the invalidation above.

**Two `Style` types will collide** here: `editor::Style { value, selection }` (what `State::draw`
takes) and `text_editor::Style { background, border, placeholder, value, selection }` (the themed
one). `use foo as bar` is banned — import the parent modules and write `editor::Style` and
`text_editor::Style`.

Decide deliberately whether to mirror one upstream quirk: `last_status` lives on the widget
(`:115`), is written only on `RedrawRequested` (`:466-468`), and `update` returns early when
`on_edit` is `None` (`:396-398`) — so a disabled editor renders as `Status::Active`, never
`Status::Disabled`. Mirroring keeps visual parity; fixing it diverges. Either is fine; record the
choice in a comment.

## Step 7 — `examples/showcase.rs`

A Level 0 single-screen app — plain `State`/`Message`/`update`/`view`, **no** `Action<I, M>` or
`Instruction` machinery. Use iced function helpers throughout.

```rust
fn main() -> iced::Result {
    iced::application(Showcase::new, Showcase::update, Showcase::view)
        .font(Font::MONOSPACE)
        .run()
}
```

Seed the content with text that stresses the later phases: a long line that wraps, multibyte
content (`héllo`, CJK, an emoji), a blank line, and enough lines to scroll. Wire
`.on_action(Message::Edit)` → `content.perform(action)` and `.highlight_with::<iced::highlighter::Parser>(..)`.

## Files

| File | Change |
| --- | --- |
| `src/code_editor.rs` | declare `mod widget;`, re-export `CodeEditor` + `code_editor` |
| `src/code_editor/widget.rs` | new — the whole widget |
| `src/lib.rs` | re-export `CodeEditor` |
| `examples/showcase.rs` | new — Level 0 demo app |
| `Cargo.toml` | no change (`[[example]]` autodiscovery handles `examples/`) |

## Verification

```bash
cargo clippy --all-targets -- -D warnings
cargo test
```

Do **not** run `cargo run --example showcase` as an agent — it opens a GUI window and will hang a
headless session. Report that the example is ready and let the human run it.

## Spot checks

| Action | Expectation |
| --- | --- |
| Type `abc` | Text appears; `content.text()` is `"abc"` |
| Double-click a word | That word is selected |
| Triple-click | Whole line selected |
| Drag past the bottom edge | Selection continues (`position_from`, not `position_in`) |
| Mouse wheel | Scrolls ~4 lines per notch, accumulating fractions |
| Ctrl+Z after an edit | Undoes it (`Edit::Undo` is in the default binding) |
| Toggle wrapping | Long line reflows; no panic |
| Switch theme | Syntax colors update immediately, not after the next keypress |
| `.on_action` omitted | Widget is read-only and does not publish messages |

## Do NOT change in this phase

- No decoration types, no `geometry.rs`, no `gutter.rs` — Phases 3-6.
- Do not call `content.0.borrow().buffer()` yet. Decoration geometry belongs in Phase 3 and must
  run **after** `editor.highlight(..)` in `draw` (see MAP_PLAN Risk B).
- Do not reimplement `State::draw` to reorder drawing. Within a layer, backends draw quads before
  text regardless of call order — reordering calls achieves nothing (MAP_PLAN correction 2).
- Do not reimplement key handling, click detection, or scroll accumulation. `editor::State::update`
  already does all of it.
- Do not introduce `text_padding()` yet — Phase 4 adds it and enumerates the five call sites.
- Do not add `Action<I, M>` to the example.
