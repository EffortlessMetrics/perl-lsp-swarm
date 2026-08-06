# Review and proof currentness

Use this document with
[`PR_REVIEW_STANDARD.md`](PR_REVIEW_STANDARD.md). The review standard defines the
substantive acceptance judgment; this document defines when later changes can alter
that judgment. Live integration posture remains a separate GitHub fact.

## Three evidence subjects

Keep three questions distinct:

1. **Candidate evidence:** what the pull request's cumulative change establishes.
2. **Integration evidence:** whether that candidate combines safely with the current
   base or merge group.
3. **Landed evidence:** what the final squash result establishes on `main`.

Movement in one does not automatically invalidate the others.

## Review is semantic, not exact-head

A review is a judgment about a claim, implementation, proof, production path, risk,
and current substantive review result. The PR head SHA identifies the code currently
visible on GitHub, but it is not a review-validity token.

Do not require:

- a review submitted on the latest commit solely because the SHA changed;
- a material-claim digest;
- `review-start` / `review-done` receipt comments;
- a full `deep` review after every repair push.

The durable review record is the useful GitHub review itself:

- submitted review conclusions and substantive result;
- inline findings;
- replies and evidence-backed dispositions;
- follow-up review of the seams changed by later repairs.

A clean review is valid and should state concisely what was checked and what remains
unproved.

## Semantic invalidation

Later work changes review currentness only where it can change the conclusion.

| Later change | Review response |
| --- | --- |
| formatting or editorial cleanup | no review refresh unless meaning changed |
| generated receipt or inventory refresh | verify the generator/input relation; no full review |
| stronger or additional tests with unchanged production behavior | review proof implications only |
| fix for one review finding | verify that finding, its proof, and the changed seam |
| local implementation repair | focused behavior and changed-seam review |
| material claim or non-goal change | review the changed claim boundary |
| production route or consumer change | review reachability and dependent conclusions |
| authority, compatibility, security, packaging, migration, support, or rollback change | review the affected risk dimensions |
| actual conflict resolution | review the conflict-affected seam and proof |
| combined-tree failure and repair | review the concrete interaction and repair |

A SHA change by itself appears nowhere in this table.

## Review-forward repair

Review is cumulative. Earlier findings and clean conclusions remain useful unless
later work materially changes their subject.

After a repair:

```text
identify changed semantic subjects
→ rerun affected proof
→ verify addressed findings
→ review newly changed risk/claim dimensions
→ update the substantive review result
→ continue
```

Do not restart the entire review sequence merely to manufacture a new current-head
receipt. A result may remain `REVIEW_CURRENT` after a non-semantic edit, become
`CHANGES_REQUIRED` after a substantive regression, or become `NOT_PROVEN` when the
repair invalidates evidence and reliable replacement proof is missing.

## GitHub-native merge blockers

Substantive review and live integration remain separate. A useful current review must
reach `REVIEW_CURRENT` before checks and mergeability can establish
`INTEGRATION_READY`.

The live merge decision then remains governed by current GitHub facts:

- draft state;
- unresolved review threads;
- current `CHANGES_REQUESTED` reviews;
- deliberately requested reviewers still pending where their review is part of the
  claim;
- required checks;
- actual conflicts and mergeability;
- rulesets, merge queue, and applicable release/changelog policy.

Pending GitHub-owned transitions yield `PR_IN_FLIGHT`; concrete integration blockers
yield `MERGE_BLOCKED`; missing reliable integration data yields `NOT_PROVEN`. None of
those states automatically changes a still-current substantive review.

Green checks or textual mergeability cannot create `REVIEW_CURRENT`. Stale bot or
human review timestamps may be reported as context. They do not block by themselves.

## Squash-merge currentness

This repository squash-merges.

```text
candidate remains conflict-free
+ unrelated main work lands
→ do nothing
```

Do not rebase, update the branch, create empty commits, replay full CI, or rerun review
merely because `main` advanced.

If Git reports a real conflict, the later lane resolves it and refreshes only the
affected proof/review. If an explicit stack or combined-tree check exposes a real
interaction, repair that interaction rather than predicting overlap in advance.

## Expected-head merge safety

At the instant of merge, use the current PR head SHA as compare-and-swap protection so
a branch cannot move between inspection and merge:

```text
gh pr merge <n> --squash --match-head-commit <current-head-sha>
```

This is merge race protection. It is not review currentness and does not justify
exact-head review comments.

## Landed reconciliation

After squash merge, verify the landed effect on current `main`, update the controlling
issue and durable claims, preserve residual work, and clean the branch/worktree. The
future squash commit was not—and did not need to be—the formal review subject.
