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

## Enforcement status is part of the claim

When a candidate adds or changes a gate, check, linting check, ratchet, or policy, state in the body whether it is **required** or **advisory**, resolved against live protection rather than intent. A body implying a gate blocks merge when it runs advisory overstates the claim, and that overstatement survives the merge as documentation.

Read both enforcement systems. Classic branch protection and repository rulesets are independent and additive, so either alone yields a confidently wrong answer, and a check may be required by one, the other, both, or neither. For rulesets, inspect enforcement status, target refs, and bypass actors before classifying applicability. A gate running inside a composite or conditional job is required only to the extent its calling job reports it; a skipped job reports Success, while a workflow-level skip leaves a required check Pending.

Where a change is deliberately advisory first — a new ratchet awaiting a baseline, or a gate that cannot pass until something merges past it — say so and name the promotion condition. Unenforced-by-design is an honest claim; unenforced-and-described-as-blocking is not.

## A published branch still has one writer

Publishing does not release the candidate. The branch keeps one writer until the claim
merges or is deliberately closed. A reviewer who wants a change requests it.

A second writer pushing to a published branch lands work carrying no proof, risks
silent absorption by the author's next force-push, and diverges the author's local head
from the PR head unnoticed. Each failure looks like the author's, because the branch
still presents as one coherent candidate.

Where a reviewer has already pushed, read what landed and verify it against observed
behavior before adopting it — a reviewer's push carries no proof, so restate it — or
replace it and say why in the thread. Treat the result as a new authored candidate and
invalidate the affected review dimensions.

Recreating a closed PR is separate. If the existing head and base branches still exist
and GitHub permits reopening, reopen and preserve the review record. A fresh PR is needed
when the branches were deleted or the existing PR cannot be reopened after a structural
rebuild; name what it supersedes and carry forward only verified findings. Prefer not
rebuilding — see the currentness contract.

## Routes

- `PR_PUBLISHED_READY` / `PR_RESUMED` → `$address-review-comments`
- `DRAFT_FOR_NAMED_REASON` → run the required remote experiment or collaboration, then repeat this skill
- `DRAFT_REASON_COMPLETE` → recheck readiness, mark the PR ready natively, and return `PR_RESUMED`
- `CANDIDATE_NOT_COHERENT` / `LOCAL_PROOF_STALE` / `WORKTREE_DIRTY` → `$build-candidate`
- `DUPLICATE_OR_WRITER_COLLISION` → reuse/resume or resolve the mechanical conflict
- `IDENTITY_NOT_PROVEN` → stop and resolve branch/candidate identity
