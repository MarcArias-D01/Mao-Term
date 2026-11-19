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
- ⬜ Color emoji (renders blank today) and combining characters (dropped today).
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
- ⬜ New tab/pane inherits the cwd (needs shell integration, M3).
- ⬜ Restore tabs/splits on restart (optional).

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
