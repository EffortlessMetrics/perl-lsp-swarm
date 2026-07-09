---
description: Address PR review comments — read feedback, fix issues, push updates
argument-hint: "<PR number>"
---

# PR Respond

Read and address review comments on PR **$ARGUMENTS**.

## Process

### 1. Read the review feedback
```bash
gh pr view $ARGUMENTS --comments
gh api repos/:owner/:repo/pulls/$ARGUMENTS/reviews --jq '.[] | "\(.user.login) (\(.state)): \(.body)"'
gh api repos/:owner/:repo/pulls/$ARGUMENTS/comments --jq '.[] | "\(.path):\(.line) \(.body)"'
```
> **MCP alternatives (web/no-gh sessions):**
> - PR review summaries: `mcp__github__pull_request_read(method:"get_reviews", pullNumber:<number>)`.
> - PR review line comments: `mcp__github__pull_request_read(method:"get_review_comments", pullNumber:<number>)` → each comment has `.path`, `.line`, `.body`.

### 2. Categorize each comment
- **Blocking** (changes requested): must fix before merge
- **Suggestion**: apply if it's an improvement, explain if you disagree
- **Question**: answer in a reply comment
- **Nitpick**: fix if trivial, skip if subjective

### 3. Address blocking comments
For each blocking comment:
1. Read the file and understand the concern
2. Make the fix
3. Commit: `fix(review): address <reviewer> feedback — <what>`

### 4. Reply to comments
```bash
gh api repos/:owner/:repo/pulls/$ARGUMENTS/comments/<comment-id>/replies \
  -f body="Fixed in <commit-hash>. <brief explanation>"
```

Or for general review comments:
```bash
gh pr comment $ARGUMENTS --body "Addressed review feedback:
- <comment 1>: fixed in <hash>
- <comment 2>: <explanation>
"
```
> **MCP alternatives (web/no-gh sessions):**
> - Reply to a specific line comment: `mcp__github__add_reply_to_pull_request_comment(pullNumber:<number>, commentId:<comment-id>, body:"Fixed in <commit-hash>. <brief explanation>")`.
> - General PR comment: `mcp__github__add_issue_comment(issue_number:<number>, body:"Addressed review feedback:\n- <comment 1>: ...")` — `issue_number` works for PRs.

### 5. Re-verify
```bash
cargo xtask fmt
cargo clippy -p <crate> --tests -- -D warnings
cargo test -p <crate>
```

### 6. Push, set label, and request re-review
```bash
git push
```

Apply the `pr-responded` label with verification (see `/label-apply-verified`):
```
/label-apply-verified pr $ARGUMENTS "pr-responded"
```

```bash
gh pr edit $ARGUMENTS --add-reviewer <original-reviewer> 2>/dev/null || true
```
