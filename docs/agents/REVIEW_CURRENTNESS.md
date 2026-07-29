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

## Formal-review currentness

A formal review is a disposition of one complete candidate revision. Any head change after formal review requires a new formal-review record bound to the new head before merge.

The depth of supporting re-examination remains proportional:

- rerun focused proof and specialist lenses only where their semantic subjects changed;
- inspect the new cumulative candidate sufficiently to verify that the changed-seam classification is itself sound;
- then submit a fresh `REVIEW_CURRENT`, `REVIEW_FINDINGS_OPEN`, or `REVIEW_NOT_PROVEN` judgment for the new head.

This preserves affected-only proof economics without allowing an unreviewed candidate revision to inherit a formal disposition from an older head.

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
→ rerun affected proof and specialist review
→ submit fresh formal review for the resulting candidate
```

A non-textual same-semantic-seam change on `main` may justify targeted interaction analysis even when Git reports no conflict. Branch mutation is required only when the integration result cannot otherwise be interpreted or live policy demands it.

## Affected-only invalidation

After repair, rerun supporting evidence dimensions whose semantic subjects changed, then issue a fresh candidate-level formal-review disposition for the new head.

| Change | Supporting evidence to refresh |
| --- | --- |
| PR-body wording only | claim review |
| test-only repair | test review and dependent conclusions |
| local implementation repair | focused behavior proof and changed-seam candidate review |
| owner/consumer change | plan, authority review, proof seam |
| external protocol change | external-truth judgment and dependent claims |
| any head change after formal review | fresh formal-review record; supporting depth remains proportional |

## Merge boundary

Merge uses current GitHub policy, substantive review convergence, and actual mergeability. After squash merge, reconciliation verifies the landed effect on current `main`; it does not pretend the future squash commit could have been reviewed in advance.
