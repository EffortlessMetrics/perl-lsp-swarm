---
name: publish-pr
description: Explicit atomic skill for publishing one locally complete coherent candidate through a concise GitHub review index, ready by default, with draft reserved for a named remote-only proof or collaboration need.
---

# Publish PR

Verify branch, worktree, candidate, base, and controlling issue identity. Confirm no equivalent active PR or real writer collision exists.

## Ready-publication threshold

A candidate publishes ready only when all applicable local preparation is current:

- focused and affected proof passes on the candidate;
- relevant negative, stale, failure, and recovery protection exists;
- test hardening, simplification, and mutable local candidate review are complete;
- the worktree contains no accidental or unsalvaged changes;
- the controlling issue, claim boundary, and governing contract are current;
- Changie/changelog, support, migration, and release dispositions are complete or explicitly not applicable;
- the candidate is one coherent acceptance-and-rollback claim.

If this threshold is not met, return to `$build-candidate` rather than opening a churn-producing ready PR.

## Draft exceptions and transition

Use draft only for:

- remote-only proof or platform behavior;
- real branch collaboration;
- early visible ownership that prevents duplicate work;
- a protected integration experiment whose remote behavior is the subject.

Record the exact draft reason and its completion condition in the PR body.

For an existing draft, inspect that named condition. When it is complete, re-evaluate the full ready-publication threshold and explicitly mark the PR ready through the provider's native GitHub action (for example `gh pr ready <n>` or the equivalent connector operation). Do not leave a completed draft in a self-repeating `DRAFT` state. If the threshold is no longer met, return to candidate repair instead of marking ready.

## PR review index

```markdown
## Claim
## Controlling issue
## Governing contract
## Changed production path
## Proof
## Test hardening
## Simplification
## Deviations
## What this establishes
## What this does not establish
## Risk and rollback
## Review index
```

Do not claim hosted proof, formal review, or merge readiness before current GitHub evidence exists.

## Routes

- `PR_PUBLISHED_READY` / `PR_RESUMED` → `$address-review-comments`
- `DRAFT_FOR_NAMED_REASON` → run the required remote experiment or collaboration, then repeat this skill
- `DRAFT_REASON_COMPLETE` → recheck readiness, mark the PR ready natively, and return `PR_RESUMED`
- `CANDIDATE_NOT_COHERENT` / `LOCAL_PROOF_STALE` / `WORKTREE_DIRTY` → `$build-candidate`
- `DUPLICATE_OR_WRITER_COLLISION` → reuse/resume or resolve the mechanical conflict
- `IDENTITY_NOT_PROVEN` → stop and resolve branch/candidate identity
