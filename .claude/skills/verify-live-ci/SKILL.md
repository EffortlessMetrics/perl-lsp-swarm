---
name: verify-live-ci
description: Evaluate one substantively reviewed PR's live checks, threads, draft state, mergeability, and policy without treating CI as review or creating exact-head churn.
user-invocable: false
---

# Verify live CI

This is Claude's live-integration fact skill. It does not perform, infer, or replace the
substantive review owned by `review-pr`.

Read one current GitHub snapshot for the selected PR:

- cumulative substantive review result;
- draft/ready state and any still-valid draft purpose;
- required checks discovered from live policy and relevant advisory checks;
- unresolved review threads and current `CHANGES_REQUESTED` reviews;
- deliberately requested reviewers still pending;
- mergeability, conflicts, queue/ruleset state, and explicit prerequisites;
- applicable changelog, support, release, or publication disposition.

Use repository helpers where they report these facts truthfully. Do not require a
review-run comment, claim digest, review submitted on the latest SHA solely because the
SHA changed, or review-receipt convergence.

## Orchestration affordances

### Lane-root decisions

The lane root retains the substantive-review prerequisite, required/advisory policy
interpretation, failure ownership, whether candidate/review meaning changed, whether a
remote state is in flight or blocked, and whether integration is ready.

### Useful evidence agents

Use focused subagents or context forks where useful for:

- enumerating required checks from live ruleset/policy;
- downloading and classifying high-output logs/receipts/artifacts;
- separating candidate, base, integration, oracle, instrument, environment/capacity,
  pending, and `NOT_PROVEN` outcomes;
- checking platform/package/release evidence identity and evaluated SHA;
- verifying unresolved threads, requested reviews, or merge-queue facts.

Children return exact run/check/artifact identity, observed conclusion, failure class,
direct evidence, contradictions, candidate ownership, what the evidence does/not prove,
and recommended route. They do not mutate the candidate or decide merge readiness.

### Join predicate and return packet

Join one current snapshot only after substantive review is `REVIEW_CURRENT`; required/
advisory status and each check's actual evaluated subject are known; relevant check/
review/thread/queue facts have honest classification; candidate/base/integration/oracle/
instrument/environment/pending/not-proven remain distinct; and one integration posture
plus wake event can be stated.

Return PR/head identity, review result, required-policy source, current facts, failure
classifications, contradictions, limitations, integration posture, exact remote wait/
wake event, and next route.

## Review sufficiency boundary

```text
no useful current substantive review
→ REVIEW_REQUIRED
→ `review-pr`

CHANGES_REQUIRED
→ `address-review-comments`

NOT_PROVEN
→ preserve missing review evidence or authority

BLOCKED_BY_PREREQUISITE
→ preserve the exact prerequisite

SUPERSEDED_OR_CLOSE
→ preserve durable closeout

REVIEW_CURRENT
→ evaluate live integration facts
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

- `INTEGRATION_READY` means current protection/integration facts permit the irreversible
  transition.
- `PR_IN_FLIGHT` means GitHub owns a named pending transition such as required checks,
  requested review, queue state, or armed auto-merge.
- `MERGE_BLOCKED` means a concrete conflict, failed required check, unresolved
  substantive thread/change request, ruleset failure, or prerequisite blocks merge.
- `NOT_PROVEN` means API/check/policy/instrument identity is missing or unreliable.

Pending checks leave substantive review current while integration is `PR_IN_FLIGHT`.

## Live evidence classification

Preserve success, failure, pending, not-applicable, cancelled, missing,
instrument-failure, and not-proven distinctly. Do not use head age as a blanket
evidence rule:

- a cancelled or incomplete run reached no verdict;
- a completed result on an earlier PR head remains usable semantic evidence when later
  commits cannot affect the tested subject. It is not a status attached to the live
  SHA, but the SHA mismatch does not erase what the run established;
- when a later PR commit can affect the tested subject, rerun only the affected proof;
- a required status that is missing or pending on the live PR head is a GitHub
  integration fact. Request a job rerun where appropriate without mutating the branch.
  Never rebase, push an empty commit, or replay unrelated CI merely to manufacture a
  current status.

Classify failures as candidate-owned, base-owned, integration interaction,
test/oracle defect, instrument failure, environment/capacity, pending, or
`NOT_PROVEN`. Do not widen the PR to absorb unrelated baseline failures, and do not
ignore current evidence contradicting the reviewed claim.

Prefer a cheap discriminator over a rebuild. Two settle most inherited reds without
running anything:

- **merge-base ancestry.** A candidate inherits whatever was broken at its merge base,
  not whatever `main` looks like now. Ask whether the repair is an ancestor of this
  candidate's merge base — `git merge-base --is-ancestor <repair> <pr-merge-base>` —
  rather than whether `main` is currently green. A red that predates the merge base is
  only a candidate for base ownership: require matching check identity and failure
  evidence at that merge base before assigning `base-owned`; otherwise retain
  `NOT_PROVEN`. Refreshing the branch may be the repair, but ancestry alone is not
  more proof;
- **by construction.** A gate derived from a property the candidate cannot affect is not candidate-owned. For path-based gates, compare against the full changed-path set, including modified and renamed paths, not only additions and deletions; the changed paths can settle the question without a build.

State which discriminator was used. An unclassified red is `NOT_PROVEN`, not someone
else's problem: "this also fails on main" is a claim about the wrong tree unless the
merge base was the thing compared.

## GitHub and wake boundary

Read live state when the skill is entered or a named wake event occurs. When GitHub owns
the next transition, update one existing route/PR summary only if another context needs
the exact pending fact and wake event, then return `PR_IN_FLIGHT`.

Do not poll unchanged state, post repeated check summaries, copy raw logs, or write an
integration frontier/state file. Post only for a material candidate-owned failure,
changed prerequisite/route, corrected instrument classification, `INTEGRATION_READY`
synthesis, or closeout-relevant limitation.

## Semantic currentness

- finding repair → check affected finding, proof, and seam;
- material claim, production route, authority, proof, compatibility, risk, or rollback
  change → `review-pr` for affected dimensions;
- formatting, editorial cleanup, generated receipt refresh, or stronger tests → no
  automatic full-review restart;
- conflict/combined-tree repair → rebase or otherwise integrate as useful, then refresh
  only the affected proof and review;
- `main` advances while the candidate remains conflict-free → no required candidate,
  proof, or review action;
- rebase is acceptable when resolving an actual conflict or when the lane owner judges
  that a base refresh materially simplifies active work or reduces a concrete
  integration risk;
- repeated rebases solely to chase `main` or replay CI are churn; distinct integration
  work may justify more than one.

Do not update the candidate, rebase it, or replay all proof merely because a
conflict-free branch is behind. Commit distance is advisory; a concrete integration
purpose governs the rebase.

## Routes

- `REVIEW_REQUIRED` → `review-pr`
- `REVIEW_FINDINGS_OPEN` / `CHANGES_REQUIRED` → `address-review-comments`
- `REVIEW_SCOPE_CHANGED` → `review-pr` for affected dimensions
- `DRAFT` → `publish-pr`
- `PENDING` / `PR_IN_FLIGHT` → return exact pending transition to `finish-pr` or `deliver-goal`
- `PRODUCT_OR_TEST_FAILURE` → `build-candidate`, then affected proof/review
- `CONFLICT` / `INTEGRATION_INTERACTION` → repair affected seam, then affected proof and `review-pr`
- `BLOCKED_BY_PREREQUISITE` / `MERGE_BLOCKED` → preserve exact blocker
- `INSTRUMENT_FAILURE` / `NOT_PROVEN` → name missing reliable evidence
- `INTEGRATION_READY` → `merge-reconcile`
