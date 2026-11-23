# Context: Direct-call resolution assessment — Issue #11138

## Problem

Several incomplete answers to “does this direct call resolve?” coexist in diagnostics
and scope analysis. Import and diagnostic consumers need one transport-neutral,
generation-bound assessment with explicit dynamic, external, readiness, and
call-shape boundaries.

## Authority

| Owner | Role |
|-------|------|
| #11138 | Call resolution semantics and `DirectCallableResolutionAssessment` |
| #8911 / `SemanticQueryOutcome` | Generic typed query completeness vocabulary |
| #14054 / #12063 / #13288 | Canonical view integration and structural completeness |
| #11145 | Diagnostic projection only (no reclassification) |
| #790 | Import consumer (downstream) |

## This slice (UCF-01 commit 1)

- In-repo specification packet and builder handoff.
- Versioned subject, evidence-axis, and assessment types in `perl-semantic-facts`.
- Conservative disposition derivation in `perl-semantic-analyzer` that **never**
  emits `exact_unresolved` from empty `Vec`/`Option` legacy inputs or incomplete
  denominators.
- Shift-left falsifiers for method exclusion, external-unindexed, and empty-query
  mutants.

Positive resolution paths, live workspace query integration, and qualified-call
classifier convergence remain for follow-up slices after the canonical view
denominator path (#14054) is safe.

## Non-goals (this PR)

No diagnostic codes, import candidates, LSP types, provider migration, or
second exact qualified classifier removal.
