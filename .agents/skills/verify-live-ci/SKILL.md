---
name: verify-live-ci
description: Evaluate one substantively reviewed PR's live checks, threads, draft state, mergeability, and policy, and verify native queue custody without treating CI as review or creating exact-head churn.
---

# Verify live CI

## Local proof before native queue admission

The October 22 #17073/#17900 user direction supersedes older qualification and
publication instructions below: no CI on Draft or Ready. Tests and substantive
review must be green locally in session before enqueue; CI runs on the actual
merge-group subject G only. Preserve each proof command, result, source/tree and
limitations separately from reported tests, resource admission and live integration.
A source review or reported green result does not prove a test executed locally.

Before push, PR creation/update, Ready, submitted review, dispatch, rerun or enqueue,
inspect the current direct and indirect event graph. If the action would trigger
ordinary CI, hold that action and prepare a portable local patch and source-bound
proof for the existing owner. The old installed qualification route is not an
exception to this user hold. Do not manufacture required-check success, weaken
selected obligations, change protection/security/credentials or activate the
incomplete queue to unblock publication.

Only when the existing supported native route and complete effective protection
are proved live may exact-H local green, current substantive review, resolved
findings and conditional landing authorization enter expected-H enqueue. A missing
route, trusted pre-enqueue admission or G producer is NOT_PROVEN with its exact
owner/capability blocker, not permission to dispatch ordinary CI. Do not invent an
invocation or treat Ready, acknowledgment or runner waiting as queue custody.
Verified entry/group/run/attempt supports PR_IN_FLIGHT. Green G proceeds to native
landing readback and reconciliation without another direct merge or mandatory
post-green manual pause. A late material finding withdraws eligibility through the
existing supported controls; prose alone does not prevent automatic landing.


Evaluate local green and current substantive review before conditional exact-H landing
authorization and native enqueue. Preserve mandatory selected tests, trusted admission
and required producer/app identity. The native route is pending activation; missing
capability is `NOT_PROVEN`, never permission to request ordinary CI. Budget actual G
qualification toward 20 minutes without dropping proof. UB Review remains paused;
no Tower deployment is implied.

This is Codex's live-integration fact and status-production skill. It does not perform,
infer, or replace the substantive review owned by `$review-pr`.

Read one current GitHub snapshot for the selected PR:

- cumulative substantive review result;
- draft/ready state and any still-valid draft purpose;
- required checks from the union of applicable branch rulesets AND classic branch
  protection, including producer/app identity, plus selected advisory checks;
- unresolved review threads and current `CHANGES_REQUESTED` reviews;
- deliberately requested reviewers still pending;
- mergeability, conflicts, queue/ruleset state, and explicit prerequisites;
- applicable changelog, support, release, or publication disposition.

Use repository helpers where they report these facts truthfully. Do not require a
review-run comment, claim digest, review submitted on the latest SHA solely because the
SHA changed, or review-receipt convergence.

## Orchestration affordances

### Root decisions

The accountable root retains the substantive-review prerequisite, required/advisory
policy interpretation, failure ownership, whether candidate/review meaning changed,
whether a remote state is in flight or blocked, and whether integration is ready.

### Delegable read-only work

Use focused workers where useful for:

- enumerating required checks from live ruleset/policy;
- downloading and classifying high-output logs/receipts/artifacts;
- separating candidate, base, integration, oracle, instrument, environment/capacity,
  pending, and `NOT_PROVEN` outcomes;
- checking platform/package/release evidence identity and evaluated SHA;
- verifying unresolved threads, requested reviews, or merge-queue facts.

Workers return exact run/check/artifact identity, observed conclusion, failure class,
direct evidence, contradictions, candidate ownership, what the evidence does/not prove,
and recommended route. They do not mutate the candidate or decide merge readiness.

### Join predicate and return packet

Join one current snapshot only after:

- substantive review is `REVIEW_CURRENT`;
- required/advisory status and each check's actual evaluated subject are known;
- every current relevant check/review/thread/queue fact has an honest classification;
- candidate-owned, base-owned, integration, oracle, instrument, environment, pending,
  and not-proven outcomes remain distinct;
- one exact integration posture and wake event can be stated.

Return PR/head identity, substantive review result, required-policy source, current
check/review/thread/queue facts, failure classifications, contradictions, limitations,
integration posture, exact remote wait/wake event, and next route.

## Review sufficiency boundary

```text
no useful current substantive review
→ REVIEW_REQUIRED
→ `$review-pr`

CHANGES_REQUIRED
→ `$address-review-comments`

NOT_PROVEN
→ preserve missing review evidence or authority

BLOCKED_BY_PREREQUISITE
→ preserve the exact prerequisite

SUPERSEDED_OR_CLOSE
→ preserve durable closeout

REVIEW_CURRENT
→ require executed local green and evaluate live native admission/protection facts
```

Green checks, textual mergeability, zero open threads, bot approval, or author
self-certification cannot promote a candidate to `REVIEW_CURRENT`.

## Integration postures

```text
INTEGRATION_READY
PR_IN_FLIGHT
MERGE_BLOCKED
NOT_PROVEN
```

- `INTEGRATION_READY` means current required actual-G qualification and effective
  protection permit native landing. It does not mean the PR has landed or closed.
  Open green G stays under native landing ownership until actual M is observed. This
  never authorizes a second direct merge; conditional landing authorization precedes enqueue.
- `PR_IN_FLIGHT` means GitHub owns a verified native entry/group/run/attempt for the
  selected H/G and a named pending qualification or landing transition; no generic auto-merge.
  Pre-enqueue requested review returns `REVIEW_REQUIRED` with the exact review wait;
  it does not establish native custody.
- `MERGE_BLOCKED` means a concrete conflict, failed required check, unresolved
  review thread of any author, current change request, ruleset failure, or
  prerequisite blocks merge.
- `NOT_PROVEN` means API/check/policy/instrument identity is missing or unreliable.

Pending checks on a verified native G leave substantive review current while
integration is `PR_IN_FLIGHT`. An ordinary pending PR check does not prove custody.

Read the unresolved thread count with `scripts/reviews/threads <pr> --unresolved-only
--json` and require `unresolved_count` of 0 before `INTEGRATION_READY`. The `main`
ruleset sets `required_review_thread_resolution`, so a bot thread (Devin, Gemini,
Codex, CodeRabbit, cubic), an outdated thread, or an editorial thread blocks the merge
exactly like a substantive one. An unresolved review thread of any author is
`MERGE_BLOCKED` routed to `address-review-comments`, not `PR_IN_FLIGHT`; zero
unresolved review threads is a precondition of protected integration.

## Native admission and status-production order

This skill owns one admissible next action from one snapshot. Branch mutation belongs
to the established writer. The following replaces legacy PR-head dispatch/rerun routes:

1. **Local tests or substantive review missing/stale/failed.** Continue affected local
   proof, review or repair. Do not enqueue, dispatch or rerun CI to fill the gap.
2. **Native route, trusted pre-enqueue admission, required G producers or complete
   effective protection unproved.** Return `NOT_PROVEN` with exact owner/capability
   blocker and portable source-bound evidence. No queue activation or fake success.
3. **Eligible exact H, route proved live, no known request.** Make conditional landing
   authorization before invoking only the existing documented expected-H native enqueue.
   No executable invocation is supplied here because its live capability is unproved.
4. **Request acknowledgment, lost response or resumed context.** Reconstruct existing
   native entry/group/run/attempt and actual H/G before another action. Acknowledgment
   alone is `NOT_PROVEN`, not custody; a moved H cannot inherit old admission.
5. **Actual matching native entry/G run active.** Return `PR_IN_FLIGHT` with accountable
   owner and exact wake. An ordinary PR-head run or runner-capacity wait is not queue
   custody. Existing historical PR runs may be inspected without starting another.
6. **G failed/ejected or instrument failed.** Preserve the original cause/run/attempt;
   route affected local repair/review or instrument/capacity work to its owner. A retry
   requires changed evidence, current eligibility and the supported native mechanism
   targeting actual G only; no generic workflow dispatch or ordinary PR rerun fallback.
7. **G green.** Inspect actual selected execution and reconcile native landing M and
   composition. Do not issue a direct merge, mandatory post-green approval or full replay.

A material adverse finding withdraws eligibility through existing supported controls.
Do not leave automatic landing eligible while relying on a prose-only objection.

### Remote-action persistence law

A command invocation, API 2xx, or requested transition is not itself evidence that the
transition exists. Return `PR_IN_FLIGHT` after native enqueue or supported G retry
only when a fresh GitHub read confirms:

```text
expected H + actual native entry/group/G subject
+ complete ordered constituents and expected context/workflow/run identity
+ expected new state or attempt
+ exact terminal wake event
```

If read-back fails, is contradictory, or names another subject, preserve the action
failure or return `NOT_PROVEN`. Do not take a second status-production action from the
same snapshot; re-read only after the named wake event.

## Terminal inspection and ownership

For the selected late qualification, retain one accountable owner through terminal
run/attempt inspection and native landing reconciliation. Conditional product/scope
and protected landing authorization must already precede enqueue. Use supported completion events or bounded checks during
the ordinary wait window. Do not return an ownerless `PR_IN_FLIGHT` merely because a
worker ended. On a real interruption, name `continuation_owner`, `next_action`, the
exact request/head/merge subject and run/attempt, pending contexts and `wake_event` in
the existing handoff packet; require receiver acceptance before transferring ownership.
Missing ownership is `NOT_PROVEN`, not a successful handoff. Do not add a state registry,
recurring poller, or repeat unchanged snapshots.

At terminal completion ask: did the selected tests actually execute on the admitted
subject? Inspect result/log/artifact content, not only the badge. `SKIPPED`, `NEUTRAL`,
`draft-no-proof`, and "no analysis executed" do not prove tests passed, even if a wrapper
is green. Preserve not-applicable separately with policy evidence; mandatory selected
proof still needs actual execution. Carry required union and reporting identity through
terminal results; do not drop the slow tail. Then return the evidence to
`$merge-reconcile` for actual native landing readback and objective reconciliation
only; its legacy direct-merge/post-green approval instructions do not apply.

## Live evidence classification

Preserve success, failure, pending, not-applicable, cancelled, missing,
instrument-failure, and not-proven distinctly. Do not use head age as a blanket
evidence rule:

- a cancelled or incomplete run reached no verdict;
- a completed result on an earlier PR head remains usable semantic evidence when later
  commits cannot affect the tested subject. It is not a status attached to the live
  SHA, but the SHA mismatch does not erase what the run established;
- when a later PR commit can affect the tested subject, rerun only the affected proof;
- a required status missing or pending on H is an integration fact. Resolve trusted
  no-CI pre-enqueue admission through its owner, or return `NOT_PROVEN`; do not request
  a PR job rerun, dispatch, empty commit or branch refresh to manufacture the status;
- old PR integration P cannot qualify actual G. Recomposition/base/constituent changes
  require the existing native mechanism to evaluate the changed G, or `NOT_PROVEN`.
  An ordinary PR rerun cannot answer that subject. Inspect main's own existing results
  when evidence suggests a base-owned failure before repairing branch-locally.


Classify failures as candidate-owned, base-owned, integration interaction,
test/oracle defect, instrument failure, environment/capacity, pending, or
`NOT_PROVEN`. Do not widen the PR to absorb unrelated baseline failures, and do not
ignore current evidence that contradicts the reviewed claim.

Prefer a cheap discriminator over a rebuild. Two settle most inherited reds without
running anything:

- **merge-base ancestry.** A candidate inherits what was broken at its merge base, not
  what `main` looks like now. Ask whether the repair is an ancestor of this candidate's
  merge base — `git merge-base --is-ancestor <repair> <pr-merge-base>` — rather than
  whether `main` is currently green. A red predating the merge base is only a candidate
  for base ownership: require matching check identity and failure evidence at that merge
  base before assigning `base-owned`; otherwise retain `NOT_PROVEN`. A branch refresh
  may be the repair, but ancestry alone is not further proof;
- **by construction.** A gate derived from a property the candidate cannot affect is not candidate-owned. For path-based gates, compare against the full changed-path set, including modified and renamed paths, not only additions and deletions; the changed paths can settle the question without a build.

Name the discriminator used. An unclassified red is `NOT_PROVEN` rather than someone
else's problem: "this also fails on main" describes the wrong tree unless the merge base
was compared.

## GitHub and wake boundary

Read live state when the skill is entered or a named wake event occurs. When GitHub owns
the next transition, update one existing route/PR summary only if another context needs
the exact pending fact and wake event, then return `PR_IN_FLIGHT`.

Do not poll unchanged state, post repeated check summaries, copy raw logs, or write an
integration frontier/state file. Post a durable update only for a material candidate-
owned failure, changed prerequisite/route, corrected instrument classification,
`INTEGRATION_READY` synthesis, or closeout-relevant limitation.

## Semantic currentness

- finding repair → check affected finding, proof, and seam;
- material claim, production route, authority, proof, compatibility, risk, or rollback
  change → `$review-pr` for affected dimensions;
- formatting, editorial cleanup, generated receipt refresh, or stronger tests → no
  automatic full-review restart;
- conflict/combined-tree repair → rebase or otherwise integrate as useful, then refresh
  only the affected proof and review;
- `main` advances while the candidate remains conflict-free → no required candidate,
  proof, or review action;
- rebase is acceptable when resolving an actual conflict or when the accountable root
  judges that a base refresh materially simplifies active work or reduces a concrete
  integration risk;
- repeated rebases solely to chase `main` or replay CI are churn; distinct integration
  work may justify more than one.

Do not update the candidate, rebase it, or replay all proof merely because a
conflict-free branch is behind. Commit distance is advisory; a concrete integration
purpose governs the rebase.

## Routes

Publication, review submission and durable updates require a proved no-CI effect.
If a downstream skill asks for ordinary CI, direct merge or mandatory post-green
approval, hold that action and return the exact `NOT_PROVEN` boundary with local proof.


- `REVIEW_REQUIRED` → `$review-pr`
- `REVIEW_FINDINGS_OPEN` / `CHANGES_REQUIRED` → `$address-review-comments`
- `REVIEW_SCOPE_CHANGED` → `$review-pr` for affected dimensions
- `DRAFT` → continue permitted local work; `$publish-pr` only through a proved
  no-CI path, otherwise portable patch + `NOT_PROVEN`
- `PENDING` / `PR_IN_FLIGHT` → return exact pending transition to `$finish-pr` or `$deliver-goal`
- `PRODUCT_OR_TEST_FAILURE` → `$build-candidate`, then affected proof/review
- `CONFLICT` / `INTEGRATION_INTERACTION` → repair affected seam, then affected proof and `$review-pr`
- `BLOCKED_BY_PREREQUISITE` / `MERGE_BLOCKED` → preserve exact blocker
- `INSTRUMENT_FAILURE` / `NOT_PROVEN` → name missing reliable evidence
- `INTEGRATION_READY` → inspect native state; landed/closed → `$merge-reconcile` readback only; open → retain native landing ownership
