# Development method

## Purpose

This repository uses a shift-left, review-forward development loop so capable agents can advance work autonomously without turning process into a permission ladder.

The useful passes are controls. Permanent personas, lifecycle labels, completion hooks, exact-head review receipts, and tracked stage state are not controls.

```text
current artifact
→ focused question
→ evidence-backed improvement or falsification
→ locally named next skill or material backward route
```

## Governing law

**Default-complete, recovery-forward.**

For substantive work, normally perform every applicable research, vision, planning, proof, hardening, simplification, review, and reconciliation pass before creating the next more expensive artifact.

When an earlier pass was missed, perform the cheapest version that can still improve the current artifact and continue. Do not discard coherent work or replay history merely to manufacture process evidence.

## Shift-left versus handcuffs

Shift-left places challenge immediately before the decision it can improve:

```text
concern
→ issue and source research
→ premise and vision challenge
→ current plan
→ proof design and oracle challenge
→ implementation
→ test hardening and simplification
→ candidate challenge
→ GitHub feedback repair
→ proportional review
→ integration and reconciliation
```

A handcuff notices later that an earlier ritual was not recorded and blocks work after most of the economic value has already been lost.

Missing labels, old stage receipts, named-agent handoffs, exact-head review comments, or arbitrary plan age are not reasons to stop. Reconstruct truth from current GitHub and repository artifacts, repair what still matters, and proceed.

## Public flows

| Flow | Outcome |
| --- | --- |
| `deliver-goal` | Advance a durable multi-PR outcome through distinct coherent claims |
| `deliver-pr` | Carry one acceptance-and-rollback claim and its current candidate through reconciliation |
| `prepare-issue` | Research, challenge, vision-check, and plan the concern |
| `prepare-proof` | Turn settled intent into discriminating executable proof |
| `build-candidate` | Implement, harden tests, simplify, and challenge the candidate |
| `finish-pr` | Publish or resume, repair feedback, review, merge, and reconcile |

A fresh or resumed session starts with the narrowest applicable public flow. Once inside the loop, each skill names its normal successor and material backward routes. Do not run a lifecycle locator between skills.

## Runtime orchestration

The root chooses the smallest useful execution shape.

- tiny, tightly coupled work often stays direct;
- substantive work may use bounded specialists or read-only questions;
- one whole coherent claim may be carried by one `deliver-pr` lane;
- genuinely independent claims may use separate writer lanes.

The root remains accountable for goal meaning, claim selection, authority, contradiction resolution, joined evidence, review sufficiency, merge judgment, and reconciliation.

Delegation is useful when it changes the source, oracle, environment, threat model, or review method; compresses high-output evidence; preserves root context; or reduces elapsed time. The brief names target, authority, mutation boundary, sufficient result, falsifiers, stop conditions, and non-goals.

## Claim and lane independence

```text
one coherent claim
→ one current candidate
→ one branch/worktree
→ one writer mutating that candidate at a time
→ one PR
```

Several distinct claims may be active. That does not require a frontier database, overlap map, file reservation, executor graph, or sibling-lane monitor.

Before creating a candidate, check only for an equivalent current PR and explicit prerequisites. Do not inspect neighbouring worktrees or touched-file overlap merely to predict coordination.

Each lane owns its own proof, review repair, and integration cleanup:

- if another PR lands and the candidate remains valid, do nothing;
- behind-only movement on `main` requires no action;
- an actual Git conflict is resolved by the affected lane, normally the one landing later;
- an explicit stack is retargeted after its prerequisite lands;
- an actual combined-tree failure is repaired in the smallest affected candidate;
- only interaction-affected proof and review are refreshed.

When a coherent PR waits on CI, review, queue state, or auto-merge, leave it in GitHub and advance another distinct claim when useful. A remote-owned wait is not a goal blocker.

## Durable state and runtime state

Repository and GitHub artifacts hold durable truth:

- issues hold the problem, research, corrections, current synthesis, plan, dependencies, and next action;
- specifications, ADRs, policies, and tests hold accepted contracts and proof obligations;
- branches and worktrees hold one candidate's mutation;
- pull requests hold one coherent acceptance-and-rollback candidate;
- submitted reviews, threads, checks, rulesets, and mergeability hold current integration evidence;
- merge closeout records what landed and what remains.

Runtime topology, task lists, liveness, retries, raw logs, and provisional reasoning remain ephemeral. Do not mirror them into GitHub.

## Issue-first as a paved road

The normal fresh path is:

```text
targeted issue and PR search
→ reuse, reconcile, or create a lightweight issue
→ research current source and external authority
→ challenge premise, ownership, scope, and vision
→ maintain one current issue synthesis and plan
→ continue immediately
```

When implementation already exists without an issue, link or create the issue where it improves continuity. Do not pretend retrospective filing shifted the implementation left.

## Vision checks

### Issue and plan mode

Ask whether the work should exist and whether the proposed shape advances the product:

- Is this a real user or repository problem?
- Is it solved, duplicated, or superseded?
- Is the semantic owner and consumer correct?
- Is the claim coherent and proportionate?
- Is there a simpler path?

### Candidate and PR mode

Ask whether the implementation still serves that vision:

- Did implementation choices alter product meaning?
- Did the candidate broaden or narrow the claim?
- Did it create duplicate authority or an unreachable route?
- Is the complexity justified?
- Does the user-visible path reach the changed behavior?

## Proof

Executable proof must discriminate realistic wrong implementations. A green test that mirrors the implementation is not sufficient.

After implementation, revisit:

- what realistic incorrect implementation still passes;
- whether the oracle is independent and non-vacuous;
- whether the opposite direction is represented;
- whether the production seam is exercised;
- whether the proof runs at the cheapest effective layer.

Never weaken a test, ratchet, support claim, or required proof merely to obtain green status.

## Review

Review is directed, falsifying, and verified. Reading a diff, relaying green CI, posting a head/claim hash, or repeating a subagent verdict is not review.

Where applicable, establish:

- claim honesty;
- semantic and external correctness;
- proof discrimination;
- production-path reachability;
- negative and fallback behavior;
- authority and complexity;
- compatibility and rollback;
- remaining uncertainty.

A clean review is valid. Do not manufacture a finding, edit, second identity, or receipt to make review visible.

### Review-forward currentness

Review is cumulative and semantic. The durable record is the submitted review, inline findings, replies, evidence-backed dispositions, and any focused follow-up on repaired seams.

A later commit does not invalidate review merely because the SHA changed.

After a repair:

```text
identify changed semantic subjects
→ rerun affected proof
→ verify addressed findings
→ review newly changed claim/risk dimensions
→ continue
```

Examples:

- formatting or editorial cleanup → no review refresh unless meaning changed;
- generated receipt refresh → verify generator/input relation only;
- stronger tests → review proof implications only;
- finding repair → review the finding, proof, and changed seam;
- material claim, production route, authority, security, compatibility, packaging, migration, support, or rollback change → review the affected dimensions;
- conflict/integration repair → review the repaired interaction.

Do not restart a full `deep` review after every push. Do not post `Review pass (...) at head ... and claim ...` comments.

The current head SHA is still useful to identify current code and required check results. At merge time it may be used as compare-and-swap protection. That is merge safety, not review currentness.

## Merge boundary

Merge eligibility is determined by current GitHub branch protection, rulesets, required checks, unresolved substantive threads, current change requests, actual mergeability, and any required combined-tree proof.

Behind-only movement does not require a rebase or review replay. If the head moves immediately before merge, re-read live state and refresh only evidence/review affected by the new commit.

After squash merge, reconciliation verifies the landed effect on current `main`.

## Hard stops

Stop only where a concrete hazard or unresolved authority remains:

- two writers would mutate the same candidate branch/worktree concurrently;
- destructive cleanup would lose unsalvaged work;
- repository, branch, or candidate identity cannot be established;
- a secret or unsafe release would be published;
- a durable contract is structurally invalid;
- substantive review findings remain unresolved;
- current GitHub branch protection, rulesets, merge queue, or required checks block merge;
- a material product or semantic decision belongs to the accountable owner.

Everything else normally follows:

```text
detect
→ explain
→ repair
→ continue
```
