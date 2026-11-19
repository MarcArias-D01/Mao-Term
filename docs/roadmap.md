# Roadmap

Status: ⬜ pending · 🟨 in progress · ✅ done

## M0 — Foundations ⬜

- ⬜ Cargo workspace with the crates from `design.md` (empty), `rust-toolchain.toml`, edition 2024.
- ⬜ CI (GitHub Actions): fmt, clippy, test on `windows-latest` and `ubuntu-latest`. `cargo-deny` for licenses.
- ⬜ winit + wgpu window that clears the screen with the theme background color.
- ⬜ Minimal config loading (font, size, colors).
- ⬜ Spike: validate `alacritty_terminal::tty` with ConPTY on Windows (see ADR 0001).

## M1 — Minimal usable terminal ⬜

- ⬜ Spawn the shell in a PTY, read on its own thread.
- ⬜ Grid rendering (text + colors + attributes + cursor) with damage tracking.
- ⬜ Full keyboard (modifiers, special keys, app cursor mode), bracketed paste.
- ⬜ Resize, scrollback with the wheel, mouse selection, copy/paste.
- ⬜ Measure performance budgets (startup, latency, throughput, idle).

## M2 — Multi-terminal ⬜

- ⬜ Tabs (create, close, move, rename).
- ⬜ Horizontal/vertical splits, resize, keyboard focus movement.
- ⬜ Command palette (`Ctrl+Shift+P`).
- ⬜ Basic status bar (cwd).

## M3 — Shell integration + autocomplete ⬜

- ⬜ Integration scripts for pwsh, bash, zsh, fish (OSC 133, OSC 7).
- ⬜ Native input editor while the shell is at the prompt (ADR 0002).
- ⬜ Autocomplete engine: history, paths, command specs (git, cargo, npm, docker…).
- ⬜ Inline suggestion (ghost text) + dropdown menu.

## M4 — Sidebar + file tree ⬜

- ⬜ Activity bar + collapsible sidebar.
- ⬜ File tree that follows the cwd of the focused pane.
- ⬜ Actions: open in viewer, `cd`, copy path, insert path into the input.

## M5 — Document viewer ⬜

- ⬜ Document pane type.
- ⬜ Rendered Markdown, highlighted code, images, SVG, JSON/TOML/YAML.

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
