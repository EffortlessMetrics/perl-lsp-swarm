# Review and proof currentness

This document defines when later changes can alter an existing review judgment. It is
not the operational review procedure.

```text
Claude Code review operation
→ CLAUDE.md and .claude/skills/*

Codex review operation
→ AGENTS.md and .agents/skills/*

shared currentness semantics
→ this document
```

Live integration posture remains a separate GitHub fact.

## Three evidence subjects

Keep three questions distinct:

1. **Candidate evidence:** what the pull request's cumulative change establishes.
2. **Integration evidence:** whether that candidate combines safely with the current
   base or merge group.
3. **Landed evidence:** what the final squash result establishes on `main`.

Movement in one does not automatically invalidate the others.

## Three identities

Do not collapse three different identities into one freshness rule.

| Subject | Identity | What can invalidate it |
| --- | --- | --- |
| semantic candidate and proof | the cumulative PR change and the named claim/proof subjects | a later PR commit that can change the claim, implementation, production route, or tested seam |
| base integration | the candidate combined with the current base | an actual conflict or demonstrated combined-tree interaction |
| merge race | the current PR head SHA | any branch push; this identity is used only for compare-and-swap protection at merge |

`main` advancing without a conflict or concrete combined-tree interaction changes none
of the candidate's semantic evidence. A new PR head does not erase completed proof for
subjects the new commit cannot affect. The merge-race SHA is not a review or CI
freshness policy.

## Review is semantic, not exact-head

A review is a judgment about a claim, implementation, proof, production path, risk,
and current substantive review result. The PR head SHA identifies the code currently
visible on GitHub, but it is not a review-validity token.

Do not require:

- a review submitted on the latest commit solely because the SHA changed;
- a material-claim digest;
- `review-start` / `review-done` receipt comments;
- a full deep review after every repair push.

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
→ review newly changed risk/claim dimensions through the provider-native flow
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

### Optional late rebase

Commit distance is a cost signal, not an acceptance condition. Once a candidate is
otherwise merge-ready, the lane owner may choose one rebase immediately before merge
when the branch is many commits behind and evaluating the refreshed integration is
cheaper or safer than carrying the old base. This is an optional, one-time late action,
not a duty to maintain zero distance from `main`.

Do not rebase repeatedly as `main` continues to move. After the optional late rebase,
refresh only the proof and review subjects that the rebase actually changed. A real
conflict, a demonstrated combined-tree failure, or an explicit lane-owner decision is
the trigger; the commit count alone is not.

## Check attribution

A failing check is evidence about a candidate only if it ran on that candidate and
does not fail without it. Neither half is settled by reading the red badge, and both
answers can be wrong in opposite directions.

1. **Did the run reach a verdict, and does that verdict still apply?** A superseded
   SHA does not by itself refute a failure. Two outcomes surface identically:

   - *cancelled or never completed* — a newer push aborted an in-flight run. This
     carries no verdict either way. Discard it as evidence; the run at the current
     head answers. Lanes that know they were cancelled usually say so in their logs;
   - *genuinely failed* — the run reached a real assertion, compile error, or command
     failure. Its SHA is superseded, but the finding stands until the seam it names
     actually changed. Carry it forward and revalidate it at the current head, or show
     that the later commits touched the failing seam. A documentation-only push does
     not refute a test failure.

   Read the run, not the badge. Only the first outcome makes the result stale.

2. **Does it reproduce without the candidate?** The comparison tree is the pull
   request's **merge base**, not current `main`: a candidate inherits what was broken
   where it branched, and `main` may have been repaired since. Two failures on two
   trees are not automatically the same failure, so assigning this one to the base
   requires all three:

   - the same check identity — that gate and job, not a locally approximated command;
   - the same failure signature — the same failing test, assertion, diagnostic, or
     path, since a command can exit nonzero on two trees for unrelated reasons;
   - that signature observed at the merge base.

   If the base fails a *different* test or error, the candidate still owns its own
   failure: preserve both findings rather than cancelling one as a reproduction. With
   any of the three missing the failure is `NOT_PROVEN`, which is not the same as
   base-owned. "It also fails on main" names the wrong tree, and on its own it can
   retire a regression the candidate introduced.

Each provider's `verify-live-ci` owns the integration-side procedure and its cheap
discriminators — merge-base ancestry
(`git merge-base --is-ancestor <repair> <pr-merge-base>`) and by-construction reasoning
over the full changed-path set. This section defines only what the two answers mean for
review currentness.

Both questions are cheap; both wrong answers are expensive. A false attribution to the
candidate sends an author to repair code that is not broken. A false attribution to the
base retires a real regression as somebody else's problem and leaves it unfiled.

A gate that fails on `main`, blocks nothing, and is labelled flaky at a 100% failure
rate is worse than a missing gate. A missing gate is visibly absent; this one looks
like coverage while carrying no signal, so genuine regressions land behind it
unnoticed. Treat a persistently red non-blocking gate as an open question about
whether it should be required, repaired, or explicitly marked advisory.

## Expected-head merge safety

At the instant of merge, use the current PR head SHA as compare-and-swap protection so
a branch cannot move between inspection and merge:

```text
gh pr merge <n> --squash --match-head-commit <current-head-sha>
```

This is merge race protection. It is not review currentness and does not justify
exact-head review comments, branch refreshes, or CI replay.

## Landed reconciliation

After squash merge, verify the landed effect on current `main`, update the controlling
issue and durable claims, preserve residual work, and clean the branch/worktree. The
future squash commit was not—and did not need to be—the formal review subject.
