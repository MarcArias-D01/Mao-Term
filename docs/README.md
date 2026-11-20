# Mao-Term documentation

| Document | Contents |
|---|---|
| [vision.md](vision.md) | What we build, for whom, what it is NOT. |
| [design.md](design.md) | Architecture, crates, threading model, performance budgets. |
| [roadmap.md](roadmap.md) | Milestones and status. |
| [decisions/](decisions/) | ADRs (Architecture Decision Records). |
| [specs/](specs/) | One spec per feature. |
| [skills/](skills/) | Skills for coding agents. |

## Specs

| Spec | Milestone | Status |
|---|---|---|
| [terminal-core](specs/terminal-core.md) | M1 | In progress |
| [renderer](specs/renderer.md) | M0–M1 | In progress |
| [workspace-layout](specs/workspace-layout.md) | M2 | In progress |
| [shell-integration](specs/shell-integration.md) | M3 | In progress |
| [input-autocomplete](specs/input-autocomplete.md) | M3 | In progress |
| [file-tree](specs/file-tree.md) | M4 | In progress |
| [document-viewer](specs/document-viewer.md) | M5 | In progress |
| [agents-panel](specs/agents-panel.md) | M6 | In progress |
| [system-monitor](specs/system-monitor.md) | M6 | In progress |
| [config-theming](specs/config-theming.md) | M0–M7 | Draft |

## Decisions

| ADR | Title | Status |
|---|---|---|
| [0001](decisions/0001-render-stack.md) | Stack: winit + wgpu + glyphon + alacritty_terminal | Accepted |
| [0002](decisions/0002-native-input-editor.md) | Native input editor for autocomplete | Accepted |
| [0003](decisions/0003-portable-agent-docs.md) | AGENTS.md and skills in docs/, portable across agents | Accepted |
| [0004](decisions/0004-document-viewer.md) | Document viewer: cell grid rendering, syntect, `mao open` over OSC | Accepted |

Spec statuses: `Draft` → `Approved` → `In progress` → `Done`.
