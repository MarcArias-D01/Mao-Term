# Spec: Document viewer

- **Status:** In progress
- **Milestone:** M5
- **Crates:** `mao-docs`, `mao-render`, `mao-app`
- **Related:** [file-tree](file-tree.md), [workspace-layout](workspace-layout.md), [shell-integration](shell-integration.md), [ADR 0004](../decisions/0004-document-viewer.md)

## Goal

Open and read documents without leaving the terminal: a README, a log, an image, a JSON… in a pane alongside.

## Scope

- `Document` pane type that lives in the split tree next to terminals. Opened from:
  - the file tree: single click previews (the tree keeps focus), double click / `Enter` opens; context menu "Open" (+ "Open with Default App");
  - the shell: `mao open <file>` (function defined by the shell-integration scripts, sends OSC 7770);
  - (later) the palette (`Open file…`) and Ctrl+click on a path in the output.
- The active tab's document pane is reused (preview-style); otherwise the focused pane is split to the right.
- v1 formats:

| Type | Render |
|---|---|
| Markdown | Rendered (headings, emphasis, lists/task lists, quotes, tables, highlighted code blocks, links, image placeholders). Toggle to highlighted source. |
| Code / text | `syntect` highlighting (+ bat's grammars), line numbers, word wrap toggle (`Alt+Z`), horizontal scroll when not wrapping. |
| JSON / TOML / YAML / logs | Highlighting like any code. |
| Images (png, jpg, gif, webp, bmp, ico) | Fit to pane (never enlarged), zoom (`+`/`-`/`0`/`1`, Ctrl-free wheel), pan (drag, arrows). Mipmapped. |
| SVG | Rasterized with `resvg` (2048 px), fit may enlarge. |
| Binary | "Binary file (size) — it can't be shown as text." |

- Header bar: file name, kind · size · lines (or size · zoom), buttons *Source*, *Wrap*, *Fit*, *Open ↗* (default app).
- Search inside the document (`Ctrl+F`, `Enter`/`Shift+Enter`, `F3`), all matches highlighted. Mouse selection, `Ctrl+A`, copy (`Ctrl+C`).
- Auto-reload when the file changes on disk; if scrolled to the end it stays at the end (log tail).
- Keyboard: arrows, PgUp/PgDn/Space, Home/End, `F5` reload, `Esc` clears selection/search.

## Out of scope

- Editing (Future).
- PDF, Office (Future; PDF would require pdfium or similar).
- HTML rendering.

## Technical design

- `mao-docs` (no UI dependencies):
  - `DocService`: worker thread + `notify` watcher on the folders of open documents (refcounted per path and folder, debounced 150 ms). Dropped when the last document pane closes.
  - `Loader`: type by extension, else content (NUL bytes → binary; UTF-16 BOMs decoded). Text read up to 64 MB (`truncated` flag). Plain text is sent first when the grammars aren't loaded yet or the file is > 32 KB; the highlighted version follows (files > 4 MB are never highlighted).
  - Model: `TextDoc` of logical `Line`s = `StyledText` (text + style runs by byte offset), a continuation prefix (hanging indent, quote bars), optional row fill (code blocks), `rule` and `nowrap` flags. `layout::layout` word-wraps it into `Row`s (line, byte range) for a width.
  - Markdown via `pulldown-cmark` (tables, strikethrough, task lists, footnotes).
  - Images via `image` (+ mip chain), SVG via `resvg`.
- `mao-render`: `ImagePipeline` (textured quads, per-draw scissor clip), `Renderer::set_image/remove_image/max_texture_size`, images drawn after grid text and before the UI.
- `mao-app`: `DocPane` (layout cache keyed by width/wrap/view, scroll, selection, search, zoom/pan), rendered into a grid only when dirty; chrome draws the header.

## Performance

- Opening a 10 MB file: first screen visible < 150 ms. **Measured: 128 ms** (10 MB log, release, Intel iGPU laptop). Code files: first (plain) screen in ~2 ms, highlighted after ~300 ms the first time (grammar loading).
- Scrolling only rebuilds the visible rows; nothing runs while idle (**measured: 0% CPU** with a Markdown, code or image document open).
- Memory: +~50 MB while a text document is open (grammars), +image size ×1.33 (mips) for images.

## Acceptance criteria

- [x] This repo's README renders with tables and code blocks.
- [ ] A 500 MB log opens without freezing and can be navigated (today: the first 64 MB are shown, marked as truncated).
- [x] Editing the file externally refreshes the viewer.
- [x] Binary files don't hang or show garbage.

## Open questions

- ~~`mao open` as a separate binary or as an OSC sequence sent from the shell?~~ → OSC 7770 from a shell function ([ADR 0004](../decisions/0004-document-viewer.md)).
- Local images inside Markdown: draw them inline (needs mixed text/image rows) or keep the placeholder?
