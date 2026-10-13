---
name: ci-failure-triage
description: Diagnose failing CI workflows, builds, lint jobs, typechecks, tests, flaky jobs, pipeline regressions, or failed GitHub Actions using logs, reproduction, and minimal fixes.
user-invocable: false
---

# CI failure triage

Fix CI from evidence instead of guesses. Establish whose red it is and which subject the
run evaluated before assigning repair ownership or selecting any remote action.

## Required checks

- Capture the workflow, job, event, head, merge subject, run attempt, command, file, and
  failure signature.
- Reproduce locally where the repository supports it.
- Distinguish deterministic failure from cancellation, capacity, and flaky behavior.
- Compare candidate, merge-base, base, and integration subjects before assigning ownership.
- Prefer the smallest repair that makes the failing signal truthful.
- Do not weaken tests, policy, lint, type checks, or evidence identity to obtain green.
- Record unreproduced or contradictory failures as missing evidence.

## Classification packet

This skill classifies; it does not dispatch, rerun, approve, update a branch, or create a
commit. Return a packet containing:

```text
evaluated_subject
required_subject
same_run_sufficient: true | false
status_production_gap: fresh_integration_subject | missing_context | none
required_context
failure_signature
failure_owner: candidate | base | integration | oracle | instrument | environment | not_proven
evidence and limitations
```

When a merge-tree run predates material base movement and a rerun would replay its
original snapshot, set `same_run_sufficient=false` and
`status_production_gap=fresh_integration_subject`. This is classification consumed by
`verify-live-ci`; it is not a lifecycle result and is not terminal `NOT_PROVEN` while
an admissible status-production action may still exist.

`verify-live-ci` alone selects, performs, or routes the next status-production action
and returns the canonical integration result. This skill never turns the classification
into an empty-commit command.

## Ownership and base movement

A red badge does not identify its owner.

- A cancelled or incomplete run reached no verdict.
- A completed failure on an older head remains actionable when later commits cannot
  affect its subject; revalidate the failing seam rather than dismissing it by SHA age.
- Base ownership requires the same check identity and failure signature observed at the
  PR's merge base. Current `main` alone is the wrong comparison tree.
- A candidate incapable of affecting the tested property may be cleared by construction,
  but path-based reasoning must use the complete changed-path set.
- A fresh advisory shard red on both `main` and the candidate may be a main-red signal;
  enumerate all of main's check-runs before repairing branch-locally.

Name the discriminator. With identity, subject, or signature missing, retain
`failure_owner=not_proven` rather than assigning the defect elsewhere.

## Definition of done

- Root cause or exact missing evidence is stated.
- Evaluated and required subjects are explicit.
- Failure ownership is supported rather than inferred from redness.
- The classification packet says whether the same run can answer the current question.
- No remote status action or candidate mutation occurred in this skill.
- A smallest valid repair is tied to the signal, or the packet preserves the unresolved
  evidence for `verify-live-ci`.
