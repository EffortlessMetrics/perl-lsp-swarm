# Review and proof currentness

## Three evidence identities

Keep three questions distinct:

1. **Candidate evidence:** what the authored PR candidate establishes.
2. **Integration-basis evidence:** whether this candidate can be applied and evaluated against the current base or merge group.
3. **Landed evidence:** what the final squash result establishes after merge.

These are separate subjects. Movement in one does not automatically invalidate the others.

## Candidate-bound evidence

A review, test, or check records the candidate and material claim it actually examined. The head SHA is evidence identity, not an instruction to continuously synchronize ancestry.

Candidate evidence becomes stale when the candidate or semantic subject changes in a way that can affect the conclusion.

Examples:

- implementation changes invalidate affected behavior and candidate review;
- test stimulus or oracle changes invalidate proof review;
- production wiring changes invalidate reachability review;
- material claim, establishment, non-goal, risk, rollback, or review-index changes invalidate claim review even when the Git head is unchanged;
- an authority or support-boundary change may route back to issue preparation.

Unrelated `main` movement does not change the candidate and therefore does not stale candidate proof or review.

## Formal-review currentness

A formal review is a disposition of one complete review subject:

```text
full candidate head SHA
+ normalized material PR claim/review index
```

The review record should preserve both the reviewed head and a digest or exact stable representation of the material PR claim sections. At minimum those sections include `Claim`, `What this establishes`, `What this does not establish`, `Risk and rollback`, and the substantive `Review index`.

Any head change or material claim/review-index change after formal review requires a new formal-review record before merge. Editorial changes that do not alter the claim, evidence boundary, risk, rollback, or reviewer map do not require review churn.

The depth of supporting re-examination remains proportional:

- rerun focused proof and specialist lenses only where their semantic subjects changed;
- inspect the new cumulative candidate and material claim sufficiently to verify that the changed-seam classification is itself sound;
- then submit a fresh `REVIEW_CURRENT`, `REVIEW_FINDINGS_OPEN`, or `REVIEW_NOT_PROVEN` judgment for the new review subject.

This preserves affected-only proof economics without allowing an unreviewed candidate revision or claim change to inherit a formal disposition from an older review subject.

## Squash-merge currentness

This repository squash-merges. Therefore:

```text
candidate head unchanged
+ material claim unchanged
+ main advances
+ no actual Git conflict
+ no required merge-group or combined-tree check has failed
→ candidate proof and review remain current
```

Do not rebase, update-branch, merge `main`, create empty commits, rerun formal review, or replay full CI merely because the branch is behind.

Do not proactively inspect sibling PR implementations, touched-file overlap, or nearby semantic surfaces to predict interactions. The candidate lane normally learns about integration through Git mergeability, an explicit stacked prerequisite, current GitHub merge-queue or required-check behavior, or an actual synthetic or hosted combined-tree result.

A changed integration basis may require a fresh merge-group or combined-tree check when current GitHub branch protection, rulesets, merge queue, or required checks require it. It does not by itself require branch mutation or fresh candidate review.

## Actual conflict or integration failure

```text
actual merge conflict
→ affected lane resolves conflict
→ candidate changed
→ rerun affected proof and specialist review
→ submit fresh formal review for the resulting candidate and claim
```

```text
combined-tree or merge-group proof fails without a text conflict
→ report the concrete interaction to the affected lane
→ repair the smallest coherent candidate
→ rerun affected proof/review
```

A lane does not need advance knowledge of another lane's implementation. A direct issue or PR comment is sufficient when a prerequisite or concrete integration finding materially affects it.

## Affected-only invalidation

After repair or claim revision, rerun supporting evidence dimensions whose semantic subjects changed, then issue a fresh formal-review disposition for the current review subject where required.

| Change | Supporting evidence to refresh |
| --- | --- |
| editorial PR-body wording outside material claim sections | none or local editorial review |
| material claim / establishment / non-goal / risk / rollback / review-index change | claim review plus fresh formal-review record |
| test-only repair | test review and dependent conclusions plus fresh formal-review record for the new head |
| local implementation repair | focused behavior proof and changed-seam candidate review plus fresh formal-review record |
| conflict resolution | conflict-affected proof/review plus fresh formal-review record |
| owner/consumer change | plan, authority review, proof seam, and fresh formal-review record |
| external protocol change | external-truth judgment, dependent claims, and fresh formal-review record |
| integration basis changes but candidate remains unchanged | merge-group or combined-tree checks only where current GitHub branch protection, rulesets, merge queue, or required checks require them |
| any head change after formal review | fresh formal-review record; supporting depth remains proportional |

## Merge boundary

Merge eligibility is determined by current GitHub branch protection, rulesets, merge queue, or required checks, substantive review convergence, actual mergeability, and any merge-group or combined-tree check required for the selected candidate by that GitHub policy.

After squash merge, reconciliation verifies the landed effect on current `main`; it does not pretend the future squash commit could have been reviewed in advance.
