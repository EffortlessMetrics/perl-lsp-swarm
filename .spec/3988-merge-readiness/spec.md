# SWARM-MERGE-1: current-head merge-readiness fan-in (M1)

## Purpose

Provide one deterministic, read-only evaluation of the evidence required before
an exact-head merge attempt. The evaluator consumes a live snapshot; it does
not collect GitHub state, rerun proof, resolve conversations, mutate labels, or
merge a pull request.

## Authorities and boundaries

| Input | Authority | M1 behavior |
| --- | --- | --- |
| Required check names and evaluated conclusions | live ruleset/check collector; repository policy is the local inventory | every required name must have exact-head `SUCCESS` |
| Review convergence | #3693 `scripts/ci/check-pr-review-convergence` | exact-head success, no unresolved conversation, evidenced dispositions, and no required review in flight |
| Release-note disposition | #3784 Changie checker | exact-head evidence; advisory findings remain visible, blocking findings block |
| Protected integration | GitHub ruleset / merge queue collector | exact-head success and merge permission required |

The evaluator emits one status: `READY`, `BLOCKED`, `PENDING`, `NOT_PROVEN`,
`STALE`, `DRAFT_SKIP`, `CANCELLED`, or `NOT_APPLICABLE`. It preserves each
input class and a human-readable finding. A missing, malformed, or
instrument-failed input is never treated as success.

## Snapshot contract

The JSON input contains repository/PR/base/head identity, an optional merge
group SHA, the live required-check name list, check evidence, #3693 review
evidence, #3784 Changie evidence, and protected-integration evidence. Every
evidence producer records the SHA it evaluated. Any mismatch with the current
PR `head_sha` is `STALE` and blocks readiness, including when the underlying
policy is otherwise advisory.

## Proof

- unit tests cover ready, missing required check, older-head evidence, pending
  review, unresolved outdated-inclusive conversations, advisory Changie
  findings, and malformed schema input;
- CLI serialization is exercised with the checked-in ready fixture;
- `cargo fmt -p xtask --check` (the workspace-wide wrapper is not usable on
  this Windows checkout because it hits the path-length limit);
- focused `cargo test -p xtask --bin xtask tasks::merge_ready`;
- focused Clippy and `git diff --check`;
- `cargo allow diff --base origin/main --include-untracked` is attempted and
  recorded honestly if the external tool cannot complete.

## Non-goals / follow-ups

This slice does not implement live GitHub collection, replace #3693/#3987/#3784,
promote a check into branch protection, implement the expected-head merge
mutation, or reconcile post-merge state. Those are M2–M5 work in issue #3988
and the linked #3989 reconciliation issue.
