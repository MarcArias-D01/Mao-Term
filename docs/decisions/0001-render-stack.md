# ADR 0001 — Stack: winit + wgpu + glyphon + alacritty_terminal

- **Status:** Accepted
- **Date:** 2026-11-19

## Context

We want a Warp-style terminal with a rich UI (sidebar, panels, document viewer) and top-tier performance on Windows and Linux.

## Options considered

1. **Custom wgpu + glyphon** — our own GPU renderer, our own UI on top.
2. **GPUI (Zed)** — high-level GPU framework. Very fast, but unstable API, little documentation and young Windows support.
3. **iced / egui** — mature, easy frameworks. Less control over grid rendering; egui is immediate-mode and over-redraws.

For emulation: write our own VT parser vs. reuse `alacritty_terminal`.

## Decision

- Window: `winit`. GPU: `wgpu`. Text: `glyphon`/`cosmic-text`. Chrome layout: `taffy`.
- Emulation: `alacritty_terminal` (parser, grid, scrollback, damage tracking).
- PTY: `alacritty_terminal::tty`; if the M0 spike shows ConPTY issues, `portable-pty`.

## Consequences

- ✅ Full control over rendering → we can meet the performance budgets.
- ✅ VT emulation, the hardest part, comes battle-tested.
- ❌ We build our own widgets (tabs, tree, lists, scrolling, menus). Kept minimal in `mao-ui`.
- ❌ We depend on `alacritty_terminal`'s API, which promises no stability: pin the version and encapsulate it fully in `mao-terminal`.
