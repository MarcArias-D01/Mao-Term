# Skills

Each skill is a folder `docs/skills/<name>/SKILL.md` with frontmatter:

```yaml
---
name: <kebab-case-name>      # same as the folder
description: <when to use it, in one sentence; this is what the agent reads to decide>
---
```

Format compatible with Claude Code and opencode. The folder may include supporting files (scripts, examples) referenced from `SKILL.md`.

| Skill | Purpose |
|---|---|
| [spec-driven-development](spec-driven-development/SKILL.md) | Default workflow to implement anything. |
| [write-spec](write-spec/SKILL.md) | Write/update specs and ADRs. |
| [rust-conventions](rust-conventions/SKILL.md) | Style, errors, dependencies, tests. |
| [performance](performance/SKILL.md) | Budgets, hot-path rules, how to measure. |
| [terminal-emulation](terminal-emulation/SKILL.md) | PTY, ConPTY, VT/ANSI/OSC, debugging. |
| [cross-platform](cross-platform/SKILL.md) | Windows + Linux: where and how to isolate OS code. |

## Adding a skill

1. Create `docs/skills/<name>/SKILL.md` with the frontmatter.
2. Add it to this table and to the table in `AGENTS.md`.
3. Keep it short and actionable: steps, rules and checklists, not theory.

## Auto-loading

Claude Code looks in `.claude/skills/` and opencode in `.opencode/skills/` (and also `.claude/skills/`). The source of truth stays in `docs/skills/`; for auto-discovery, link or copy them there (see ADR 0003).
