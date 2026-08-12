---
name: deliver-pr
description: Run one coherent claim through its named Claude route in one persistent claim-local lane, preserving context across issue, proof, build, candidate-bound local proof, review, live integration, and closeout.
argument-hint: "[issue, PR, branch, or claim]"
---

# Deliver PR

This is the lane-root flow for one coherent acceptance-and-rollback claim. Reconstruct
only that lane's issue, governing contract, proof, branch/worktree, candidate, local
candidate result, PR, substantive review, live integration, explicit prerequisites, and
closeout state.

The lane root runs the route. It is not a stage-specific reviewer, repairer, proof
runner, or finisher. Keep the same lane context and worktree across useful transitions.
Invoke the next named skill from the current skill's result instead of returning an
intermediate packet so another subagent can rediscover the claim.

A campaign or claim orchestrator ingests `change-graph` once before entering this flow.
Do not reload the whole graph at every transition. Use `orchestrate-work` for focused
independent evidence where useful; those workers return evidence to this lane and do not
replace the lane root or become rival candidates. Keep one writer on the current
candidate at a time.

Mentioning one issue or PR does not make the campaign root a leaf worker. A campaign
root normally delegates a substantial claim as one whole-flow `deliver-pr` lane. A
lane root may perform claim-local review, mutation, proof, and integration directly
when its brief grants the required authority.

Before creating a candidate, check whether an equivalent current PR already implements
the same claim. Do not inspect sibling lanes, touched-file overlap, nearby symbols, or
unrelated worktrees as a routine ownership check.

## Lane continuity

The durable unit is the claim lane, not the current skill.

```text
candidate becomes coherent
→ same lane `prove-before-push`
→ same lane `publish-pr`

review finds a candidate-owned defect
→ same lane `address-review-comments` or `build-candidate`
→ same lane affected proof / `prove-before-push` where publication state changed
→ same lane affected `final-challenge` and `review-pr`

review is current
→ same lane `verify-live-ci`

candidate-owned CI failure
→ same lane `build-candidate`
→ same lane affected proof and review

integration is ready
→ same lane `merge-reconcile` when authorized
```

Do not close, replace, or cold-start the lane merely to change from issue to proof,
review to repair, repair to local proof, local proof to publication, proof to review, or
review to integration. A lane may change from read-only activity to candidate mutation
when the accepted result and parent brief grant mutation authority. Focused child
reviewers remain read-only unless explicitly promoted inside their same context.

When a remote-owned wait pauses the route, return `IN_FLIGHT` with the exact wake event.
The campaign root should resume this same lane when the runtime still retains it. When
resumption is impossible, GitHub and repository artifacts reconstruct the route; do not
invent a second candidate.

## Entry route

Enter at the earliest absent or stale useful judgment:

```text
concern, issue, owner, scope, acceptance, or plan unsettled
→ `prepare-issue`

intent settled, proof absent or weak
→ `prepare-proof`

reviewed proof or implementation candidate needs completion
→ `build-candidate`

coherent un-published candidate needs candidate-bound local proof
→ `prove-before-push`

locally proven candidate needs publication
→ `publish-pr`

existing PR needs convergence
→ `finish-pr`

merged or deliberately closed but unreconciled
→ `merge-reconcile` through `finish-pr`

claim already reconciled
→ return `RECONCILED`
```

Create or link a missing issue where it improves continuity, but do not replay completed
work merely to manufacture chronology.

## Transition contract

Every invoked skill owns its immediate procedure and returns a typed result. Continue in
this lane according to that skill's routes, valid exits, or equivalent next-step table.

| Result | Same-lane next action |
| --- | --- |
| `PROOF_READY` / `ALREADY_PROVEN` | `build-candidate` |
| `CANDIDATE_READY` | `prove-before-push` |
| `LOCAL_CANDIDATE_PROVEN` | `publish-pr` |
| `REMOTE_ONLY_PROOF_REQUIRED` | `publish-pr` only through an explicit draft/remote-proof boundary |
| `CANDIDATE_PRODUCT_OR_TEST_FAILURE` | `build-candidate`, then repeat `prove-before-push` |
| `RIPR_GAP_REQUIRES_REPAIR` | `improve-test-suite` or `build-candidate`, then repeat affected proof |
| `WEAK_OR_CIRCULAR_PROOF` | `prepare-proof`, then resume `build-candidate` |
| `INSTRUMENT_NOT_PROVEN` | repair/bootstrap the named instrument or preserve the exact boundary |
| `PR_PUBLISHED_READY` / `PR_RESUMED` | `finish-pr` at current findings/review state |
| `CHANGES_REQUIRED` / `REVIEW_FINDINGS_OPEN` | `address-review-comments`; use `build-candidate` for implementation work |
| `FINDINGS_REPAIRED_OR_DISPOSITIONED` | affected proof, `final-challenge`, affected `review-pr` |
| `WEAK_PROOF` / `PROOF_REVISE` | `prepare-proof`, then resume the requesting route |
| `REVIEW_CURRENT` | `verify-live-ci` |
| `PRODUCT_OR_TEST_FAILURE` | `build-candidate`, affected proof, affected review |
| `CONFLICT` / `INTEGRATION_INTERACTION` | repair the affected seam, then affected proof and review |
| `INTEGRATION_READY` | `merge-reconcile` when authorized |
| `PR_IN_FLIGHT` / `PENDING_REMOTE` | return `IN_FLIGHT` with the wake event |
| `BLOCKED_BY_PREREQUISITE` | return the exact prerequisite without taking unrelated ownership |
| `SUPERSEDED_OR_CLOSE` | `merge-reconcile` when authorized, otherwise return durable closeout |
| `RETURN_TO_ISSUE` / material premise change | `prepare-issue` |
| `NOT_PROVEN` | resolve the named missing evidence when possible; otherwise return the exact boundary |

Do not stop after a review or proof packet when the next action is bounded,
candidate-owned, within the accepted claim, and authorized. Do not treat formatting or
`git diff --check` as behavioral proof. Do not call a published repair solid while
affected proof, local candidate result, or review is `NOT_PROVEN`.

## Run the route through claim-local orchestration

For each current transition:

1. Invoke the named public flow or atomic skill.
2. Use `orchestrate-work` only for independent evidence that can change the decision.
3. Require children to consume the named skill when one is supplied.
4. Join compact evidence and contradictions; do not adopt a child verdict as approval.
5. Send accepted mutations through this lane's one candidate writer.
6. Encode durable issue/spec/proof/Changie/PR state at the first boundary where another
   competent context would otherwise need to rediscover it.
7. After each coherent candidate commit, run the smallest affected proof and
   `prove-before-push` before ordinary publication or republishing a material repair.
8. Publish useful durable facts at the native GitHub boundary; keep runtime topology and
   ordinary transitions local.
9. Continue in this lane through the named next/backward route.
10. Return only at a real remote wait, terminal disposition, named prerequisite, durable
    hazard, external-action boundary, or precise `NOT_PROVEN` boundary.

A whole-flow lane may recursively orchestrate focused subagents, context forks,
Ultracode, or an Agent Team within this claim. Use Agent Teams only when lateral
communication changes the result. Leaf workers may not select unrelated work or widen
into lane ownership unless their brief explicitly grants that authority.

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

## Durable state boundary

Follow `change-graph`:

- issue body/comments own the current problem, research, plan, decisions, and
  prerequisites;
- `.spec/`, ADR, policy, schema, or contract owns settled cross-PR/public invariants;
- tests, fixtures, and oracles own executable discrimination;
- `.changes/unreleased/` owns user-visible disposition while context is fresh;
- the local candidate packet owns exact committed-range affected proof, Changie, RIPR,
  and limitations;
- the PR body owns the cumulative candidate review index;
- review threads/submitted review own findings, dispositions, and cumulative judgment;
- checks own current-head remote integration facts;
- merge/issue closeout owns landed effect and residual work.

Keep subagent identity, topology, Teams/Ultracode state, liveness, retry order,
proof-token allocation, provisional reasoning, raw logs, unchanged polls, and routine
skill transitions runtime-local.

## Remote-owned waits

When review, CI, queue state, auto-merge, a platform, or another external transition
owns the next action:

- leave the coherent candidate in GitHub;
- record the exact remaining action and wake event once when useful;
- return `IN_FLIGHT` to the campaign root;
- let `deliver-goal` advance another distinct claim;
- do not poll unchanged state;
- do not refresh the branch for unrelated `main` movement;
- do not call the claim blocked merely because it is in flight.

## Completion

Return `RECONCILED`, `IN_FLIGHT`, `PARTIAL`, `SUPERSEDED`, `BLOCKED`, or
`NOT_PROVEN`, naming what landed or remains, which evidence is current, durable state
written or deliberately omitted, the issue/PR subject, current/next skill or wake event,
and cleanup of lane-created resources when no longer needed.

## What this establishes

One persistent claim-local context follows a traceable route through issue, proof,
candidate, local candidate verification, publication, review, integration, merge, and
closeout without paying a new subagent cold start at each skill boundary.

## What this does not establish

A repository scheduler, tracked frontier, stage-agent taxonomy, competing candidate
set, overlap ledger, comment-per-transition protocol, or merge authorization independent
of `review-pr`, live required checks, mergeability, rulesets, and unresolved findings.
