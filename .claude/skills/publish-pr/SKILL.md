---
name: publish-pr
description: Publish one locally complete coherent candidate through a concise GitHub review index, ready by default, with draft reserved for a named remote-only proof or collaboration need.
user-invocable: false
---

# Publish PR

Verify candidate, branch, base, worktree, controlling issue, an equivalent active candidate, and any actual same-branch/worktree writer collision. Do not infer ownership from touched-file or nearby-symbol overlap.

Publish ready only when applicable focused/affected proof and negative protection are current; hardening, simplification, and local candidate review are complete; the worktree is clean; the published file set matches the intended worktree change set — compare `gh pr diff --name-only <n>` or the created PR's file list against the intended changed paths, because a squash or partial stage must not silently drop an intended file; the branch head passes `cargo fmt -p <pkg> -- --check` and its merge-base result is classified — the head check is binding, the base check diagnostic, naming inherited base-redness at publish time so a drift-repair candidate whose base is red by construction still publishes; the issue/claim/contract are current; and Changie/changelog, support, migration, and release dispositions are complete or not applicable.

Otherwise return to `build-candidate`. Draft only for a named remote-only proof, real collaboration, early visible ownership, or protected integration experiment. Record that reason and its completion condition in the PR body.

For an existing draft, inspect the named condition. Once complete, recheck the entire ready threshold and explicitly mark the PR ready through Claude's native GitHub surface or `gh pr ready <n>`. Do not leave a completed draft in a repeating `DRAFT` state. If the threshold is no longer met, return to candidate repair rather than marking ready.

## PR review index

A mechanical candidate — generated regeneration, lint-site collapse, allowlist row
removal, or comment-only edits — may publish a reduced index of **Claim**, **Proof**,
and **Non-goals**. The full index remains the default for anything crossing a
production seam.

The order is load-bearing: establish claim and authority before proof; trace the changed
production path; record focused and affected proof with `pass` / `fail` / `not-run` /
`NOT_PROVEN`; challenge a realistic wrong implementation with negative, stale,
failure, recovery, or opposite-direction controls; simplify before publication; bound
the claim and non-goals; then name risk, rollback, and review locations.

```markdown
## Claim
## Controlling issue
## Governing contract
## Changed production path
## Proof
## Test hardening
## Simplification
## Deviations
## Claim Boundary
## Non-goals
## Risk and rollback
## Review index
```

Proportionality mirrors `review-pr`'s carve-out: a candidate whose cumulative diff is mechanical — generated regeneration, lint-site collapse, allowlist row removal, comment-only edits — may publish a reduced index of three sections: **Claim**, **Proof**, and **What this does not establish**. The full index remains the default for anything crossing a production seam.

## Enforcement status is part of the claim

When a candidate adds or changes a gate, check, linting check, ratchet, or policy, state in the body whether it is **required** or **advisory**, and resolve that against live protection rather than intent. A body implying a gate blocks merge when it runs advisory overstates the claim, and the overstatement survives the merge as documentation.

Read both enforcement systems. Classic branch protection and repository rulesets are independent and additive, so either one alone gives a confidently wrong answer, and a check may be required by one, the other, both, or neither. Inspect ruleset enforcement status, target refs, and bypass actors as well as classic branch protection. A gate that runs inside a composite or conditional job is required only to the extent its calling job reports it; a skipped job reports Success, while a workflow-level skip leaves a required check Pending.

Where a change is deliberately advisory first — a new ratchet needing a baseline, or a gate that cannot pass until something merges past it — say so and name the condition for promotion. Unenforced-by-design is an honest claim; unenforced-and-described-as-blocking is not.

## A published branch still has one writer

Publishing does not release the candidate. The branch keeps exactly one writer until
the claim merges or is deliberately closed, and a reviewer who wants a change requests
it rather than pushing it.

A second writer pushing to a published branch produces failures that look like the
author's: the push can land a change that was never proven, it can be silently absorbed
by the author's next force-push, and the author's local head and the PR head diverge
without either party noticing. All three are expensive precisely because the branch
still looks like one coherent candidate.

If a reviewer has already pushed, do not race it. Read what landed, verify it against
observed behavior rather than assuming it is correct, and either adopt it — restating
the proof, since a reviewer's push carries none — or replace it and say why in the
thread. Step 8 of `address-review-comments` covers the same case: a reviewer-applied
repair makes a new authored candidate whose affected review dimensions are invalid.

Recreating a closed PR is a different matter, and the first move is to try reopening
it. A closed PR whose head and base branches both still exist normally reopens even
after the head was force-pushed or rewritten, and reopening preserves the review record
— findings, dispositions, and the conversation — that a fresh PR discards. Open a new PR
only when reopening is actually refused: the head or base branch was deleted, the head
was rebuilt as an unrelated branch, or GitHub rejects the transition for its own
reasons. Then name what it supersedes and carry forward only findings still verified
against the new candidate. Prefer not rebuilding at all — see the currentness contract.

## Routes

- `PR_PUBLISHED_READY` / `PR_RESUMED` → `address-review-comments`
- `DRAFT_FOR_NAMED_REASON` → run the named experiment or collaboration, then repeat this skill
- `DRAFT_REASON_COMPLETE` → recheck readiness, mark the PR ready natively, and return `PR_RESUMED`
- `CANDIDATE_NOT_COHERENT` / `LOCAL_PROOF_STALE` / `WORKTREE_DIRTY` → `build-candidate`
- `DUPLICATE_OR_WRITER_COLLISION` → reuse/resume or resolve the conflict
- `IDENTITY_NOT_PROVEN` → stop for identity repair
