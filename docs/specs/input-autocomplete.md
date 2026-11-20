# Spec: Input editor and autocomplete

- **Status:** In progress
- **Milestone:** M3
- **Crates:** `mao-input`, `mao-complete`, `mao-app`
- **Related:** [shell-integration](shell-integration.md), [ADR 0002](../decisions/0002-native-input-editor.md)

## Goal

Make typing commands fast: as you type, the terminal suggests how to complete it (command, subcommand, flag, path, git branch, history entry).

## Scope

**Input editor** (`mao-input`), active in editor mode (see ADR 0002):

- Free cursor, word movement (`Ctrl+←/→`), home/end, selection with Shift and mouse.
- Undo/redo, copy/cut/paste.
- Multiline (`Shift+Enter`), with per-shell line continuation.
- History (`↑/↓`) filtered by prefix; incremental search (`Ctrl+R`).
- Basic syntax highlighting (command, flags, strings, valid/invalid paths).

**Autocomplete** (`mao-complete`):

- **Ghost text**: the best suggestion appears greyed out after the cursor. `→` or `End` accepts it all; `Ctrl+→` accepts one word.
- **Menu**: appears automatically when starting a token after a space (configurable) or with `Tab`/`Ctrl+Space`. Navigate `↑/↓`, accept `Tab`/`Enter`, dismiss `Esc`. Each option has a description.
- **Sources**:
  1. History with *frecency*, prioritizing the current cwd.
  2. File system paths (relative to the pane's cwd).
  3. Command specs: subcommands, flags and arguments for `git`, `cargo`, `npm`/`pnpm`, `docker`, `kubectl`, `winget`, `apt`… defined in data files.
  4. Dynamic: git branches, `package.json` scripts, `Cargo.toml` targets, environment variables.
  5. `PATH` executables and shell aliases/functions (sent by the integration).

## Input position (ADR 0005)

- `[input] position = "bottom"` (default): Warp-style box at the bottom of each pane with a context line (folder, git branch, last exit code / running command), output above (bottom-aligned when short), live prompt hidden from the output, completion menu above the box; hidden for full-screen apps. `"inline"`: the editor over the prompt (ADR 0002).

## Out of scope

- AI suggestions (Future).
- Completion inside interactive programs (passthrough mode → whatever the program does).

## Technical design

- **Shell-aware tokenizer**: quotes, escapes, `|`, `&&`, `;`, `$(…)`, pwsh vs POSIX differences. Determines the command and the token under the cursor.
- **Engine**: `complete(ctx) -> Vec<Suggestion>` where `ctx` = {line, cursor, cwd, shell, env}. Synchronous sources (history, specs, cached PATH) answer on the UI thread within budget; slow ones (large fs, git) go to the *completion worker* and arrive later with a request id (stale ones dropped).
- **Ranking**: prefix match > fuzzy; boost by frequency, recency and cwd.
- **Command specs** in data files (TOML/JSON) under `crates/mao-complete/specs/`, lazily loaded per command. Simple custom format; consider an importer for the `withfig/autocomplete` format (MIT license).
- **History**: our own, per session/global, in the data directory (we don't depend on the shell's history file, but import it at startup).

## Performance

- Ghost text updated < 5 ms after the keystroke (local sources).
- File system walking never blocks the UI; huge directories capped.
- Spec memory: only for commands actually used.

## Acceptance criteria

- [ ] `git ch` → suggests `checkout`; `git checkout ` → shows branches.
- [ ] `cd ` → shows only directories in the cwd; paths with spaces are quoted correctly per shell.
- [ ] `cargo ` → subcommands with descriptions.
- [ ] A command previously run in this cwd appears as ghost text when typing its prefix.
- [ ] Entering `vim` disables the editor and keys go raw.
- [ ] Disabling the editor in config leaves a classic terminal.

## Open questions

- Automatic menu or Tab-only by default?
- Spec format: custom or Fig-compatible?
