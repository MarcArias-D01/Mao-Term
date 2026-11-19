# ADR 0003 — AGENTS.md and skills in docs/, portable across agents

- **Status:** Accepted
- **Date:** 2026-11-19

## Context

We want to work on the repo with different agents (Claude Code, opencode, Codex…) without duplicating instructions, and keep the structure extractable to other projects.

## Decision

- `AGENTS.md` at the root is the source of truth (read natively by opencode, Codex and others).
- `CLAUDE.md` only contains `@AGENTS.md` (Claude Code imports the file).
- Skills in `docs/skills/<name>/SKILL.md` with `name` + `description` frontmatter (format compatible with Claude Code and opencode).
- `AGENTS.md` includes a skills table so any agent can discover them even without auto-loading.
- Specs in `docs/specs/`, decisions in `docs/decisions/`.

## Consequences

- ✅ One place to maintain; `docs/skills/` can be copied to another project.
- ❌ Neither Claude Code nor opencode auto-discover `docs/skills/` (they look in `.claude/skills/`, `.opencode/skills/`…). If we want auto-loading, we add a script that links or copies them there, without moving the source.
