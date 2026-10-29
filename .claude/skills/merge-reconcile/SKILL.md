---
name: merge-reconcile
description: Reconstruct verified native queue custody and reconcile protected landing or deliberate closure without duplicate merge calls or proof replay.
user-invocable: false
---

# Merge and reconcile

This skill reconciles native protected landing for one root-held claim frame. It may
be invoked directly, so reconstruct the actual PR state and current authority rather
than assuming an earlier skill ran. Conditional landing authorization belongs before
enqueue in the existing `verify-live-ci` route; this skill makes no direct merge call.

No tracked review receipt, state file, claim digest, private task record, or agent
identity is authority. Use current GitHub reviews, complete feedback and dispositions,
checks/rulesets, actual native custody, candidate meaning and repository evidence.

## State branch

Inspect the live PR state first.

- `MERGED` → read the durable PR/commit state and reconcile actual M/current main.
- `CLOSED_UNMERGED` with an evidence-backed close/supersede disposition → reconcile
  within that disposition; closure is not landing.
- `CLOSED_UNMERGED` without a durable disposition → `NOT_PROVEN`.
- `OPEN` → reconstruct local proof, substantive review, conditional authorization and
  actual native entry/group/run/attempt through `verify-live-ci`. Missing verified native
  capability is `NOT_PROVEN`; do not invoke a legacy direct-merge fallback.
- unknown or partial state → `NOT_PROVEN`.

For open green G, retain native landing ownership and inspect the existing transition.
Green alone is not merged. Do not issue a duplicate merge, mandatory post-green
approval, branch refresh or ordinary CI replay. Landed/closed state enters readback
reconciliation. Every write still obeys the no-CI event boundary in `publish-pr`.

## Review and authorization predecessor

Read complete paginated feedback with `scripts/reviews/feedback <pr> <owner/repo>`:
ordinary issue comments, full review bodies including substantive `COMMENTED` concerns,
cumulative submitted reviews, localized findings and evidence-backed dispositions.
Retain terminal `fixed`, `refuted`, `superseded` and `post-merge-follow-up` separately
from open `current-blocker`, `blocked-by-prerequisite` and `not-proven` findings.

Green checks, zero threads, bot approval or author self-certification cannot create
`REVIEW_CURRENT`. A resolved thread without a visible disposition is not convergence.
Preserve unaffected semantic review and its original subject; refresh only dimensions
changed by claim, production route, authority, proof, risk, compatibility or actual
integration interaction. Use `check-pr-semantic-review-currentness.py --expected-head`
for the observed H; missing or stale semantic binding is `NOT_PROVEN`.

Before enqueue, the accountable root (the main Claude thread) assesses accepted product vision and current
repository fit and records conditional landing authorization for exact H under
standing deliver-through-merge authority. Enqueue can lead to automatic protected
landing. No separate mandatory final judgment follows green G. A bounded reviewer
does not acquire integration authority solely from its role.

- `REVIEW_REQUIRED` → `finish-pr` / `review-pr` for the missing useful judgment.
- `CHANGES_REQUIRED` → `address-review-comments`, one writer, affected proof and affected review.
- `BLOCKED_BY_PREREQUISITE` → preserve the actual prerequisite and wake event.
- `NOT_PROVEN` → resolve the missing evidence or capability; do not bypass protection.
- `SUPERSEDED_OR_CLOSE` → reconcile the evidence-backed disposition.
- `REVIEW_CURRENT` → consume `verify-live-ci` and the verified native route.

## Native integration evidence

Reconstruct actual expected H, integration base and ordered constituents, G, required
producer/app identities and the matching run/attempt. Read the union of applicable
branch rulesets AND classic branch protection; inability to read one is not proof that
it is absent. Retain every mandatory selected result and its actual execution.
Skipped, missing, stale, empty, cross-attempt, cancelled and instrument-failed evidence
is not passing qualification. Old PR synthetic merge P does not qualify actual G.

The live policy requires thread resolution: zero unresolved review threads is a
precondition of protected integration. Enumerate complete threads with
`scripts/reviews/threads <pr> --unresolved-only --json`; bot and outdated threads count.
Any open substantive finding or unresolved thread is `MERGE_BLOCKED`, routed through
`address-review-comments`. A material adverse finding on a queued candidate must withdraw
eligibility through the supported native/trusted controls; a prose comment alone is
insufficient. If that operation is unavailable, report the exact action boundary and
preserve the finding instead of claiming withdrawal happened.

`PR_IN_FLIGHT` requires actual matching native custody, not an API acknowledgment,
ordinary PR run or runner-capacity wait. Reconstruct native state after a lost response
before retrying. H2 cannot inherit H admission. The expected-head guard protects H;
it does not freeze base/constituents or prove G. No guessed command is supplied here.

G failure/ejection returns to the affected repair or instrument/capacity owner.
An open green G remains under native landing ownership until actual M is observed.
`INTEGRATION_READY` does not authorize this skill to make another merge call.
Retain the useful warm owner through terminal inspection and read-back; a genuine
interruption gets exact subject/run/attempt, limitations, next action and wake event.
Do not create an idle polling child or automatically change the selected model.

## Reconciliation

### Compare the reviewed claim with the landed effect

Establish what each comparison answers before treating a difference as a blocker.
The reviewed PR delta and the actual merge-parent delta have different bases:

```text
git diff <review-base> <reviewed-head> -- <claim-paths>
git rev-parse <merge>^1
git diff <merge>^1 <merge> -- <claim-paths>
```

Use the review's comparison base for the first command. The second command selects
one parent explicitly, including for a normal multi-parent merge. Verify through
commit/PR history that this first parent is the target branch's pre-merge mainline
before interpreting the third command; do not substitute the topic parent or all
parents as the comparison base. A squash merge inherits main's intervening contributions; it
does not need whole-file equality with the reviewed branch. Inspect the landed claim,
including reviewed behavior that was already present in the merge parent. If current
main moved again, distinguish that later delta from the merge's own effect.

A reviewed-head-to-merge comparison can locate differences, but cannot by itself
classify them. Attribute them through the actual parent and commit/PR history. For
example, an independent complementary test already landed on main is an inherited
contribution, not unexplained local mutation. Reconcile it read-only without an
unrelated rebase or CI replay. Provenance does not prove semantic independence:
investigate an interacting or unexplained behavioral difference and obtain affected
proof/review before claiming the landed result satisfies the reviewed claim.

### Resolve ordinary engineering questions under existing authority

Investigate comparison failures, trace provenance, and resolve ordinary engineering
questions within the authorized lane. An invalid equality assumption or an
unexecuted investigation is not a reason to request permission again. Ask the user
only for a decision outside existing authority or necessary information that cannot
be obtained through the available evidence. Do not invent another approval gate.

This does not relax existing stop/ownership rules for unexpected local uncommitted
edits, competing writers, destructive actions, or unsalvaged work. Source-backed
inherited-main contributions are expected integration differences, not those local
mutation hazards.

Keep an external wait scoped to its claim and name the wake event. When another
authorized claim can progress, return to `deliver-goal` and continue it. Do not mark
the umbrella blocked because one claim is waiting, an investigation has not run, or
a permission question was repeated. A goal-level blocker needs an evidenced external
dependency and no remaining useful authorized action, subject to the applicable goal
status rules.

After merge or evidence-backed deliberate closure:

1. verify the landed/current-main effect where applicable;
2. update or close the controlling issue accurately;
3. keep umbrella goals open when only one predicate landed;
4. update durable contracts, proof, support claims, and changelog only within the
   proven boundary;
5. preserve partial or residual work explicitly;
6. release the claim's worktree through the allocator/main thread that owns it;
7. update the root-held claim frame and expose the next coherent claim to `deliver-goal`.

Release local mutation resources on every terminal outcome—merged, superseded,
deliberately closed, or abandoned—not only the merge path. An abandoned claim reports
its typed `ABANDONED`/`EXTERNAL_BLOCKER`/`NOT_PROVEN` result to the main thread; there is
no subordinate orchestrator that must remain alive to represent it.

Release belongs to whoever allocated/controls the worktree, not to the writer standing
inside it. Keep a worktree only when it holds state that exists nowhere else:
uncommitted changes, unpushed commits, or detached useful state outside the base. A
fully pushed open PR is reconstructable with `git worktree add`.

Use `bash scripts/cleanup-completed-worktrees.sh --dry-run` to classify safe residue
before applying cleanup. Do not persist runtime topology, task state, or merge-check
polling as closeout evidence.

## Supersession carries its corrections

When one candidate replaces another, the replacement inherits the superseded candidate's
findings, dispositions, and corrections. Carry forward:

- corrections to claims the replacement still makes;
- accepted findings not yet repaired, with the evidence behind each disposition;
- limitations and `NOT_PROVEN` boundaries still applying to the replacement;
- revalidated dispositions against the replacement candidate.

Where the replacement's claim differs, state which corrections no longer apply and why
rather than dropping them silently.

## Results and routes

- `RECONCILED` → `deliver-pr` or `deliver-goal`
- `PARTIAL` → preserve remaining acceptance in the root-held claim frame
- `SUPERSEDED` / `DELIBERATELY_CLOSED` → preserve the durable disposition
- `REVIEW_REQUIRED` → `finish-pr` / `review-pr`
- `CHANGES_REQUIRED` → `address-review-comments`
- `PR_IN_FLIGHT` → update the claim frame with the wake event and return to `deliver-goal`
- `CANDIDATE_MOVED` → refresh only affected proof/review/integration
- `MERGE_BLOCKED` / `NOT_PROVEN` → preserve the exact blocker or missing evidence

> Self-authored reversible corrections follow the canonical [correction and disclosure contract](../../../docs/agents/DEVELOPMENT_METHOD.md); this reference does not override higher-precedence instructions.
