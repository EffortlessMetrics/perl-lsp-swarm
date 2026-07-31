---
name: research-issue
description: Explicit atomic skill for establishing the current source, owner, consumer, reproduction, related-work, and external-truth basis of one issue before planning.
---

# Research issue

Inspect current source rather than relying on the issue's initial theory.

Establish:

- the observed behavior and reproducible boundary;
- current semantic owner and production consumers;
- overlapping or superseding PRs and issues;
- existing tests, receipts, and known limitations;
- relevant external authority;
- observations, inferences, contradictions, and unknowns as separate categories.

Post material evidence and corrections to the issue. Update the current synthesis when the initial premise is wrong.

## Orchestration

Read-heavy owner mapping, related-work search, reproduction, and external research may run in parallel. One integrator preserves the joined evidence.

## Routes

- `ISSUE_RESEARCHED` → `$review-issue`
- `MORE_RESEARCH_NEEDED` → continue this skill with the named question
- `ALREADY_SATISFIED` / `SUPERSEDED` → return to `$deliver-pr`
- `MISSING_AUTHORITY` / `NOT_PROVEN` → preserve the exact uncertainty
