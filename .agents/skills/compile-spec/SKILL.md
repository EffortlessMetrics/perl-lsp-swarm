---
name: compile-spec
description: Explicit conditional skill for compiling settled issue decisions into a durable specification, ADR, policy, invariant, or linked builder view when the decision must outlive one PR.
---

# Compile specification

Use only when the accepted decision has durable value, including cross-PR architecture, semantic ownership, public API or protocol behavior, migration, reusable invariants, support commitments, security boundaries, or several lasting consumers.

Do not create a permanent specification for every localized bug.

## Mutation boundary

Before editing repository contracts, establish or reuse an appropriate branch/worktree and one accountable writer for the contract surface. The root may own that write directly. A read-only planner or reviewer returns a proposed contract delta and evidence to the admitted writer.

This is mechanical write admission for a durable repository mutation, not a requirement for another persona or human sign-off.

Compile the settled decision into the repository's existing contract surface. Preserve:

- normative behavior and vocabulary;
- owner and consumer boundaries;
- invariants and failure behavior;
- compatibility and migration disposition;
- proof obligations;
- explicit non-goals;
- links back to the controlling issue.

Do not copy unresolved issue history into normative prose.

## Routes

- `SPEC_CURRENT` → `$prepare-proof`
- `NO_SPEC_DELTA` → `$prepare-proof` using the existing contract
- `READ_ONLY_RESULT` → return the proposed delta to the admitted contract writer
- `MATERIAL_DECISION_UNRESOLVED` → `$prepare-issue`
- `WRITER_COLLISION` / `UNSAFE_WORKTREE` → stop and resolve the mechanical hazard
- `STRUCTURALLY_INVALID` → repair the contract before proceeding
