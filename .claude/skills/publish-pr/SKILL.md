---
name: publish-pr
description: Publish one locally complete coherent candidate through a concise GitHub review index, ready by default, with draft reserved for a named remote-only proof or collaboration need.
user-invocable: false
---

# Publish PR

Verify candidate, branch, base, worktree, controlling issue, an equivalent active candidate, and any actual same-branch/worktree writer collision. Do not infer ownership from touched-file or nearby-symbol overlap.

Publish ready only when applicable focused/affected proof and negative protection are current; hardening, simplification, and local candidate review are complete; the worktree is clean; the issue/claim/contract are current; and Changie/changelog, support, migration, and release dispositions are complete or not applicable.

Otherwise return to `build-candidate`. Draft only for a named remote-only proof, real collaboration, early visible ownership, or protected integration experiment. Record that reason and its completion condition in the PR body.

For an existing draft, inspect the named condition. Once complete, recheck the entire ready threshold and explicitly mark the PR ready through Claude's native GitHub surface or `gh pr ready <n>`. Do not leave a completed draft in a repeating `DRAFT` state. If the threshold is no longer met, return to candidate repair rather than marking ready.

Use the PR body as a review index covering claim, issue, contract, production path, proof, hardening, simplification, deviations, limits, risk, rollback, and review locations.

## Enforcement status is part of the claim

When a candidate adds or changes a gate, check, linting check, ratchet, or policy, state in the body whether it is **required** or **advisory**, and resolve that against live protection rather than intent. A body implying a gate blocks merge when it runs advisory overstates the claim, and the overstatement survives the merge as documentation.

Read both enforcement systems. Classic branch protection and repository rulesets are independent and additive, so either one alone gives a confidently wrong answer, and a check may be required by one, the other, both, or neither. Inspect ruleset enforcement status, target refs, and bypass actors as well as classic branch protection. A gate that runs inside a composite or conditional job is required only to the extent its calling job reports it; a skipped job reports Success, while a workflow-level skip leaves a required check Pending.

Where a change is deliberately advisory first — a new ratchet needing a baseline, or a gate that cannot pass until something merges past it — say so and name the condition for promotion. Unenforced-by-design is an honest claim; unenforced-and-described-as-blocking is not.

## Routes

- `PR_PUBLISHED_READY` / `PR_RESUMED` → `address-review-comments`
- `DRAFT_FOR_NAMED_REASON` → run the named experiment or collaboration, then repeat this skill
- `DRAFT_REASON_COMPLETE` → recheck readiness, mark the PR ready natively, and return `PR_RESUMED`
- `CANDIDATE_NOT_COHERENT` / `LOCAL_PROOF_STALE` / `WORKTREE_DIRTY` → `build-candidate`
- `DUPLICATE_OR_WRITER_COLLISION` → reuse/resume or resolve the conflict
- `IDENTITY_NOT_PROVEN` → stop for identity repair
