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

### 2. Categorize each comment

Classify each comment, then dispose of it for a real reason — never resolve a
thread performatively:
- **Fix**: valid issue — make the change
- **Refute**: not a real issue — explain why in the reply, with evidence
- **Supersede**: overtaken by a later commit/PR — cite the commit/PR
- **Follow-up**: valid but out of scope here — file an issue, cite it in the reply

(Maps onto the legacy triage labels: **Blocking** -> fix; **Suggestion** -> fix
or refute; **Question** -> refute (answer) or follow-up; **Nitpick** -> fix if
trivial, refute if subjective.)

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

### 4.5 Resolve the conversation thread

After replying with evidence, resolve the GitHub review thread — only once it
has a real disposition (fixed/refuted/superseded/accepted-with-follow-up), not
performatively:
```bash
gh api graphql -f query='mutation { resolveReviewThread(input: {threadId: "<thread-id>"}) { thread { isResolved } } }'
```
Get thread IDs via `gh api graphql -f query='query { repository(owner:"OWNER", name:"REPO") { pullRequest(number: $ARGUMENTS) { reviewThreads(first: 50) { nodes { id isResolved path body } } } } }'`.

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

Before this PR can be marked ready or auto-merge enabled, every requested
reviewer must finish on the current HEAD SHA and every substantive thread must
be resolved (see step 4.5) — check with
`gh pr view $ARGUMENTS --json reviewRequests,reviews,reviewDecision`. Never
enable or retain auto-merge while either condition is unmet.
