# Spec: Agents panel

- **Status:** In progress
- **Milestone:** M6
- **Crates:** `mao-agents`, `mao-ui`, `mao-app`
- **Related:** [workspace-layout](workspace-layout.md), [system-monitor](system-monitor.md)

## Goal

An activity bar button that shows **which AI coding agents are running on the machine** (Claude Code, opencode, Codex, Gemini CLI, Aider…), where they are working and what they consume, with quick access to them.

## Scope

- Detection of known agent processes, **inside and outside** Mao-Term.
- Per session: agent, icon, cwd (project), PID, uptime, CPU and RAM, and whether it runs inside a Mao-Term pane.
- Grouping by project (cwd).
- Actions:
  - **Go to pane** if running inside Mao-Term.
  - **Open terminal in its cwd**.
  - **Open the agent's sessions/logs folder** if known (e.g. `~/.claude/projects/…`).
  - **Kill process** (with confirmation).
- Badge on the activity bar and counter in the status bar with the number of active agents.
- Configurable detectors: users can add new agents without recompiling.

## Out of scope (v1)

- Reading conversation contents / detailed status ("thinking", "waiting for permission"…) → Future.
- Controlling agents (sending prompts).
- Remote or cloud agents.

## Behavior

```
AGENTS (3)
▾ Mao-Term  ~/Documents/Mao-Term
   ● Claude Code   pid 4120 · 42 min · 1.2% · 310 MB   [in tab 2]
▾ api-server  ~/work/api
   ● opencode      pid 8812 · 5 min · 0.4% · 180 MB
   ● Codex         pid 9001 · 1 h · 0.0% · 95 MB
```

Explicit empty ("No agents running") and error (no permission to read processes) states.

## Technical design

```toml
# default detectors (overridable in config)
[[agents.detector]]
name = "Claude Code"
match_exe = ["claude", "claude.exe"]
match_cmdline = ["@anthropic-ai/claude-code"]   # when running under node
sessions_dir = "~/.claude/projects"

[[agents.detector]]
name = "opencode"
match_exe = ["opencode", "opencode.exe"]
```

- `sysinfo` to list processes (name, cmdline, cwd, CPU, memory, start time).
- Agents running as `node`/`python`: detected via `cmdline`.
- Dedupe: keep the agent's root process (not its children).
- **Inside Mao-Term**: walk up parent PIDs until finding the `child_pid` of a `TerminalSession`.
- cwd: Linux via `/proc`; Windows via `sysinfo` (may fail for other users'/elevated processes → show "—").
- Scan every 3 s **only** while the panel is open; if only the status bar counter is active, every 10 s and by exe name only (cheap).

## Performance

- One scan < 20 ms with 500 processes.
- Panel closed and counter disabled → 0 scans.

## Privacy

- Fully local and read-only. Session contents are not read in v1.

## Implementation notes (M6)

- `mao-agents` works on `mao-sysmon`'s process list (one `sysinfo` refresh shared with the monitor). The cwd is read only for processes a detector accepts (reading it is slow on Windows).
- Command-line patterns include the package folder (`@github/copilot/`), so editor-embedded runtimes (VS Code's `@github/copilot-sdk-…`) are not reported as CLI agents; the Claude desktop app is excluded by path.
- "In Mao-Term": each pane's shell pid (from the PTY) is looked up among the agent's ancestors.
- Sampling runs only while the agents or monitor panel is open (every 3 s with only the agents panel). The activity-bar badge and the status bar count are shown while it runs; a background mode for them (spec: every 10 s) is not implemented, to keep idle CPU at 0%.
- Detectors: built-ins + `[[agents.detector]]` in the config (same name replaces a built-in); read at startup.

## Acceptance criteria

- [x] Claude Code launched in another terminal shows up with its cwd within ≤ 3 s (verified with this repo's own Claude Code session). opencode: detector written, not verified (not installed).
- [x] One launched inside Mao-Term shows "tab N"; "Go to Pane" is in its context menu.
- [x] Closing the agent removes it from the list on the next scan.
- [ ] A detector added in the config works without restarting (needs config hot reload; today: restart).

## Open questions

- Which agents to include by default? (proposal: Claude Code, opencode, Codex CLI, Gemini CLI, Aider, Cursor Agent, Goose)
- Show IDE agents too (Cursor, Copilot in VS Code)? Probably not: they are editor processes.
