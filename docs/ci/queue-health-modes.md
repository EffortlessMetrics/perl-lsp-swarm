# Queue Health Modes

`cargo xtask queue health` classifies default-branch (main) queue safety for orchestrator actions without mutating labels, merging PRs, cancelling workflows, or dispatching agents.

## Commands

```bash
# Default (live) path: reads target/receipts/master-ci-state.json. A missing,
# malformed, or incomplete live input writes a NOT_PROVEN receipt instead of a green one.
cargo xtask queue health --receipt target/receipts/queue-health.json

# Offline fixtures: for rehearsal and documentation only. Fixture output is
# recorded as source OFFLINE_FIXTURE and can never authorize live lanes.
cargo xtask queue health --fixture xtask/tests/fixtures/queue-health/master-green.json
cargo xtask queue health --fixture xtask/tests/fixtures/queue-health/master-pending.json
cargo xtask queue health --fixture xtask/tests/fixtures/queue-health/master-red.json
```

## Modes

- **GREEN**
  - Merge drain allowed
  - Cascade update allowed
  - Green-CI promotion allowed
- **PENDING**
  - Read-only review/design allowed
  - No merge-ready promotion unless candidate is current
  - No broad cascade final labels
- **RED**
  - Freeze merge drain
  - Classify shared blocker
  - Allow master-fix and read-only review only
- **NOT_PROVEN** (#15387)
  - Main-CI evidence is missing, unknown, malformed, incomplete, or carries an
    unproven subject identity
  - Only read-only investigation and exact evidence refresh allowed
  - Never equated with pending or red; the receipt preserves why the queue
    state cannot be known

## When GREEN is earned

GREEN requires all of the following; anything less fails closed to NOT_PROVEN:

- the observation is live (`source: LIVE`), never an offline fixture;
- the producer object declares `input_schema_version: 1`;
- `master_sha` is a full 40-character object id for the current default branch;
- the collector reported no `source_error` and `source_complete: true`;
- `ci_state` is present and `green`;
- the required-check denominator is proved: either `expected_required_checks`
  names the exact required contexts, or the collector completed and declared a
  legitimate `zero_applicable` population (distinct from a source failure);
- every expected required context has exactly one typed evidence row
  (`required_check_evidence`, reusing the #15998 `RequiredCheckEvidence`
  pattern) evaluated at exactly `master_sha` against the default-branch
  subject (`candidate_head`) with terminal `SUCCESS`.

Observed failure/pending evidence still dominates: failed checks, a shared
blocker, pending/running checks, or a red/pending `ci_state` classify RED or
PENDING as before — neither opens mutating lanes.

## Receipt fields

Written JSON includes:

- `check` (`queue-health`)
- `schema_version` (2)
- `source` (`LIVE` or `OFFLINE_FIXTURE`)
- `master_sha` (exact default-branch subject; empty only when no usable input existed)
- `mode`
- `allowed_lanes`
- `blocked_lanes`
- `reasons`
- `verdict`

Schema: `.ci/receipts/schemas/queue-health.schema.json`.

## Input shape

The input JSON accepts:

- `master_sha` (required; full 40-character object id for green)
- `ci_state` (`green`, `pending`, `red`; anything else, including missing,
  empty, or unrecognized values, can never yield green)
- `pending_checks`, `running_checks`, `failed_checks` (arrays)
- `input_schema_version` (must be `1` for green)
- `source_complete` (boolean; absent means completeness unknown)
- `source_error` (optional collector/source failure)
- `expected_required_checks` (array; the declared denominator)
- `required_check_evidence` (typed rows: `name`, `evaluated_sha`,
  `subject` (`candidate_head`, `pull_request_integration`, `merge_group`),
  `result` (`SUCCESS`, `PENDING`, `NOT_PROVEN`, `STALE`, ...))
- `zero_applicable` (boolean; collector completed with zero applicable checks)
- `failure_classifier.shared_blocker`, `failure_classifier.summary` (optional)
- `gate_policy.pending_allows_merge_ready_if_candidate_current` (optional)

## Fixture versus live authority

The checked-in `master-green.json` fixture demonstrates the complete typed
shape GREEN requires, but because it is a fixture, the classifier records
`OFFLINE_FIXTURE` and emits NOT_PROVEN: fixture output cannot be consumed as
live queue authorization. A nonexistent default input is never repaired by
hand-writing an unversioned object; the live path fails closed to NOT_PROVEN.
