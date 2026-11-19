# Spec: Shell integration

- **Status:** Draft
- **Milestone:** M3
- **Crates:** `mao-terminal`; scripts in `shell-integration/`
- **Related:** [input-autocomplete](input-autocomplete.md), [file-tree](file-tree.md), [ADR 0002](../decisions/0002-native-input-editor.md)

## Goal

Let the terminal know **where the prompt is, when a command is running, its exit code, and the cwd**. Needed by the input editor, autocomplete, the file tree and cwd inheritance for new panes.

## Scope

Sequences emitted by our scripts (compatible with the de facto standard used by VS Code, WezTerm, iTerm2):

| Sequence | Meaning |
|---|---|
| `OSC 133;A ST` | Prompt starts |
| `OSC 133;B ST` | Prompt ends / input starts |
| `OSC 133;C ST` | Command execution starts |
| `OSC 133;D;<exit> ST` | Command finished with exit code |
| `OSC 7;file://<host>/<path> ST` | Current directory |
| `OSC 633;E;<cmd> ST` (or our own `OSC 1337`) | Executed command line (history) |

- Scripts: `mao.ps1` (pwsh and Windows PowerShell), `mao.bash`, `mao.zsh`, `mao.fish`.
- **Automatic injection** when spawning the shell (without touching user dotfiles): env var + startup args (`-NoExit -Command . mao.ps1`, `--rcfile` for bash, `ZDOTDIR` for zsh, `--init-command` for fish). Can be disabled.
- Documented manual install for those who prefer it (and for ssh).
- Parsing of these sequences in `mao-terminal` → per-session `ShellState` and `cwd`.

## Out of scope

- Automatic integration for remote ssh sessions (Future).
- cmd.exe (no integration → passthrough mode).

## Technical design

```rust
enum ShellState { Unknown, Prompt, Input, Running { started: Instant }, Finished { exit: i32 } }
```

- `alacritty_terminal` doesn't directly expose unknown OSCs → intercept on the I/O thread (light pre-scan of the stream or wrapping the parser's `Perform`). Decide in M3 and document.
- cwd fallback without integration: Linux `/proc/<pid>/cwd` of the foreground process; Windows has no reliable method → no cwd.

## Performance

- The OSC pre-scan must not reduce throughput by more than 3%.
- Shell hooks must add < 5 ms to the prompt.

## Acceptance criteria

- [ ] In pwsh, bash, zsh and fish: correct Prompt/Input/Running/Finished states, including Ctrl+C.
- [ ] cwd updates after `cd`, `pushd`, `z`/`zoxide`.
- [ ] Paths with spaces, Unicode and Windows drives (`C:\…`) work (URL-encoding in OSC 7).
- [ ] Custom prompts (oh-my-posh, starship) keep working.
- [ ] Disabling integration leaves a classic terminal with no errors.

## Open questions

- Emit the command line with `OSC 633;E` (VS Code) or our own sequence?
- How to avoid the duplicated echo of the command sent by the native editor? (see ADR 0002)
