# ADR 0002 — Native input editor for autocomplete

- **Status:** Accepted (validated in M3 with PowerShell)
- **Date:** 2026-11-19

## Context

We want autocomplete as you type. In a classic terminal, what the user types goes straight to the shell, and the shell's line editor (PSReadLine, readline, zle) owns it: the terminal **does not know** what is on the line.

## Options

1. **Native editor (Warp-style).** When the shell is waiting at the prompt (known via OSC 133), input is captured by our own editor. On Enter we send the full line to the PTY.
2. **Overlay on top of the shell's editor (Fig-style).** Each shell reports its current buffer via hooks; we paint suggestions on top. Requires fragile per-shell integration, and the shell's cursor/render fights ours.
3. **Only the shell's native completion.** Doesn't meet the goal.

## Decision

Option 1, with two modes per pane:

- **Editor mode**: the shell is at the prompt (`OSC 133;B` received and no command running). Input is handled by `mao-input`, with suggestions from `mao-complete`.
- **Passthrough mode**: a command is running (`OSC 133;C`) or the shell has no integration. Keys go raw to the PTY → vim, ssh, REPLs, htop work as always.

Without shell integration, Mao-Term behaves as a classic terminal (always passthrough).

## Consequences

- ✅ Same autocomplete and UX across pwsh, bash, zsh and fish.
- ✅ Instant suggestions with no round-trips to the shell.
- ❌ We must reimplement line-editor basics (word movement, history, selection, multiline).
- ❌ Shell-specific features (zle widgets, custom PSReadLine bindings) don't apply in editor mode. The editor can be disabled per profile.
- ✅ The shell echoes the submitted command: in practice this looks exactly like a normal shell (the editor overlay disappears and the shell prints the command where it was typed).
- ⚠️ PSReadLine's own features (predictions, custom key handlers) don't apply in editor mode.
