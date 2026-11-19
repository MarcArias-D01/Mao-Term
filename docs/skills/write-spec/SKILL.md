---
name: write-spec
description: Create or update a spec in docs/specs or record an architecture decision (ADR) in docs/decisions, when a feature is undefined or a non-trivial decision is made.
---

# Writing specs and ADRs

## New spec

1. Copy `docs/specs/_template.md` to `docs/specs/<kebab-name>.md`.
2. Fill in every section. Rules:
   - **Goal** from the user's point of view, not the implementation's.
   - **Out of scope** always explicit (prevents scope creep).
   - **Performance**: concrete numbers and what happens when the feature is inactive (must cost ~0).
   - **Acceptance criteria** verifiable (someone who didn't write the spec must be able to check them).
   - What you don't know → **Open questions**; don't make it up.
3. Add it to the specs table in `docs/README.md` and, where relevant, to `docs/vision.md` and `docs/roadmap.md`.
4. Initial status `Draft`. Moves to `Approved` only when the user validates it.

## Updating a spec

- If the implementation deviates from the spec, update the spec in the same change.
- When an open question is resolved, move the answer into the body and remove it from the list.

## ADR (architecture decision)

Write one when: choosing between alternatives with real trade-offs, adding a major dependency, changing the threading/data model, or changing something from a previous ADR.

1. File `docs/decisions/NNNN-kebab-title.md` (next free number).
2. Sections: **Status** (Proposed/Accepted/Superseded by NNNN), **Date**, **Context**, **Options**, **Decision**, **Consequences** (✅ / ❌ / ⚠️).
3. ADRs are not rewritten: if the decision changes, write a new one that supersedes the old one and update the old one's status.
4. Add it to the table in `docs/README.md` and link it from the affected specs.
