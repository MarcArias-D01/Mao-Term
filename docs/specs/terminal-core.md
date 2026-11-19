# Spec: Terminal core

- **Status:** Draft
- **Milestone:** M1
- **Crates:** `mao-terminal`, `mao-app`
- **Related:** [renderer](renderer.md), [shell-integration](shell-integration.md), [ADR 0001](../decisions/0001-render-stack.md)

## Goal

A correct and fast terminal session: spawn a shell in a PTY, interpret its output (VT/ANSI) and send it keyboard and mouse input. Everything else builds on top of this.

## Scope

- Spawn the configured shell (or the OS default) with initial cwd, env and size.
- Read the PTY on its own thread and feed `alacritty_terminal::Term`.
- Encode keyboard → bytes (modifiers, function keys, app cursor/keypad modes).
- Mouse: report to the program (SGR 1006) when requested; otherwise local selection.
- Bracketed paste. Large pastes without blocking the UI.
- Resize (rows/columns and pixels) → PTY + Term.
- Configurable scrollback (default 10,000 lines).
- Selection (simple, word on double-click, line on triple-click, block with Alt) and copy/paste.
- OSC 8 hyperlinks and URL detection (Ctrl+click to open).
- OSC 52 (copy to clipboard) — can be disabled.
- Window/tab title (OSC 0/2).
- Session end: show exit code and offer close or restart.

## Out of scope

- Command blocks (Future).
- In-terminal image protocols (Sixel, Kitty) — Future.
- Input editor → [input-autocomplete](input-autocomplete.md).

## Behavior

| Action | Default shortcut |
|---|---|
| Copy | `Ctrl+Shift+C` (and copy-on-select, optional) |
| Paste | `Ctrl+Shift+V`, right click (Windows) |
| Scroll | wheel, `Shift+PgUp/PgDn` |
| Search buffer | `Ctrl+Shift+F` |
| Font zoom | `Ctrl+=` / `Ctrl+-` / `Ctrl+0` |

## Technical design

```rust
pub struct TerminalSession {
    id: SessionId,
    term: Arc<FairMutex<Term<EventProxy>>>,
    pty_tx: Sender<PtyMsg>,        // input / resize / shutdown to the I/O thread
    child_pid: u32,                // for agents-panel and cwd on Linux
    shell_state: ShellState,       // from shell-integration: prompt / running / unknown
    cwd: Option<PathBuf>,          // from OSC 7
}
```

- One I/O thread per session: reads into 64 KiB buffers, parses under the lock, releases the lock, sends `UserEvent::Wakeup(id)` (coalesced: no new one until the UI consumes it).
- The `Term` lock is never held during I/O or GPU submit: the UI copies what it needs (damaged rows) and releases.
- `Term` events (title, bell, clipboard, exit) → `UserEvent`.
- Default shell: see the platforms table in `design.md`.

## Performance

- Throughput similar to Alacritty on `cat` of 100 MB.
- No output → the I/O thread is blocked on read (0% CPU).
- The UI never waits on the lock for more than ~1 ms (parser processes bounded chunks).

## Acceptance criteria

- [ ] pwsh, cmd (Windows) and bash, zsh, fish (Linux) start and work.
- [ ] vim/nvim, htop/btop, less, tmux, ssh work without artifacts.
- [ ] Basic `vttest` (cursor, erase, scroll regions) correct.
- [ ] Resizing with a fullscreen program does not corrupt output.
- [ ] Pasting 1 MB of text does not freeze the window.
- [ ] Closing the tab kills the child process (and its tree on Windows).

## Open questions

- `alacritty_terminal::tty` or `portable-pty`? → M0 spike.
- Copy-on-select enabled by default on Linux?
