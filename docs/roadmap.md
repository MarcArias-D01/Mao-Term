# Roadmap

Status: ⬜ pending · 🟨 in progress · ✅ done

## M0 — Foundations ✅

- ✅ Cargo workspace with the crates from `design.md` (empty), `rust-toolchain.toml`, edition 2024.
- ✅ CI (GitHub Actions): fmt, clippy, test on `windows-latest` and `ubuntu-latest`. `cargo-deny` for licenses. *(Not run yet: no remote.)*
- ✅ winit + wgpu window that clears the screen with the theme background color.
- ✅ Minimal config loading (font, size, colors).
- ✅ Spike: validate `alacritty_terminal::tty` with ConPTY on Windows (see ADR 0001). Output and input both work (`crates/mao-terminal/tests/spawn.rs`); `portable-pty` not needed.

**M0 measurements** (release, Windows 11, Intel Iris Xe, empty window):

| Metric | DX12 | Vulkan | GL |
|---|---|---|---|
| Startup to first frame (warm) | ~150 ms | ~125–155 ms | — |
| Idle CPU (10 s) | 0.000% | 0.000% | 0.47% |
| Working set / private | 106 / 100 MB | 113 / 69 MB | 104 / 38 MB |

Cold start (first launch after build) ~300–500 ms due to driver shader caches. Most of the memory is the GPU driver: the RAM budget in `design.md` must be measured relative to this baseline.

## M1 — Minimal usable terminal 🟨

- ✅ Spawn the shell in a PTY, read on its own thread (`alacritty_terminal` event loop; wakeups coalesced).
- ✅ Grid rendering (text + colors + attributes + cursor) with damage tracking (per-row text buffers and quads, only dirty rows reshaped).
- ✅ Keyboard: modifiers, special keys, F1–F12, app cursor mode, AltGr on Windows, injected Unicode. Bracketed paste.
- ✅ Resize, scrollback (wheel, `Shift+PgUp/PgDn`), alternate scroll in full-screen apps, selection (click/drag, double = word, triple = line, Alt = block), copy/paste (`Ctrl+Shift+C/V`, `Ctrl+C` with selection, right click on Windows), zoom (`Ctrl+=/-/0`).
- ✅ Measure performance budgets (see below).
- ✅ Mouse reporting to applications (normal + SGR 1006: clicks, drag/motion, wheel; Shift bypasses it for selection) and focus reporting.
- ⬜ Search in buffer (`Ctrl+Shift+F`), URL detection and OSC 8 hyperlinks.
- ✅ Wide glyphs: text after a wide glyph (emoji, CJK) is anchored to the right column (fixed in M5).
- ⬜ Emoji with variation selectors (`🗂️` is measured 1 cell, drawn 2) and combining characters (dropped today).
- ✅ Shell exit: shows the exit code; Enter restarts the shell, Esc closes the window.
- ✅ Recover from GPU surface loss by recreating the surface (implemented; not exercised yet — hard to trigger on demand).
- ⬜ Verify on Linux (X11 + Wayland).

**M1 measurements** (release, Windows 11, Intel Iris Xe, PowerShell 5.1, 1393×922 window):

| Metric | Result | Budget |
|---|---|---|
| Startup to first frame | ~240 ms (window 21 · GPU/DX12 112 · text pipeline 15–36 · shell spawn 21 · first frame 43) | < 200 ms ❌ |
| Idle CPU (15 s, after shell finished loading) | 0% (no thread ran) | ~0% ✅ |
| `type` of a 19 MB file | 16.1 s — same as native conhost (16.3 s); ConPTY's conhost used 7.8 s CPU vs 4.3 s for Mao-Term | ≈ Alacritty ⚠️ bound by ConPTY |
| Private memory after 10k lines of scrollback filled | ~118 MB | < 50 MB over baseline ✅ |

Next steps for startup: create the GPU device in parallel with window creation and shell spawn; cache compiled pipelines. Next step for throughput: evaluate shipping the newer `conpty.dll`/`OpenConsole.exe` from Windows Terminal (as WezTerm does).

## M2 — Multi-terminal 🟨

- ✅ Tabs: create (`Ctrl+Shift+T`, `+`), close (`×`, middle click, `Ctrl+Shift+W` on the last pane), switch (click, `Ctrl+Tab`, `Ctrl+Shift+Tab`, `Alt+1…9`), activity dot for background output.
- ✅ Splits right/down (`Ctrl+Shift+D/E`), drag dividers to resize (PTYs follow), focus with `Alt+Arrows` or click, maximize pane (`Ctrl+Shift+Enter`).
- ✅ Command palette (`Ctrl+Shift+P`) with fuzzy search over every action, keyboard and mouse.
- ✅ Basic status bar: focused pane title, tab/pane position, grid size.
- ⬜ Rename tabs, reorder tabs by dragging.
- ✅ New tab/pane inherits the cwd (via shell integration, M3).
- ⬜ Restore tabs/splits on restart (optional).

## M3 — Shell integration + autocomplete 🟨

- ✅ OSC 133/7 scanner in front of the parser (`ScanningPty`); ConPTY passes the sequences through (end-to-end test).
- ✅ Automatic injection without touching dotfiles. PowerShell 5.1/7 validated on Windows; bash/zsh/fish scripts written but **untested** (need Linux).
- ✅ Native prompt editor drawn over the grid where the prompt ends: word/selection editing, undo, emacs keys, syntax highlighting, history Up/Down filtered by prefix.
- ✅ Ghost text from history (cwd-aware), accept with →/End (Ctrl+→ one word); imports PSReadLine/bash/zsh history.
- ✅ Tab completion menu: commands (specs, history, `PATH`), subcommands and options (git, cargo, npm, docker, winget, cd…), paths (quoted per shell), git branches read from `.git`.
- ✅ Status bar shows cwd and the last non-zero exit code; tabs are named after their folder.
- ✅ Grid text pinned to cells (fixes a drift of ~0.4 px/column present since M1; cosmic-text's `monospace_width` snapping is broken).
- ⬜ Multi-line input (Shift+Enter), `Ctrl+R` history search, click to move the caret.
- ⬜ Completion on a worker thread (today: synchronous local reads; network paths are skipped).
- ⬜ Shell aliases/functions as completion candidates; importer for Fig specs.
- ⬜ Verify bash/zsh/fish on Linux.

## M4 — Sidebar + file tree 🟨

- ✅ Activity bar (Files, Agents, Monitor, Settings; Segoe Fluent/MDL2 icons with plain fallbacks) and collapsible, resizable sidebar (`Ctrl+Shift+B`, `Ctrl+Shift+1/2/3`).
- ✅ `mao-fs`: listing on a worker thread, `.gitignore`-aware (toggle from the palette), natural sort, watchers only on visible folders with debounced re-listing. Closed sidebar = no threads, no watchers (measured: idle CPU 0%, +2 threads only while open).
- ✅ Tree follows the focused pane's cwd; lazy expansion, keyboard navigation (arrows, Enter, `Ctrl+Enter` = cd/insert path, type-ahead), mouse wheel, scrollbar.
- ✅ Actions: click previews a file in the document viewer, double click opens it (folders: `cd`), drag onto a terminal inserts the path (relative to the shell's folder when possible), context menu (open, open with default app, cd here, open in new tab, insert/copy path, copy relative path, reveal in file explorer).
- ✅ Settings button / `Ctrl+,` opens the config file (created from a commented template).
- ✅ UI top layer so menus and popups cover the text below them.
- ⬜ Per-file git status, file-type icons, "pin root" / repo-root option, filter-as-you-type (today: type-ahead jump), persist sidebar width.
- ✅ Open files in the document viewer (M5).

## M5 — Document viewer 🟨

- ✅ `mao-docs`: background loader + watcher (auto-reload, log tail), type detection, binary detection, UTF-16, 64 MB cap.
- ✅ Document pane in the split tree (reused per tab), header with kind/size/lines and Source/Wrap/Fit/Open buttons, title and status bar info.
- ✅ Markdown rendered on the cell grid (headings, emphasis, lists, task lists, quotes, box-drawn tables, highlighted code blocks) + source view ([ADR 0004](decisions/0004-document-viewer.md)).
- ✅ Code/text/JSON/TOML/YAML/logs highlighted with syntect + bat grammars; line numbers; wrap toggle; plain text first, highlighting after (first screen ~2 ms; 10 MB log in 128 ms).
- ✅ Images (png/jpg/gif/webp/bmp/ico) and SVG (resvg) on a new mipmapped textured-quad pipeline: fit, zoom, pan.
- ✅ Search (`Ctrl+F`, all matches highlighted), mouse selection, copy, keyboard scrolling.
- ✅ `mao open <file>` in pwsh/bash/zsh/fish via OSC 7770 (ConPTY passes it through); opens from the file tree (click = preview).
- ✅ Idle CPU 0% with documents open; grammars (~50 MB) freed when the last document closes.
- ⬜ Palette `Open File…` / quick open, Ctrl+click on paths in terminal output.
- ⬜ Huge files (> 64 MB) by chunks/mmap; lazy highlighting of the visible part only.
- ⬜ Inline images in Markdown, clickable links, JSON folding, animated GIFs.

## M6 — Agents + monitor ⬜

- ⬜ Agents panel: process detection (Claude Code, opencode, Codex, Gemini CLI, Aider…), jump to pane, open its cwd.
- ⬜ Monitor panel: CPU, RAM, disk, network, top processes.
- ⬜ Status bar indicators.

## M7 — Polish and distribution ⬜

- ⬜ Bundled themes + settings editor.
- ⬜ Packages: MSI/winget (Windows), AppImage/.deb (Linux).
- ⬜ User documentation.

## Future (unscheduled)

- Command blocks (output grouped per command, Warp-style).
- AI suggestions (natural language → command, explain errors).
- PDF viewer, light document editing.
- Detailed agent session status (reading their logs/transcripts).
- GPU in the system monitor.
- macOS.
- Plugins.
