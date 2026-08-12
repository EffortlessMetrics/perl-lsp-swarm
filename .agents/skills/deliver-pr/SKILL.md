---
name: deliver-pr
description: Run one coherent claim through its named Codex route in one persistent claim-local lane, preserving context across review, repair, proof, live integration, and closeout.
---

# Deliver PR

This is the lane-root flow for one coherent acceptance-and-rollback claim. Reconstruct
only that lane's issue, governing contract, proof, branch/worktree, candidate, PR,
substantive review, live integration, explicit prerequisites, and closeout state.

The lane root runs the route. It is not a stage-specific reviewer, repairer, proof
runner, or finisher. Keep the same lane context and worktree across useful transitions.
Invoke the next named skill from the current skill's result instead of returning an
intermediate packet so another agent can rediscover the claim.

Use `$orchestrate-work` for focused evidence lenses where useful. Those workers return
evidence to this lane; they do not replace the lane root or become rival candidates.
Keep one writer on the current candidate at a time.

Mentioning one issue or PR does not make the campaign root a leaf worker. A campaign
root normally delegates a substantial claim as one whole-flow `$deliver-pr` lane. A
lane root may perform claim-local review, mutation, proof, and integration directly
when its brief grants the required authority.

Before creating a candidate, check whether an equivalent current PR already implements
the same claim. Do not inspect sibling lanes, touched-file overlap, nearby symbols, or
unrelated worktrees as a routine ownership check.

## Lane continuity

The durable unit is the claim lane, not the current skill.

```text
review finds a candidate-owned defect
→ same lane `$address-review-comments` or `$build-candidate`
→ same lane affected proof
→ same lane affected `$final-challenge` and `$review-pr`

review is current
→ same lane `$verify-live-ci`

candidate-owned CI failure
→ same lane `$build-candidate`
→ same lane affected proof and review

integration is ready
→ same lane `$merge-reconcile` when authorized
```

Do not close, replace, or cold-start the lane merely to change from review to repair,
repair to proof, proof to review, or review to integration. A lane may change from
read-only review activity to candidate mutation when the accepted result and its parent
brief grant mutation authority. Focused child reviewers remain read-only.

When a remote-owned wait pauses the route, return `IN_FLIGHT` with the exact wake event.
The campaign root should resume this same lane when the runtime still retains it. When
resumption is impossible, GitHub and repository artifacts reconstruct the route; do not
invent a second candidate.

## Entry route

Enter at the earliest absent or stale useful judgment:

```text
concern, issue, owner, scope, or plan unsettled
→ `$prepare-issue`

intent settled, proof absent or weak
→ `$prepare-proof`

reviewed proof or implementation candidate needs completion
→ `$build-candidate`

publication-ready candidate or existing PR needs convergence
→ `$finish-pr`

merged or deliberately closed but unreconciled
→ `$merge-reconcile` through `$finish-pr`

claim already reconciled
→ return `RECONCILED`
```

Create or link a missing issue where it improves continuity, but do not replay completed
work merely to manufacture chronology.

## Transition contract

Every invoked skill owns its immediate procedure and returns a typed result. Continue in
this lane according to that skill's `Routes`, `Valid exits`, or equivalent next-step
table. The common transitions are:

| Result | Same-lane next action |
| --- | --- |
| `PROOF_READY` / `ALREADY_PROVEN` | `$build-candidate` |
| `CANDIDATE_READY` | publication/convergence through `$finish-pr` |
| `CHANGES_REQUIRED` / `REVIEW_FINDINGS_OPEN` | `$address-review-comments`; use `$build-candidate` for implementation work |
| `FINDINGS_REPAIRED_OR_DISPOSITIONED` | affected proof, `$final-challenge`, affected `$review-pr` |
| `WEAK_PROOF` / `PROOF_REVISE` | `$prepare-proof`, then resume the requesting route |
| `REVIEW_CURRENT` | `$verify-live-ci` |
| `PRODUCT_OR_TEST_FAILURE` | `$build-candidate`, affected proof, affected review |
| `CONFLICT` / `INTEGRATION_INTERACTION` | repair the affected seam, then affected proof and review |
| `INTEGRATION_READY` | `$merge-reconcile` when authorized |
| `PR_IN_FLIGHT` / `PENDING_REMOTE` | return `IN_FLIGHT` with the wake event |
| `BLOCKED_BY_PREREQUISITE` | return the exact prerequisite without taking unrelated ownership |
| `SUPERSEDED_OR_CLOSE` | `$merge-reconcile` when authorized, otherwise return durable closeout |
| `NOT_PROVEN` | resolve the named missing evidence when possible; otherwise return the exact boundary |

Do not stop after a review packet when the repair is bounded, candidate-owned, within
the accepted claim, and authorized. Do not treat formatting or `git diff --check` as
behavioral proof. Do not call a published repair solid while affected proof or review is
`NOT_PROVEN`.

## Run the route through claim-local orchestration

For each current transition:

1. Invoke the named public flow or atomic `$skill`.
2. Use `$orchestrate-work` only for independent evidence that can change the decision.
3. Require children to consume the named `$skill` when one is supplied.
4. Join compact evidence and contradictions; do not adopt a child verdict as approval.
5. Send accepted mutations through this lane's one candidate writer.
6. Run the smallest affected proof that can falsify the changed seam.
7. Publish useful durable facts at the native GitHub boundary.
8. Continue in this lane through the named next/backward route.
9. Return only at a real remote wait, terminal disposition, named prerequisite, durable
   hazard, external-action boundary, or precise `NOT_PROVEN` boundary.

A whole-flow lane may recursively orchestrate focused workers within this claim. Leaf
workers may not select unrelated work or widen into lane ownership unless their brief
explicitly grants that authority.

## Candidate and lane contract

One claim normally has one current candidate. One writer mutates its branch/worktree at
a time. Focused readers, external oracles, CI evidence workers, and differentiated
review lenses may assist without creating rival implementations.

This lane owns its integration work:

- behind-only movement on `main` requires no action;
- an actual Git conflict is resolved in this lane, normally by the later-landing lane;
- an explicit stacked prerequisite is retargeted after the prerequisite lands;
- a combined-tree semantic failure is repaired in the smallest affected candidate;
- only conflict- or interaction-affected proof and review are refreshed.

A worktree and branch are operational context, not an exact-head lease. Commit and push
normally without force. Re-read intervening content after a rejected push or another
material integration event. A head SHA change alone does not end the lane.

Use direct issue or PR comments for material cross-lane facts. Do not create
reservations, overlap ledgers, central lane state, or routine sibling-PR surveillance.

## Traceable intended route

When another context will need the route and it is not already obvious, publish one
compact declaration on the controlling issue or PR:

```text
Route
- Goal / parent: <umbrella or durable outcome>
- Claim: <one acceptance-and-rollback claim>
- Entry flow: `$deliver-pr`
- Current useful transition: <named skill or external wait>
- Why: <material missing judgment>
- Durable subject: <issue / PR / merged commit>
- Resume when: <material wake event, if any>
```

Update only when the material route changes. This is a resumability aid, not a stage
record, lease, or per-step status protocol.

## Useful GitHub boundaries

Publish a durable issue/PR comment, inline review, submitted review, or finding
disposition when the information:

- changes claim, authority, accepted plan, proof obligation, route, prerequisite,
  support, risk, or rollback meaning;
- is source-backed evidence another context would otherwise rediscover;
- is a localized review finding or evidence-backed disposition;
- records a real external wait and its wake event;
- provides a useful candidate-wide review, integration, merge, or closeout synthesis.

Keep agent identity, topology, liveness, retry order, provisional reasoning, raw logs,
unchanged polls, and routine skill transitions runtime-local. Do not write lane state to
a tracked file.

## Remote-owned waits

When review, CI, queue state, auto-merge, a platform, or another external transition
owns the next action:

- leave the coherent candidate in GitHub;
- record the exact remaining action and wake event once when useful;
- return `IN_FLIGHT` to the campaign root;
- let `$deliver-goal` advance another distinct claim;
- do not poll unchanged state;
- do not refresh the branch for unrelated `main` movement;
- do not call the claim blocked merely because it is in flight.

## Completion

Return `RECONCILED`, `IN_FLIGHT`, `PARTIAL`, `SUPERSEDED`, `BLOCKED`, or
`NOT_PROVEN`, naming what landed or remains, which evidence is current, the durable
issue/PR subject, the current/next skill or wake event, and cleanup of lane-created
worktrees/process groups when no longer needed.

## What this establishes

One persistent claim-local context follows a traceable provider-native route through
issue, proof, candidate, review, integration, merge, and closeout without paying a new
agent cold start at each skill boundary.

## What this does not establish

A repository scheduler, tracked frontier, stage-agent taxonomy, agent registry,
competing candidate set, overlap ledger, comment-per-transition protocol, or merge
authorization independent of `$review-pr`, live required checks, mergeability, rulesets,
and unresolved findings.
