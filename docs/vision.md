# Vision

## In one sentence

A terminal for Windows and Linux that feels as modern as Warp but is **fast, light and stays out of your way**, and that also gives you context about your machine: files, documents, AI agents and system performance.

## Who it is for

Developers who live in the terminal, increasingly alongside AI coding agents (Claude Code, opencode, Codex…), and who want to:

- type commands faster (autocomplete),
- keep several terminals at hand without fighting windows,
- see where they are and what is around (file tree, documents),
- know which agents are running and how their machine is doing.

## v1 features

| # | Feature | Spec |
|---|---|---|
| 1 | High-performance terminal (GPU, full VT, scrollback, selection, copy/paste) | [terminal-core](specs/terminal-core.md), [renderer](specs/renderer.md) |
| 2 | Autocomplete as you type (inline + menu) | [input-autocomplete](specs/input-autocomplete.md), [shell-integration](specs/shell-integration.md) |
| 3 | Multiple terminals: tabs + splits + command palette | [workspace-layout](specs/workspace-layout.md) |
| 4 | Sidebar with a file tree of the current directory | [file-tree](specs/file-tree.md) |
| 5 | Open and view documents | [document-viewer](specs/document-viewer.md) |
| 6 | Button/panel with the agent sessions running on the machine | [agents-panel](specs/agents-panel.md) |
| 7 | Button/panel with system performance and health | [system-monitor](specs/system-monitor.md) |
| 8 | Configuration, themes and keybindings | [config-theming](specs/config-theming.md) |

## Principles

1. **Performance is a feature.** If something makes the terminal slower, it doesn't ship (or ships off by default).
2. **A terminal is still a terminal.** vim, htop, ssh, REPLs… all work exactly as in Alacritty/Windows Terminal.
3. **Optional panels.** Sidebar, agents and monitor cost nothing when closed.
4. **Keyboard first**, mouse welcome.
5. **Local and private.** Nothing leaves the machine unless the user explicitly enables it.

## What it is NOT (for now)

- Not an IDE or text editor (the document viewer is read-only in v1).
- No built-in generative AI in v1 (see roadmap "Future").
- No account, login or telemetry.
- No macOS support in v1.
