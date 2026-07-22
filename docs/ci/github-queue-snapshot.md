# GitHub queue snapshot

Status: active
Scope: read-only queue observation
Owner: issue [#4554](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4554)
Authority-map rollout: issue [#4561](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4561)

`cargo xtask queue snapshot` captures a stable JSON snapshot of open PR state.
It is a navigation and reconciliation input, not merge authorization and not a
semantic PR-value classifier.

## Fields

- top-level: `snapshot_id`, `captured_at`, `repository`, `default_branch`,
  `master_sha`, `ruleset_summary`;
- per PR: `number`, `title`, `head_sha`, `base_sha`, `is_draft`,
  `merge_state_status`, `labels`, `status_check_rollup`, `updated_at`, `author`,
  `review_decision`;
- derived observations:
  - `merge_ready`;
  - `ci_green`;
  - `needs_ci_fix`;
  - `needs_builder_fix`;
  - `needs_diff_fix`;
  - `diff_audited_waiting_ci`;
  - `conflicting`;
  - `unknown_not_proven`;
  - `pending_or_unclassified`;
  - `draft`.

`master_sha` is a legacy serialized field name retained for compatibility. Its
value is the captured SHA of the current default integration branch, which is
`main` for this repository.

## Mergeability observations

The snapshot deliberately keeps these states separate:

| Bucket | Meaning | What it does not mean |
| --- | --- | --- |
| `conflicting` | GitHub reports `DIRTY` or `CONFLICTING`; an actual textual conflict needs inspection | automatic rebase, obsolescence, or low value |
| `unknown_not_proven` | GitHub reports `UNKNOWN` or provides no mergeability state | conflict, safe merge, safe close, or safe mutation |
| `pending_or_unclassified` | mergeability is known/non-conflicting, but checks are neither terminal-failing nor all non-blocking | product failure or required wait without further classification |

These observations are orthogonal to CI and routing buckets. A PR can be both
`conflicting` and `needs_ci_fix`; neither finding should hide the other.

The old `stale_or_dirty` and `blocked_unknown` buckets are removed because they
collapsed inactivity, actual conflict, and unavailable state into misleading
action-shaped categories.

## Behavioral rules

- comments are evidence, not authoritative CI state;
- current `head_sha` and exact-head check evidence are freshness truth;
- labels are projected/navigation state, not proof;
- PR age or inactivity is not a close/rebase/cherry-pick disposition;
- `UNKNOWN` is `NOT_PROVEN`, never an inferred conflict;
- `DIRTY`/`CONFLICTING` requires conflict inspection before selecting a repair;
- required/advisory/current/stale check semantics belong to the current-head
  proof and merge-readiness authorities;
- product value, semantic supersession, and base-update strategy belong to
  PLSP-SPEC-0006 and the PR-incorporation review;
- future meaningful-activity work under
  [#4570](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4570) may
  add an `IDLE_REVIEW_NEEDED` observation, but must not restore age-driven
  mutation or closure.

## Compatibility and rollout

This schema change is intentionally explicit rather than silently preserving the
ambiguous bucket. Consumers must migrate to the specific observation they need.
Fixture tests cover clean, conflicting, and unknown mergeability states.

The snapshot remains read-only. It does not apply labels, update branches, close
PRs, resolve reviews, rerun workflows, or merge.
