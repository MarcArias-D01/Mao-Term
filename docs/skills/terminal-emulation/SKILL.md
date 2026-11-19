---
name: terminal-emulation
description: Practical knowledge for working with PTY/ConPTY, VT/ANSI/OSC sequences, keyboard/mouse encoding and shell integration in Mao-Term, and for debugging emulation bugs.
---

# Terminal emulation

## Pieces

- **PTY**: `alacritty_terminal::tty` (ConPTY on Windows, `openpty` on Unix). Encapsulated in `mao-terminal`.
- **Parser + grid**: `alacritty_terminal::Term`. Don't reimplement what it already does.
- **Ours**: keyboard/mouse encoding, custom OSCs (133, 7, 633), shell integration, UI integration.

## Sequences we care about

| Sequence | Use |
|---|---|
| `OSC 0/2` | Title |
| `OSC 7` | cwd (`file://host/path`, URL-encoded) |
| `OSC 8` | Hyperlinks |
| `OSC 52` | Clipboard (can be disabled for security) |
| `OSC 133 A/B/C/D` | Prompt / input / execution / end + exit code |
| `OSC 633;E` | Command line (VS Code format) |
| `CSI ?2004h` | Bracketed paste |
| `CSI ?1000/1002/1003/1006h` | Mouse reporting (SGR) |
| `CSI ?1h` / `ESC =` | App cursor / keypad mode |

## Keyboard

- Honor app cursor mode (`ESC O A` vs `ESC [ A`).
- Modifiers on special keys: `CSI 1;<mod> A` (mod = 1 + Shift·1 + Alt·2 + Ctrl·4).
- `Alt+key` → `ESC` prefix.
- Kitty keyboard protocol (`CSI >1u`) is a future improvement, not v1.
- Mao-Term shortcuts are resolved **before** sending to the PTY; if no shortcut matches, the key goes to the PTY (or the input editor in editor mode).

## ConPTY quirks (Windows)

- ConPTY **rewrites** output: it may reorder/re-emit sequences and repaint the screen on resize. Don't assume incoming bytes are what the program emitted.
- It passes unknown OSCs (133, 7) through on recent Windows versions; verify on the minimum supported version.
- Killing: close the pseudoconsole + terminate the process tree (Job Object).
- Pass the initial size when creating the ConPTY to avoid a wrong first repaint.

## Debugging

0. **Drive the shell without touching other windows**: `MAO_DEBUG_INPUT='dir\n' cargo run -p mao-app` types into Mao-Term's own PTY at startup (a literal `\n` = Enter). `MAO_DEBUG_ACTIONS='Split Pane Right|New Tab'` runs palette actions by name at startup. `MAO_DEBUG_BACKGROUND=1` opens the window without taking keyboard focus — on Windows also launch it with `CreateProcess` + `STARTF_USESHOWWINDOW`/`SW_SHOWNOACTIVATE` (`Start-Process` forces activation), otherwise whatever the user is typing elsewhere goes into the test window. `MAO_LOG_FILE=<path>` writes logs to a file. To look at the result, capture *that* window (e.g. Win32 `PrintWindow`), not the screen; to send keys or clicks, `PostMessage` to *that* window. Never use OS-level key injection (e.g. `SendKeys`): if focus is not obtained, keystrokes land in whatever window the user has focused.
1. **Record raw bytes** *(planned)*: with `MAO_PTY_LOG=<file>` the I/O thread dumps everything read (debug feature). Reproduce by feeding the `Term` in a test.
2. Compare with another terminal: Windows Terminal (Windows), Alacritty/WezTerm (Linux).
3. Test with: `vttest`, `vim`, `htop`/`btop`, `tmux`, `less`, `fzf`, a mouse-enabled TUI, and `cat` of a file with wide Unicode and emoji.
4. To inspect sequences: `showkey -a` (Linux) or a script that prints received bytes.

## Checklist when touching emulation

- [ ] Works on ConPTY **and** Unix PTY.
- [ ] A recorded-bytes test reproduces the case.
- [ ] No throughput regression (`performance` skill).
