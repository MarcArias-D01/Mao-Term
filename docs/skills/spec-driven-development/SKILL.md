---
name: spec-driven-development
description: Default workflow to implement or change any Mao-Term feature, starting from its spec in docs/specs and ending with the spec and roadmap updated.
---

# Spec-driven development

## 1. Understand

1. Read `AGENTS.md` and the relevant section of `docs/design.md`.
2. Find the spec in `docs/specs/` (index in `docs/README.md`).
   - Missing, or it has open questions that block the task → use the `write-spec` skill and **stop to confirm with the user** before implementing.
3. Read the ADRs linked from the spec.
4. Identify which other skills apply (performance, platform, emulation) and read them.

## 2. Plan

- Split into small steps; each step compiles and leaves the program working.
- Order: domain types and logic testable without a GPU first, then integration in `mao-app`, then UI.
- If the plan contradicts the spec or design, update the spec (or write an ADR) **before** coding.

## 3. Implement

- Respect the crate dependency rules in `docs/design.md`.
- Unit tests for logic (tokenizer, tree, OSC parsing, ranking…).
- Nothing that blocks the UI thread.
- OS code isolated (`cross-platform` skill).

## 4. Verify

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- Manual test of the spec's flow with `cargo run -p mao-app`.
- If you touched hot paths, measure (`performance` skill).
- Go through the spec's acceptance criteria one by one.

## 5. Close

- Check off the satisfied criteria in the spec and update its **Status**.
- Update `docs/roadmap.md` and the table in `docs/README.md` if the status changed.
- Summarize for the user: what was done, what was verified and how, what remains.
- Commit with Conventional Commits (`feat(complete): git branch suggestions`).
