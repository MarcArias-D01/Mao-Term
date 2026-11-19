# AGENTS.md — Mao-Term

Instructions for any coding agent (Claude Code, opencode, Codex, etc.) working in this repo.
This file is the **source of truth**; other agent files (`CLAUDE.md`, …) only reference it.

## What Mao-Term is

A modern, fast and efficient terminal written in Rust, GPU-rendered (Warp-style).
Target platforms: **Windows and Linux** (macOS out of scope for now, but don't close the door).

Product pillars (details in `docs/vision.md`):

1. **Performance first** — minimal latency, 0% CPU when idle, low memory.
2. **Autocomplete as you type** — inline and menu suggestions (history, paths, commands, flags).
3. **Easy multi-terminal** — tabs and split panes.
4. **Sidebar** with a file tree of the current directory.
5. **Document viewer** — open and view Markdown, code, images, etc.
6. **Agents panel** — see the AI agent sessions running on the machine.
7. **System monitor** — performance and health of the machine.

## Repo map

```
AGENTS.md              ← you are here
CLAUDE.md              ← only imports AGENTS.md
docs/
  README.md            ← index of all documentation
  vision.md            ← what we build and for whom
  design.md            ← architecture, crates, threading, performance budgets
  roadmap.md           ← milestones (M0…M7) and status
  decisions/           ← ADRs: architecture decisions and their rationale
  specs/               ← one spec per feature/module (+ _template.md)
  skills/              ← agent skills: docs/skills/<name>/SKILL.md
crates/                ← Rust workspace, see docs/design.md
shell-integration/     ← (later) scripts for pwsh, bash, zsh, fish
```

## Skills

Skills live in `docs/skills/<name>/SKILL.md` (frontmatter with `name` + `description`).
**Before starting a task, check this table and read the skills that apply.**

| Skill | Use it when… |
|---|---|
| `spec-driven-development` | You implement or change any feature (default workflow). |
| `write-spec` | A spec must be created/updated or a decision recorded (ADR). |
| `rust-conventions` | You write or review Rust code in this repo. |
| `performance` | You touch rendering, PTY, parsing, input or any hot path; or measure performance. |
| `terminal-emulation` | You work with PTY, ConPTY, VT/ANSI/OSC sequences or shell integration. |
| `cross-platform` | Code depends on the OS (paths, processes, shells, windows, fonts). |

## Workflow (summary)

1. Find the spec in `docs/specs/`. If missing or ambiguous → `write-spec` skill first.
2. Implement in small steps that compile; each step with tests where possible.
3. If you make an architecture decision not yet covered → ADR in `docs/decisions/`.
4. When done, update the spec (status, acceptance criteria checked) and `docs/roadmap.md`.

## Commands

```sh
cargo build                          # build
cargo run -p mao-app                 # run the terminal (binary: mao-term)
MAO_LOG=debug cargo run -p mao-app   # verbose logs (MAO_LOG_FILE=path to log to a file); WGPU_BACKEND=dx12|vulkan|gl
# Testing hooks: MAO_DEBUG_INPUT, MAO_DEBUG_ACTIONS, MAO_DEBUG_BACKGROUND (see skill terminal-emulation)
cargo test --workspace               # tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

## Definition of done

- [ ] `cargo fmt`, `cargo clippy -D warnings` and `cargo test` pass.
- [ ] Works (or compiles with correct `cfg`) on **Windows and Linux**.
- [ ] Does not break the performance budgets in `docs/design.md`.
- [ ] Spec and roadmap updated; ADR if a new decision was made.
- [ ] No new `unwrap()` outside tests (use `?` or `expect("invariant: …")`).

## Golden rules

- **Nothing on the UI thread that can block** (I/O, processes, disk, network → background threads).
- **Render on demand**: only request a frame when something changed. Never a continuous redraw loop.
- **New dependencies must be justified** (see `rust-conventions`): permissive license, maintained, builds on Windows and Linux.
- **OS-specific code is isolated** in `platform/` modules with `#[cfg(...)]`, never scattered.
- Don't invent crate APIs: check the docs of the version we actually use.
- Small commits using Conventional Commits (`feat(render): …`, `fix(pty): …`).

## Language

**Everything in English**: docs, code, comments, commit messages, UI strings.
