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
> **MCP alternative (web/no-gh sessions):** `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)` provides PR metadata. Fetch by numeric PR ref to avoid interpolating an untrusted branch name: `git fetch origin "refs/pull/<number>/head:refs/remotes/origin/pr-<number>" && git checkout --detach "refs/remotes/origin/pr-<number>"`; push with `git push`. For `gh pr review --approve`, first create the pending review with `mcp__github__pull_request_review_write(method:"create", owner, repo, pullNumber:<number>, body:"Deep review: <what you improved>. Logic verified, low regression risk.")`, then submit it with `mcp__github__pull_request_review_write(method:"submit_pending", owner, repo, pullNumber:<number>, event:"APPROVE", body:"Deep review: <what you improved>. Logic verified, low regression risk.")`; or use `mcp__github__add_issue_comment(owner, repo, issue_number:<number>, body:"Deep review: ...")` to post a textual approval noting deep-reviewed, since the label (`/label-apply-verified`) is the actual merge gate.
```
/label-apply-verified pr <number> "deep-reviewed"
```
```bash
gh pr edit <number> --remove-label "needs-deep-review"
```
> **MCP alternative (web/no-gh sessions):** Read current labels via `mcp__github__pull_request_read(method:"get", owner, repo, pullNumber:<number>)`, then write the union minus `needs-deep-review` via `mcp__github__issue_write(method:"update", owner, repo, issue_number:<number>, labels:[...])` — **labels are replaced, not appended** (read current list first). See [docs/reference/GH_MCP_FALLBACK.md].

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
> **MCP alternative (web/no-gh sessions):** First create the pending review with `mcp__github__pull_request_review_write(method:"create", owner, repo, pullNumber:<number>, body:"<reason>")`, then submit it with `mcp__github__pull_request_review_write(method:"submit_pending", owner, repo, pullNumber:<number>, event:"REQUEST_CHANGES", body:"<reason>")` — full parity for requesting changes.

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
