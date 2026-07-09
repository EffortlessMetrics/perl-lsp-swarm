---
description: Deep reviewer step 4 — approve or send back with analysis
user-invocable: false
---

# Deep Reviewer Decide

Make the final call based on your analysis. You are the final quality gate before merge.

## Decision tree

### Default → Fix forward and approve

Push improvements directly: add edge case tests, fix logic issues, simplify code. Then approve with a summary of what you changed and set the `deep-reviewed` label to signal approval.

```bash
gh pr checkout <number>
# ... make improvements, commit ...
git push
gh pr review <number> --approve --body "Deep review: <what you improved>. Logic verified, low regression risk."
```
> **MCP alternatives (web/no-gh sessions):**
> - `gh pr checkout`: no MCP equivalent — in worktree: `git fetch origin pull/<number>/head:<branch> && git checkout <branch>`.
> - `gh pr review --approve`: `mcp__github__pull_request_review_write(method:"create", pullNumber:<number>, event:"APPROVE", body:"Deep review: ...")`.
> - `gh pr edit --remove-label`: read current labels with `mcp__github__pull_request_read(method:"get", pullNumber:<number>)`, then write back filtered list with `mcp__github__issue_write(method:"update", issue_number:<number>, labels:[...current minus "needs-deep-review"])`.
```
/label-apply-verified pr <number> "deep-reviewed"
```
```bash
gh pr edit <number> --remove-label "needs-deep-review"
```

After approval, write a version-bound receipt:
```
/label-receipt-write pr <number> deep-reviewed reviewer-deep
```

The `deep-reviewed` label is **required for non-docs PRs** before a PR can be marked `merge-ready`. Docs-only PRs may use the reviewer fast-track path instead; do not set `deep-reviewed` unless you actually performed the deep review.

### Logic issues → Fix them on the branch
You're a sonnet agent on an isolated branch. If the logic is wrong but the approach is right, fix the logic yourself. Only send back if the approach is fundamentally wrong.

### Structural problems → Send back (rare)
Only when the approach is wrong, wrong crate, or the codebase moved too far:
```bash
gh pr review <number> --request-changes --body "<what's structurally wrong and why it can't be fixed locally>"
```
> **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_review_write(method:"create", pullNumber:<number>, event:"REQUEST_CHANGES", body:"<what's structurally wrong>")` — same semantics as `--request-changes`.

## Rules

- **You are the final quality gate.** On approval, you MUST set the `deep-reviewed` label. Without it, the PR cannot merge.
- **Fix forward is the default.** If you can fix it, fix it.
- "I would have done it differently" → make it how you'd do it and push.
- Edge cases → add the test yourself, don't file a follow-up.
- Send back only for structural issues you can't resolve on the branch.
- **Recommend next steps.** Typical recommendations:
  - "Approved with improvements — `deep-reviewed` set, ready for merge"
  - "Approved — recommend a follow-up builder for the related edge case I found in X"
  - "Fixed logic bug on branch — recommend a second deep-review to verify my fix"
