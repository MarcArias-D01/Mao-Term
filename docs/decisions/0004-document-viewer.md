# ADR 0004 — Document viewer: cell grid rendering, syntect, `mao open` over OSC

- **Status:** Accepted (implemented in M5)
- **Date:** 2026-11-20

## Context

M5 adds a document pane (Markdown, code, text, images) next to the terminals. Three choices shape it:

1. How text documents are drawn.
2. How syntax highlighting is done.
3. How a shell asks Mao-Term to open a file (`mao open <file>`), given the shell may run inside ConPTY, WSL or ssh.

## Decision

1. **Text documents are drawn on the same monospace cell grid as terminals** (`mao-render` grids), in the spirit of `glow`: Markdown becomes styled lines (bold/italic/colors, bullets, quote bars, box-drawn tables, filled code blocks) that are word-wrapped into rows. Images and SVGs use a new textured-quad pipeline (mipmapped, clipped per pane).
2. **`syntect` with bat's extra grammars (`two-face`)**, pure-Rust regex engine (`fancy-regex`) so it builds everywhere without C code. Loaded lazily on the loader thread; the plain text is shown first and the highlighted version replaces it.
3. **`mao open` is a shell function from the shell-integration scripts that prints `OSC 7770 ; open ; file://host/path BEL`**, parsed by the same scanner as OSC 133/7. No extra binary, no IPC socket. ConPTY passes the unknown OSC through (verified on Windows 11).

## Alternatives considered

- *Proportional rich text* (glyphon rich layout, real heading sizes): nicer typography, but a second layout engine, its own scrolling/selection/search, and a lot more code. The cell grid reuses the renderer, selection model and performance work we already have. Can be revisited for Markdown only.
- *tree-sitter* highlighting: faster and more precise, but one C grammar crate per language and a bigger build; syntect covers hundreds of languages from one dependency.
- *`mao` as a separate binary talking over a socket/named pipe*: works without shell integration, but needs IPC, discovery of the right window/pane, and breaks over ssh. The OSC travels with the terminal stream, so it works wherever the output reaches us (including ssh, if the remote shell defines `mao`).

## Consequences

- ✅ One rendering path for terminals and documents; documents get selection, search and 0% idle CPU for free.
- ✅ `mao open` works in every integrated shell without new processes.
- ❌ Markdown headings can't be bigger than body text; emoji with variation selectors may misalign (same limitation as the terminal grid).
- ❌ The grammars cost ~50 MB of memory while a document is open (freed when the last document pane closes).
