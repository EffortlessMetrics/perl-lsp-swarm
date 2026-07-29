---
name: compile-spec
description: Compile settled issue decisions into a durable specification, ADR, policy, invariant, or builder view when the decision must outlive one PR.
user-invocable: false
---

# Compile specification

Use for durable architecture, semantic ownership, public behavior, migrations, reusable invariants, support/security contracts, or several lasting consumers. Do not create permanent specification ceremony for every localized bug.

Preserve normative behavior, ownership, invariants, failure behavior, compatibility, proof obligations, non-goals, and issue traceability.

## Routes

- `SPEC_CURRENT` → `prepare-proof`
- `NO_SPEC_DELTA` → `prepare-proof`
- `MATERIAL_DECISION_UNRESOLVED` → `prepare-issue`
- `STRUCTURALLY_INVALID` → repair before proceeding
