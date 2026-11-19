# Spec: Agents panel

- **Status:** Draft
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

## Acceptance criteria

- [ ] Claude Code and opencode launched in another terminal show up with their cwd within ≤ 3 s.
- [ ] One launched inside Mao-Term shows "[in tab N]" and "Go to pane" works.
- [ ] Closing the agent removes it from the list on the next scan.
- [ ] A detector added in the config works without restarting.

## Open questions

- Which agents to include by default? (proposal: Claude Code, opencode, Codex CLI, Gemini CLI, Aider, Cursor Agent, Goose)
- Show IDE agents too (Cursor, Copilot in VS Code)? Probably not: they are editor processes.
