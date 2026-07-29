---
name: research-issue
description: Establish current source, ownership, consumers, reproduction, related work, and external truth for one issue.
user-invocable: false
---

# Research issue

Inspect current source rather than accepting the issue's opening theory. Separate observations, inferences, contradictions, and unknowns. Preserve material evidence and corrected assumptions in GitHub.

Read-heavy source mapping, reproduction, related-work search, and external research may use subagents; one main-thread synthesis controls the issue update.

## Routes

- `ISSUE_RESEARCHED` → `review-issue`
- `MORE_RESEARCH_NEEDED` → continue with the named question
- `ALREADY_SATISFIED` / `SUPERSEDED` → `deliver-pr`
- `MISSING_AUTHORITY` / `NOT_PROVEN` → preserve the uncertainty
