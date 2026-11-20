# ADR 0005 — Warp-style input box at the bottom of the pane

- **Status:** Accepted
- **Date:** 2026-11-20

## Context

ADR 0002 put a native editor where the shell's prompt ends, drawn over the grid. Users coming from Warp expect the opposite layout: command output above, a fixed input box at the bottom of the pane.

## Decision

With `[input] position = "bottom"` (the default), each terminal pane with shell integration reserves a box at its bottom:

- The PTY is sized to the area above the box; the box shows a context line (folder with `~`, git branch read from `.git/HEAD`, last non-zero exit code or the running command) and the native editor, rendered as a small grid of its own (syntax colors, ghost text, caret). Long lines grow the box upward over the output (the output's text is clipped, the PTY is not resized while typing).
- The shell's live prompt row(s) — the cursor row and the rows that soft-wrapped into it — are blanked in the output while the editor is active; previous prompts and their commands stay in the output as history.
- Short output is aligned to the bottom, right above the box.
- The box disappears (and the PTY grows back) when there is no shell integration, the shell exited, or a full-screen program uses the alternate screen.
- The completion menu opens above the box.

`position = "inline"` keeps the ADR 0002 overlay.

## Consequences

- ✅ Warp-like layout without changing the shells: output is still the real terminal stream, so `ls`, colors, progress bars and scrollback behave as before.
- ✅ No PTY resizes while typing.
- ❌ One PTY resize when the box appears/disappears (first prompt, full-screen apps).
- ❌ Multi-line prompts whose first line ends with a hard newline (e.g. some oh-my-posh themes) keep that first line visible in the output.
- ❌ Not yet "command blocks" (per-command grouping, collapse, copy output): a later step that can build on the prompt marks (OSC 133).
