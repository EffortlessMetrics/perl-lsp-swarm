# Review and proof currentness

## Three evidence identities

Keep three questions distinct:

1. **Candidate evidence:** what the authored PR candidate establishes.
2. **Base interaction evidence:** whether current `main` changes the same semantic seam or creates a real conflict.
3. **Integration evidence:** what the merge group or landed squash result establishes.

## Candidate-bound evidence

A review, test, or check records the candidate it actually examined. The head SHA is evidence identity, not an instruction to continuously synchronize ancestry.

Candidate evidence becomes stale when the candidate or semantic subject changes in a way that can affect the conclusion.

Examples:

- implementation changes invalidate affected behavior and candidate review;
- test stimulus or oracle changes invalidate proof review;
- production wiring changes invalidate reachability review;
- claim wording changes invalidate the affected claim-boundary judgment;
- an authority or support-boundary change may route back to issue preparation.

## Squash-merge currentness

This repository squash-merges. Therefore:

```text
candidate unchanged
+ main advances
+ no merge conflict
+ no material same-semantic-seam interaction
→ candidate proof and review remain current
```

Do not rebase, update-branch, merge `main`, create empty commits, rerun formal review, or replay full CI merely because the branch is behind.

## Actual conflict or interaction

```text
actual merge conflict
→ resolve conflict
→ candidate changed
→ rerun affected proof and review
```

A non-textual same-semantic-seam change on `main` may justify targeted interaction analysis even when Git reports no conflict. Branch mutation is required only when the integration result cannot otherwise be interpreted or live policy demands it.

## Affected-only invalidation

After repair, rerun only evidence dimensions whose semantic subjects changed.

| Change | Likely invalidation |
| --- | --- |
| PR-body wording only | claim review |
| test-only repair | test review and dependent conclusions |
| local implementation repair | focused behavior proof and changed-seam candidate review |
| owner/consumer change | plan, authority review, proof seam |
| external protocol change | external-truth judgment and dependent claims |
| head changes after formal review | formal review for affected dimensions |

## Merge boundary

Merge uses current GitHub policy, substantive review convergence, and actual mergeability. After squash merge, reconciliation verifies the landed effect on current `main`; it does not pretend the future squash commit could have been reviewed in advance.
