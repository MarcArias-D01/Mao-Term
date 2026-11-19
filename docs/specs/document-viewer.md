# Spec: Document viewer

- **Status:** Draft
- **Milestone:** M5
- **Crates:** `mao-docs`, `mao-ui`, `mao-render`
- **Related:** [file-tree](file-tree.md), [workspace-layout](workspace-layout.md)

## Goal

Open and read documents without leaving the terminal: a README, a log, an image, a JSON… in a pane alongside.

## Scope

- `Document` pane type, openable from: the file tree, the palette (`Open file…`), the `mao open <path>` command from the shell, Ctrl+click on a path in the output.
- v1 formats:

| Type | Render |
|---|---|
| Markdown | Rendered (headings, lists, tables, highlighted code, links, local images). Toggle to source. |
| Code / text | `syntect` highlighting, line numbers, optional wrapping. |
| JSON / TOML / YAML | Highlighting; optional folding for JSON. |
| Images (png, jpg, gif, webp, bmp, ico) | Fit to pane, zoom, pan. |
| SVG | Rasterized with `resvg`. |
| Logs / large files | Optional "tail" mode following the end of the file. |

- Search inside the document (`Ctrl+F`), copy selection.
- Auto-reload when the file changes on disk.
- "Open in editor" button (`$EDITOR` / default app).

## Out of scope

- Editing (Future).
- PDF, Office (Future; PDF would require pdfium or similar).
- HTML rendering.

## Technical design

- `mao-docs` loads and parses on a background thread → document model (Markdown blocks, highlighted lines, decoded image).
- Large files: chunked reads/`mmap`, lazy highlighting of visible content only, configurable limit (e.g. > 50 MB → plain text mode).
- Type detection by extension, otherwise by content (binary → "cannot display" + size/type).

## Performance

- Opening a 10 MB file: first screen visible < 150 ms.
- Scrolling at 60 fps in large documents.

## Acceptance criteria

- [ ] This repo's README renders with tables and code blocks.
- [ ] A 500 MB log opens without freezing and can be navigated.
- [ ] Editing the file externally refreshes the viewer.
- [ ] Binary files don't hang or show garbage.

## Open questions

- `mao open` as a separate binary or as an OSC sequence sent from the shell?
