# Review and proof currentness

## Three evidence identities

Keep three questions distinct:

1. **Candidate evidence:** what the authored PR candidate establishes.
2. **Base interaction evidence:** whether current `main` changes the same semantic seam or creates a real conflict.
3. **Integration evidence:** what the merge group or landed squash result establishes.

## Candidate-bound evidence

A review, test, or check records the candidate and material claim it actually examined. The head SHA is evidence identity, not an instruction to continuously synchronize ancestry.

Candidate evidence becomes stale when the candidate or semantic subject changes in a way that can affect the conclusion.

Examples:

- implementation changes invalidate affected behavior and candidate review;
- test stimulus or oracle changes invalidate proof review;
- production wiring changes invalidate reachability review;
- material claim, establishment, non-goal, risk, rollback, or review-index changes invalidate claim review even when the Git head is unchanged;
- an authority or support-boundary change may route back to issue preparation.

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
+ no merge conflict
+ no material same-semantic-seam interaction
→ candidate proof and review remain current
```

Do not rebase, update-branch, merge `main`, create empty commits, rerun formal review, or replay full CI merely because the branch is behind.

A merged fix on `main` is a reason to inspect interaction, not automatically a reason to mutate every open candidate. Update the candidate only when the fix materially changes the same semantic seam, resolves an actual conflict, makes the integration result otherwise uninterpretable, or current GitHub branch protection, rulesets, merge queue, or required checks require integration evidence.

## Actual conflict or interaction

```text
actual merge conflict
→ resolve conflict
→ candidate changed
→ rerun affected proof and specialist review
→ submit fresh formal review for the resulting candidate and claim
```

A non-textual same-semantic-seam change on `main` may justify targeted interaction analysis even when Git reports no conflict. Branch mutation is required only when the integration result cannot otherwise be interpreted or current GitHub branch protection, rulesets, merge queue, or required checks require it.

## Affected-only invalidation

After repair or claim revision, rerun supporting evidence dimensions whose semantic subjects changed, then issue a fresh formal-review disposition for the current review subject where required.

| Change | Supporting evidence to refresh |
| --- | --- |
| editorial PR-body wording outside material claim sections | none or local editorial review |
| material claim / establishment / non-goal / risk / rollback / review-index change | claim review plus fresh formal-review record |
| test-only repair | test review and dependent conclusions plus fresh formal-review record for the new head |
| local implementation repair | focused behavior proof and changed-seam candidate review plus fresh formal-review record |
| owner/consumer change | plan, authority review, proof seam, and fresh formal-review record |
| external protocol change | external-truth judgment, dependent claims, and fresh formal-review record |
| any head change after formal review | fresh formal-review record; supporting depth remains proportional |

## Merge boundary

Merge eligibility is determined by current GitHub branch protection, rulesets, merge queue, or required checks, substantive review convergence, and actual mergeability. It must prove that the current head and current material claim match the formal review subject. After squash merge, reconciliation verifies the landed effect on current `main`; it does not pretend the future squash commit could have been reviewed in advance.
