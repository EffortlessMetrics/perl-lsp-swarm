---
name: publish-pr
description: Publish one candidate-bound locally proven coherent change through a concise GitHub review index, ready by default, with draft reserved for a named remote-only proof or collaboration need.
user-invocable: false
---

# Publish PR

Publish the current coherent candidate; do not use GitHub as the ordinary scratchpad for
unfinished local work.

Verify branch, worktree, candidate commit, base, controlling issue, claim, and governing
contract identity. Confirm no equivalent active PR or actual same-candidate writer
collision exists. Touched-file or nearby-symbol overlap alone is not ownership.

## Required local input

Consume the current `prove-before-push` packet for this exact candidate commit/range.
The packet must state:

- resolved base/head and change-set digest;
- selected and deferred affected-proof steps;
- commands/results and affected scope;
- Changie fragment/exemption identity and dry-render result;
- diff-scoped RIPR and review-guidance disposition, or an exact local `NOT_PROVEN` /
  named remote-only boundary owned by #7365;
- proof deliberately not run and why;
- one result class and next route.

Do not reconstruct this packet from raw logs or attach a result from another candidate
head. Formatting, `git diff --check`, a green focused test, or a clean worktree alone do
not satisfy the candidate-bound result.

## Ready-publication threshold

A candidate publishes ready only when:

- `prove-before-push` returned `LOCAL_CANDIDATE_PROVEN` for the current candidate;
- focused and selected affected proof passes;
- relevant negative, stale, failure, refusal, and recovery protection exists;
- applicable local diff-scoped RIPR and Changie obligations are current;
- test hardening, simplification, and mutable local candidate review are complete;
- the worktree contains no accidental or unsalvaged changes;
- the controlling issue, claim boundary, governing contract, prerequisites, and
  deviations are current;
- the candidate is one coherent acceptance-and-rollback claim;
- deferred and remote-only proof are stated without being represented as already run.

If this threshold is not met, follow the `prove-before-push` result:

- product/test failure or RIPR gap → `build-candidate` / `improve-test-suite`;
- weak proof → `prepare-proof`;
- material premise change → `prepare-issue`;
- instrument `NOT_PROVEN` → repair/bootstrap the instrument or preserve the boundary;
- incoherent/dirty candidate → `build-candidate`.

Do not open a churn-producing ready PR merely to obtain ordinary proof that belongs
locally.

## Draft exceptions and transition

Draft publication is allowed only when it buys one concrete capability:

- `prove-before-push` returned `REMOTE_ONLY_PROOF_REQUIRED` and named the platform,
  packaged artifact, external service, clean-environment behavior, or protected
  integration fact unavailable locally;
- real branch collaboration is required;
- early visible ownership prevents duplicate substantial work;
- a protected integration experiment is itself the subject.

A local `INSTRUMENT_NOT_PROVEN` result is not automatically remote-only. The body must
name why the local instrument cannot establish the fact, which GitHub workflow/check
will, and what exact result completes the draft purpose.

For an existing draft, inspect that named condition. Once complete, re-run any local
proof invalidated by intervening candidate changes, recheck the entire ready threshold,
and explicitly mark the PR ready through Claude's native GitHub surface or
`gh pr ready <n>`. Do not leave a completed draft in a repeating `DRAFT` state.

## PR review index

Use the PR body as an index into durable state, not a copy of the issue or raw logs:

```markdown
## Claim
## Controlling issue
## Governing contract / spec
## Changed production path
## Local candidate proof
- base/head + change-set digest
- selected/deferred proof
- tests/oracles and negative controls
- RIPR disposition
- Changie disposition
## Test hardening
## Simplification
## Deviations
## What this establishes
## What this does not establish
## Remote-only proof still required
## Risk and rollback
## Review index
```

Do not claim hosted proof, formal review, or merge readiness before current GitHub
evidence exists. Link stable receipts/artifacts rather than copying high-volume output.

## Enforcement status is part of the claim

When a candidate adds or changes a gate, check, ratchet, or policy, state whether it is
required or advisory, resolved against live classic branch protection and repository
rulesets. They are independent and additive. Inspect enforcement status, target refs,
bypass actors, and the reporting job; do not infer blocking authority from the workflow
name or intent.

Where a change is deliberately advisory first, say so and name the promotion condition.
Unenforced-by-design is honest; unenforced-and-described-as-blocking is not.

## A published branch still has one writer

Publishing does not release the candidate. The branch keeps one writer until the claim
merges or is deliberately closed. Reviewers request changes; a reviewer may be promoted
in place only through the claim's one-writer boundary.

If another actor has already pushed, read and verify what landed before adopting or
replacing it. Treat a material reviewer-applied repair as new authored candidate state:
run affected local proof and refresh affected review dimensions. Do not force-push over
compatible work or rebuild a PR merely because the head changed.

When a closed PR can be reopened with its branches intact, reopen it and preserve the
review record. Create a fresh PR only when the old one cannot represent the rebuilt
candidate; name what it supersedes and carry forward only verified findings.

## Routes

- `PR_PUBLISHED_READY` / `PR_RESUMED` → `finish-pr`, entering at current findings or
  cumulative review rather than replaying publication
- `DRAFT_FOR_NAMED_REASON` → run the named remote experiment/collaboration, then repeat
  this skill when its wake event occurs
- `DRAFT_REASON_COMPLETE` → recheck candidate-bound proof and readiness, mark ready
  natively, and return `PR_RESUMED`
- `CANDIDATE_PRODUCT_OR_TEST_FAILURE` / `RIPR_GAP_REQUIRES_REPAIR` /
  `CANDIDATE_NOT_COHERENT` / `WORKTREE_DIRTY` → `build-candidate`
- `WEAK_OR_CIRCULAR_PROOF` / `LOCAL_PROOF_STALE` → `prepare-proof` or repeat
  `prove-before-push` as appropriate
- `RETURN_TO_ISSUE` → `prepare-issue`
- `INSTRUMENT_NOT_PROVEN` / `IDENTITY_NOT_PROVEN` → repair the named input/instrument or
  preserve `NOT_PROVEN`; do not publish ready
- `DUPLICATE_OR_WRITER_COLLISION` → reuse/resume the equivalent candidate or resolve the
  actual same-candidate collision
