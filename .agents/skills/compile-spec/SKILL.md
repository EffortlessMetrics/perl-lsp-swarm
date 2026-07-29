---
name: compile-spec
description: Explicit conditional skill for compiling settled issue decisions into a durable specification, ADR, policy, invariant, or linked builder view when the decision must outlive one PR.
---

# Compile specification

Use only when the accepted decision has durable value, including cross-PR architecture, semantic ownership, public API or protocol behavior, migration, reusable invariants, support commitments, security boundaries, or several lasting consumers.

Do not create a permanent specification for every localized bug.

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
- `MATERIAL_DECISION_UNRESOLVED` → `$prepare-issue`
- `STRUCTURALLY_INVALID` → repair the contract before proceeding
