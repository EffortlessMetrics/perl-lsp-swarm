---
name: compile-spec
description: Compile settled issue decisions into a durable specification, ADR, policy, invariant, or builder view when the decision must outlive one PR.
user-invocable: false
---

# Compile specification

Use for durable architecture, semantic ownership, public behavior, migrations, reusable invariants, support/security contracts, or several lasting consumers. Do not create permanent specification ceremony for every localized bug.

Before editing repository contracts, establish or reuse the branch/worktree and one accountable writer for that contract surface. The main thread may own the write. A read-only planner or reviewer returns a proposed delta and evidence to the admitted writer.

Preserve normative behavior, ownership, invariants, failure behavior, compatibility, proof obligations, non-goals, and issue traceability.

## Routes

- `SPEC_CURRENT` → `prepare-proof`
- `NO_SPEC_DELTA` → `prepare-proof`
- `READ_ONLY_RESULT` → return the proposed delta to the contract writer
- `MATERIAL_DECISION_UNRESOLVED` → `prepare-issue`
- `WRITER_COLLISION` / `UNSAFE_WORKTREE` → stop for the mechanical hazard
- `STRUCTURALLY_INVALID` → repair before proceeding
