# Design and architecture

> Living document. Important decisions are justified in [decisions/](decisions/).

## Stack

| Layer | Choice | Reason |
|---|---|---|
| Window / events | `winit` | Rust standard, Windows + Linux (X11 and Wayland). |
| GPU | `wgpu` | DX12/Vulkan on Windows, Vulkan/GL on Linux. |
| Text | `glyphon` (on top of `cosmic-text`) | Shaping, font fallback, GPU glyph atlas. |
| VT emulation | `alacritty_terminal` | Parser + grid + scrollback, battle-tested. |
| PTY | `alacritty_terminal::tty` (ConPTY / Unix PTY) | Same code as Alacritty; fallback: `portable-pty`. |
| UI layout | `taffy` | Flexbox for the chrome (sidebar, bars, panels). |
| Config | `serde` + `toml`, `directories`, `notify` | TOML config with hot reload. |
| System | `sysinfo` | Processes, CPU, RAM, disks, network on Windows and Linux. |
| Files | `ignore`, `notify` | `.gitignore`-aware walking, watchers. |
| Documents | `pulldown-cmark`, `syntect`, `image`, `resvg` | Markdown, highlighting, images, SVG. |
| Logging | `tracing` | Logs + spans for profiling. |

See [ADR 0001](decisions/0001-render-stack.md).

## Crate workspace

```
crates/
  mao-app/        bin: winit event loop, wires everything, keybindings, actions
  mao-render/     wgpu: pipelines (instanced quads, glyphon text, images), atlas
  mao-ui/         layout tree (taffy), widgets, hit-testing, focus → draw list
  mao-terminal/   terminal session: PTY + alacritty Term, shell integration (OSC)
  mao-input/      native line editor (buffer, cursor, selection, history)
  mao-complete/   autocomplete engine: shell tokenizer + suggestion sources
  mao-fs/         lazy file tree + watcher
  mao-docs/       document loading/parsing for the viewer
  mao-agents/     AI agent session detection
  mao-sysmon/     system metrics sampling
  mao-config/     config, themes, keybindings, per-OS paths
shell-integration/
  mao.ps1  mao.bash  mao.zsh  mao.fish
```

Dependency rules:

- `mao-app` depends on everything; **no crate depends on `mao-app`**.
- `mao-render` knows nothing about terminals or widgets: it receives a **draw list** (`DrawList`: rects, text, images).
- `mao-ui` produces a `DrawList`; it does not touch wgpu.
- Domain crates (`terminal`, `complete`, `fs`, `docs`, `agents`, `sysmon`) don't depend on `render` or `ui` → testable without a GPU.

## Window structure

```
┌──┬────────────┬─────────────────────────────────────────────┐
│A │            │ [tab 1] [tab 2] [+]                          │
│c │  Sidebar   ├──────────────────────┬──────────────────────┤
│t │  (Files /  │                      │                      │
│i │   Agents / │   Pane: Terminal     │   Pane: Terminal     │
│v │   Monitor) │                      │   or Document        │
│i │            │                      │                      │
│t │            │  ┌ input editor ───┐ │                      │
│y │            │  └ suggestions ────┘ │                      │
├──┴────────────┴──────────────────────┴──────────────────────┤
│ status: cwd · git branch · CPU 12% · RAM 8.1/32 GB · agents: 2 │
└─────────────────────────────────────────────────────────────┘
```

- **Activity bar** (left): Files, Agents, Monitor, Settings buttons. These are the buttons that open each panel.
- **Sidebar**: shows the active panel; collapsible (`Ctrl+Shift+B`).
- **Main area**: tabs; each tab is a split tree; each leaf is a *pane* (`Terminal` or `Document`).
- **Status bar**: compact info, cheap to compute.

## Threading model

| Thread | Responsibility |
|---|---|
| Main (UI) | winit event loop, input, layout, render. **Never blocks.** |
| PTY reader (1 per session) | Reads bytes from the PTY, feeds the parser under `FairMutex<Term>`, marks dirty and wakes the UI via `EventLoopProxy`. |
| PTY writer | Writes to the PTY (input, large pastes) without blocking the UI. |
| Completion worker | Slow suggestion sources (fs, git). Results tagged with a request id; stale ones are dropped. |
| FS watcher | `notify` + lazy tree walking. |
| Sysmon sampler | Only active if the monitor or status bar needs it. |
| Agents scanner | Periodic process scan only while the panel is open (or the status bar counter is enabled). |

Communication: channels (`crossbeam-channel`) to the UI + `EventLoopProxy::send_event(UserEvent::…)` to wake it.
No `tokio` in v1: there is no networking; dedicated threads are simpler and more predictable.

## Render loop

1. Something changes (PTY bytes, input, resize, panel event) → mark `dirty` → `window.request_redraw()`.
2. On `RedrawRequested`: layout (if changed) → build `DrawList` → reuse text buffers of unchanged rows (damage tracking from `alacritty_terminal`) → submit.
3. No changes → no frames. The blinking cursor uses a timer (`ControlFlow::WaitUntil`), not a loop.
4. Coalescing: under heavy PTY output, at most one frame per vsync.

## Performance budgets

Measured in release, on an average laptop (4+ cores, integrated GPU).

| Metric | Target |
|---|---|
| Startup to first frame with prompt | < 200 ms |
| Key → pixel latency (excluding vsync) | < 10 ms |
| Idle CPU (1 tab, blinking cursor) | ~0% (< 0.5%) |
| RAM with 1 tab, 10k lines of scrollback | < 120 MB |
| `cat` of 100 MB of text | Similar to Alacritty (±25%) |
| Autocomplete suggestion (local sources) | < 5 ms per keystroke |
| Sidebar/agents/monitor closed | zero cost (no threads or timers) |

See the [`performance`](skills/performance/SKILL.md) skill for how to measure.

## Platforms

| | Windows | Linux |
|---|---|---|
| PTY | ConPTY (Windows 10 1809+) | `openpty` |
| Default shell | `pwsh` → `powershell` → `cmd` | `$SHELL` → `/bin/bash` |
| GPU backend | DX12 (Vulkan fallback) | Vulkan (GL fallback) |
| Windowing | Win32 | X11 and Wayland |
| Config | `%APPDATA%\mao-term\` | `$XDG_CONFIG_HOME/mao-term/` |

See the [`cross-platform`](skills/cross-platform/SKILL.md) skill.

## Errors and logging

- Libraries: `thiserror`. Binary: `anyhow` at the edges.
- A failing panel (agents, monitor…) shows the error inside the panel; **it never takes the terminal down**.
- Logs via `tracing` to a file in the data directory; level via `MAO_LOG=debug`.
