---
name: find-or-create-issue
description: Explicit atomic skill for reconciling a substantive concern with the narrowest existing GitHub issue or creating a lightweight controlling issue when none exists.
---

# Find or create issue

Search narrowly by outcome, owner, user-visible symptom, and known symbols. Reuse or reconcile an existing issue when it represents the same claim. Create a lightweight issue when durable continuity is useful and no issue exists.

A new issue needs only:

```markdown
## Problem or desired outcome
## Current evidence
## Known context
```

Do not fabricate root cause, exact files, or test code before source investigation. Do not scan or score the entire backlog.

## GitHub updates

Link the selected issue to obvious umbrellas, dependencies, superseding issues, or existing PRs. Apply stable area/kind/risk labels only.

## Routes

- `ISSUE_READY_FOR_RESEARCH` → `$research-issue`
- `EXISTING_CANDIDATE_FOUND` → record it and return to `$prepare-issue`
- `ALREADY_SATISFIED` / `SUPERSEDED` → return to `$deliver-pr`
- `NOT_PROVEN` → preserve the search boundary and missing access
