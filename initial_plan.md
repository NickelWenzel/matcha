Below is the plan I would hand directly to Codex or Claude Code. It deliberately stops before virtual text: **inlay hints are visual overlays only and never change wrapping, cursor positions, hit-testing, or the underlying text layout.**

I verified this against the current `iced` master as of September 16, 2026. The workspace is currently `0.15.0-dev`. The relevant public APIs are sufficient for this approach: `iced::advanced` exposes custom-widget and graphics APIs, `graphics::text::Editor::buffer()` exposes the shaped `cosmic_text::Buffer`, and `text::editor::State` exposes the existing interaction behavior. ([GitHub][1])

# Implementation plan: standalone decorated iced text editor

## Goal

Create a standalone `CodeEditor`/`LspTextEditor` widget derived from iced's current `TextEditor`, without modifying or forking iced.

The widget should support:

* normal `TextEditor` behavior
* current `Parser`/`Highlighter` API
* LSP semantic highlighting
* diagnostic ranges
* error/warning/info squiggles
* non-layout-affecting inlay hints
* wrapping
* scrolling
* selections
* cursor
* Unicode
* renderer scaling/hinting

Explicitly **do not** implement virtual/injected text. Inlay hints must not alter document layout.

The widget should depend only on public iced APIs.

---

## Architectural constraint

Do **not** wrap `iced::widget::TextEditor`.

Create a new widget based on its implementation.

iced's normal `TextEditor` owns a reference to a `Content`, but its actual renderer editor is hidden behind private fields, so a wrapper cannot obtain the shaped text geometry needed for decorations. 

Instead, own the graphics editor directly:

```rust
pub struct Content {
    editor: RefCell<graphics::text::Editor>,
}
```

and constrain the custom widget renderer to:

```rust
Renderer:
    text::Renderer<Editor = graphics::text::Editor>
```

This intentionally targets iced's standard graphics text editor instead of arbitrary third-party text renderers.

That gives access to:

```rust
content.editor.borrow().buffer()
```

because `graphics::text::Editor::buffer()` is public. 

---

## Proposed source structure

```text
src/code_editor/
    mod.rs
    content.rs
    widget.rs
    decoration.rs
    geometry.rs
```

Keep the LSP protocol itself outside this module.

The editor should accept already-normalized document positions.

Use iced's existing:

```rust
iced::advanced::text::Position
```

whose `index` is explicitly a UTF-8 byte index into a line. 

Therefore:

```text
LSP UTF-8/UTF-16/UTF-32 positions
             ↓
application/LSP adapter
             ↓
iced text::Position { line, index: UTF-8 byte index }
             ↓
CodeEditor
```

Do not put LSP position-encoding conversion inside the widget.

---

# Implementation phases

1. **Create the custom `Content` type.** Mirror the useful public interface of `iced::widget::text_editor::Content`: `new`, `with_text`, `perform`, `move_to`, `cursor`, `line_count`, `line`, `lines`, `text`, `selection`, `is_empty`, and optionally `line_ending`. Internally store `RefCell<graphics::text::Editor>`. Use the public `text::Editor` trait for editor operations. The existing iced `Content` is essentially just a private `RefCell` around `Renderer::Editor`, so very little logic needs to be reproduced. 

2. **Copy the structural behavior of `TextEditor`, not its private implementation details.** Implement `Widget` using `iced::advanced::Widget`. Reproduce `tag`, `state`, `size`, `layout`, `update`, `draw`, `mouse_interaction`, and `operate`. Keep `text::editor::State` as the input/focus state instead of implementing keyboard/mouse editing yourself. Its public `update`, `input_method`, `is_focused`, and cursor behavior handle keyboard input, mouse editing, selection, scrolling, IME and focus.  The current iced `TextEditor` implementation at lines ~301–585 should be treated as the primary reference. 

3. **Preserve iced's parser/highlighter architecture unchanged.** Give the custom widget the same conceptual fields:

```rust
parser_settings: Parser::Settings,
highlighter:
    Option<Box<dyn text::Highlighter<Parser::Output, Theme> + 'a>>,
```

and widget state:

```rust
struct State<P: text::Parser> {
    editor: text::editor::State,
    parser: RefCell<P>,
    parser_settings: P::Settings,
    last_theme: RefCell<Option<String>>,
}
```

Implement `highlight_with` in the same way as iced. During layout, call:

```rust
editor.update(..., parser)
```

and during drawing call:

```rust
editor.highlight(...)
```

before accessing decoration geometry. This ensures the `cosmic_text` layout is already shaped. Current iced performs these two operations in precisely these phases. 

4. **Reuse iced's existing `text_editor` theme API instead of inventing one.** Prefer:

```rust
Theme: iced::widget::text_editor::Catalog
```

and reuse `text_editor::Status` and `text_editor::Style`. That keeps the new widget visually compatible with normal iced editors. Only define additional styling for decorations:

```rust
#[derive(Debug, Clone, Copy)]
pub struct DiagnosticStyle {
    pub color: Color,
    pub thickness: f32,
    pub amplitude: f32,
    pub wavelength: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct InlayStyle {
    pub color: Color,
    pub size_scale: f32,
    pub offset: Vector,
}
```

Do not add these to iced's theme system initially; expose builder methods such as `.diagnostic_style(...)` and `.inlay_style(...)`.

5. **Define a small editor-native decoration model.** Keep it independent from `lsp_types`:

```rust
#[derive(Debug, Clone, Copy)]
pub struct TextRange {
    pub start: text::Position,
    pub end: text::Position,
}

#[derive(Debug, Clone, Copy)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

pub struct DiagnosticDecoration {
    pub range: TextRange,
    pub severity: DiagnosticSeverity,
}

pub struct InlayHint<'a> {
    pub position: text::Position,
    pub label: Cow<'a, str>,
}
```

Expose them with:

```rust
CodeEditor::diagnostics(&[DiagnosticDecoration])

CodeEditor::inlay_hints(&[InlayHint])
```

Treat all positions as immutable display data. Invalid/stale positions should be ignored instead of panicking.

6. **Implement a dedicated `geometry.rs` using the public cosmic-text buffer.** This is the most important part. Reproduce iced's private `highlight_line` and `visual_lines_offset` algorithms locally rather than copying private APIs through hacks. iced's own range-selection implementation already derives visual rectangles from `BufferLine::layout_opt()`, glyph `start/end/x/w`, scroll offsets and line height.  Implement:

```rust
fn range_fragments(
    editor: &graphics::text::Editor,
    range: TextRange,
) -> Vec<Rectangle>;

fn position_anchor(
    editor: &graphics::text::Editor,
    position: text::Position,
) -> Option<Point>;
```

Return coordinates **relative to the editor's text origin**, exactly like `Editor::selection()`. Account for `buffer.scroll().horizontal`, `buffer.scroll().vertical`, wrapped visual lines and `editor.hint_factor().unwrap_or(1.0)`. The graphics editor already performs the same scaling for its cursor and selection geometry. 

7. **Make `range_fragments` closely match iced selection semantics.** For every logical source line touched by the range, compute the start byte and end byte relevant to that line. Iterate all wrapped visual lines for that `BufferLine`. For each visual line, find intersecting glyphs and calculate `x` and `width`. Calculate `y` from the number of preceding visual lines, `buffer.metrics().line_height`, and vertical scroll. Return one rectangle per visible wrapped fragment. A diagnostic crossing wrapping should therefore naturally produce:

```text
some_really_long_expression(...)
            ~~~~~~~~~~~~~~~~~~~
~~~~~~~~~~~~~~~~~~~~
```

rather than one incorrect rectangle.

8. **Implement `position_anchor` by adapting iced's caret calculation.** Use the shaped line containing `Position.line`; find the wrapped visual line containing `Position.index`; sum the widths of glyphs before the position; add the first glyph's `x`; subtract horizontal scroll; calculate Y using `visual_lines_offset`; divide layout coordinates by the editor hint factor. iced's caret calculation already implements almost exactly this algorithm.  Because a code editor should always use left/default alignment, explicitly support `text::Alignment::Default` only for v1 instead of reproducing center/right alignment edge cases.

9. **Render diagnostics without introducing the canvas/geometry feature initially.** Use `Renderer::fill_quad`, which is part of the base renderer API.  For every `range_fragment`, position the underline close to the bottom of the text line and create a small pixel-style wave from short rectangles:

```text
 _   _   _
  |_| |_|
```

For example, alternate a 1–2 px segment between `baseline` and `baseline + amplitude`. Clip every generated quad against the editor text viewport. This avoids introducing a `geometry::Renderer` bound solely for squiggles. Keep the wave implementation isolated as:

```rust
fn draw_squiggle<R: renderer::Renderer>(
    renderer: &mut R,
    fragment: Rectangle,
    clip_bounds: Rectangle,
    style: DiagnosticStyle,
)
```

A later visual-quality improvement can replace this with a stroked geometry path without changing the public decoration model.

10. **Render inlay hints with `text::Renderer::fill_text`.** `fill_text` is already part of the public text renderer API.  Resolve each hint's anchor with `position_anchor`, then render a smaller text label at:

```rust
text_origin
    + anchor
    + inlay_style.offset
```

with `Wrapping::None`.

A sensible initial default is approximately:

```rust
size = editor_text_size * 0.75;
offset = Vector::new(2.0, -size * 0.25);
```

but keep the offset configurable.

These hints are intentionally overlays:

```text
source layout:        foo(value)
                          ↑
hint overlay:          param:
```

They must **not** cause `value` or any later source text to move.

Document that overlapping source text is allowed in v1. Applications can choose an offset or restrict hints to locations where the overlay is readable. End-of-line hints should look especially good with this model.

11. **Keep decoration geometry completely passive.** Decorations must never be passed to `editor.update`, `perform`, hit testing, mouse handling, selection logic or IME positioning. This guarantees that introducing hints cannot alter existing editor behavior. The invariant should be:

```text
with decorations:
content.text()       == without decorations
content.cursor()     == without decorations
layout/min_bounds    == without decorations
mouse hit testing    == without decorations
wrapping             == without decorations
```

Only pixels produced during `draw()` change.

12. **Take control of drawing order instead of just calling `State::draw`.** `State::draw` currently bundles text rendering with selection/caret rendering.  Reproduce its small drawing routine locally so the custom widget can use this order:

```text
editor background
      ↓
source text
      ↓
selection highlight
      ↓
diagnostic squiggles
      ↓
inlay hints
      ↓
caret
```

Use `state.editor.is_focused()` and `state.editor.is_cursor_visible()` for caret behavior, and `editor.selection()` for caret/range geometry. Those APIs are public. Preserve iced's existing clipping and crisp-cursor behavior as closely as practical.

13. **Optimize decorations only after correctness works.** Do not initially build an interval tree or cache glyph geometry. The `cosmic_text::Buffer` already stores shaped layouts. Start by filtering diagnostics/hints to visible logical lines based on `buffer.scroll().line` and the viewport. If profiling shows a problem, later store decorations indexed by line:

```rust
BTreeMap<usize, Vec<DiagnosticDecoration>>
BTreeMap<usize, Vec<InlayHint>>
```

or build a pre-indexed decoration snapshot outside the widget. Avoid storing application/LSP state inside widget tree state.

14. **Add unit tests for geometry separately from widget rendering.** `geometry.rs` should contain most of the testable complexity. Cover ASCII positions, multibyte UTF-8, a diagnostic inside one line, multiline diagnostics, wrapped lines, ranges at beginning/end of line, positions at wrap boundaries, horizontal scrolling, vertical scrolling and invalid/stale positions. The expected behavior for invalid indexes should be `None`/empty fragments—not panic.

15. **Add an example app as the final integration test.** Use fixed source text containing long wrapped lines and Unicode. Add several hard-coded diagnostics and hints, plus normal syntax highlighting. Verify visually that scrolling and wrapping move decorations with the text, selection/editor input remain normal, hints don't shift any source text, and squiggles continue underneath wrapped diagnostics. Then add one simple mutable example where editing invalidates/replaces the decoration vectors to simulate LSP responses.

# Suggested public API

The end result should be approximately:

```rust
code_editor(&state.content)
    .on_action(Message::Edit)
    .highlight_with::<LspParser>(
        state.semantic_highlighting.clone(),
        highlight_token,
    )
    .diagnostics(&state.diagnostics)
    .inlay_hints(&state.inlay_hints)
    .diagnostic_style(|severity| match severity {
        DiagnosticSeverity::Error => error_style,
        DiagnosticSeverity::Warning => warning_style,
        _ => default_style,
    })
    .inlay_style(InlayStyle {
        color: hint_color,
        size_scale: 0.75,
        offset: Vector::new(2.0, -2.0),
    })
```

The application remains responsible for converting LSP data:

```rust
lsp_types::Diagnostic
        ↓
UTF-16 → UTF-8 conversion
        ↓
DiagnosticDecoration

lsp_types::InlayHint
        ↓
UTF-16 → UTF-8 conversion
        ↓
InlayHint
```

This keeps the widget useful without any dependency on `lsp-types`.

## Definition of done

Codex/Claude should consider the implementation complete when `CodeEditor` can replace the current iced `TextEditor` in the application without losing editing functionality; normal iced parser/highlighter syntax highlighting still works; squiggles track diagnostic ranges correctly through wrapping and scrolling; Unicode byte positions work; hints track their source positions while scrolling/wrapping but do not affect layout; selections, clicks and cursor behavior are unchanged by hints; the implementation contains no patched/forked iced code and no access to private iced internals; and the decoration-to-screen-coordinate calculations have focused tests.

One especially important instruction for the coding agent: **do not start implementing a second `cosmic_text::Buffer`, logical↔visual mapping, custom mouse hit testing, or virtual text.** If it starts doing that, it has drifted into the full inline-inlay architecture we explicitly excluded.

The three iced sources worth keeping open while implementing are the current `TextEditor`, graphics `Editor`, and core editor `State`; together they contain virtually all the behavior that needs to be adapted. 

[1]: https://github.com/iced-rs/iced/blob/master/Cargo.toml?utm_source=chatgpt.com "iced/Cargo.toml at master · iced-rs/iced · GitHub"
