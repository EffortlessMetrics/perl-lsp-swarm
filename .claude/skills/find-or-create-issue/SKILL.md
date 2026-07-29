---
name: find-or-create-issue
description: Reconcile a substantive concern with the narrowest existing GitHub issue or create a lightweight controlling issue.
user-invocable: false
---

# Find or create issue

Search narrowly by outcome, owner, symptom, and known symbols. Reuse an issue representing the same claim; otherwise create a lightweight issue with the problem, current evidence, and known context.

Do not invent implementation precision before research and do not scan or score the whole backlog.

## Routes

- `ISSUE_READY_FOR_RESEARCH` → `research-issue`
- `EXISTING_CANDIDATE_FOUND` → return to `prepare-issue`
- `ALREADY_SATISFIED` / `SUPERSEDED` → return to `deliver-pr`
- `NOT_PROVEN` → preserve the search boundary
