# Learned LEM Estimates

PR 16 of the rollout. The aggregator + consumer scripts that turn observed
`ci-actuals.json` artifacts into per-lane percentile estimates the PR Plan
can consume in place of the static `base_lem` floors.

> Companion: [pr-plan.md](pr-plan.md), [ci-actuals.md](ci-actuals.md),
> [lem-budgeting.md](lem-budgeting.md).

This is **scaffolding**: the scripts run on zero data today (they emit a
valid empty history) and start producing meaningful estimates only after
`ci-actuals.json` artifacts have accumulated.

---

## Pieces

| File | Role |
|---|---|
| `scripts/ci/aggregate_lane_history.py` | Walks `target/ci/actuals/` for `ci-actuals.json`; computes per-lane `samples`, `p50`, `p90`, `p95`. Emits `.ci/metrics/ci-lane-history.json`. |
| `scripts/ci/learned_estimate.py` | Reads the history file; given a lane id, returns versioned stdout JSON (`schema_version: learned_estimate.v1`) with the learned estimate, p90 warning threshold, and p95 hard-planning threshold. Falls back to the static floor when fewer than `MIN_SAMPLES_FOR_LEARNED` samples exist. |
| `.ci/metrics/ci-lane-history.json` | Output of the aggregator. Tracked in git so the planner can read it without an extra CI step. |

---

## Estimate model

```text
estimate     = max(static_floor, p50_recent_actual * 1.15)
warning      = p90_recent_actual
hard_planning = p95_recent_actual
```

The 15% safety margin on top of `p50` absorbs ordinary CI noise without
over-budgeting. The `static_floor` clamp guards against runaway optimism
when `p50` lags real cost — e.g. when a lane has a fast-path that
dominates samples but occasionally hits a slow path that the floor still
captures.

`MIN_SAMPLES_FOR_LEARNED = 5` per lane: until the lane has at least that
many samples in the rolling window, the planner keeps using the static
floor and the consumer returns `learned: false`.

### Envelope checks on read

`schema_version` is not write-only. Each consumer validates the envelope before
reading any field:

| Consumer | Behavior |
|---|---|
| `aggregate_lane_history.py` | `validate_history_payload()` requires an exact integer `schema_version == 1` before emitting. |
| `pr_plan.py` | `load_learned_history()` requires an exact integer `schema_version == 1` and an object `lanes`, and reports how the read resolved in the plan receipt's `learned.history_disposition`. |
| `learned_estimate.py` | `main()` requires an exact integer `schema_version == 1` before reading lane fields, and reports `learned: false` with an `unsupported history schema_version` reason otherwise. |

"Exact integer" means a JSON number with no fractional part and not `true`/`false`:
Python's `bool` is an `int` subclass and `1.0 == 1`, so every consumer checks
`type(schema_version) is int` before the value comparison.

The read-side check exists because a payload from a different envelope version
is not merely unreadable — it is *consumable*, and reading it as v1 writes
another version's numbers into the lane estimate. A v2 payload that still used a
`lanes` key would have had its `static_floor` substituted for the planner's own
`base_lem`, with no error. See [pr-plan.md](pr-plan.md#history-envelope).

A rejected payload falls back to the static `base_lem` floors, the same as an
absent or corrupt file, and is named in the plan warnings. If you introduce a
second envelope version, update `HISTORY_SCHEMA_VERSION` in `pr_plan.py` (and
`SCHEMA_VERSION` in the aggregator) together — the consumers fail closed against
versions they were not written for, by design.

---

## Window

Default 14 days. Set with `--window-days N`. The aggregator filters by
file `mtime` rather than embedded timestamps so receipts older than the
window simply drop off without explicit pruning.

---

## Output schema

`.ci/metrics/ci-lane-history.json`:

```json
{
  "schema_version": 1,
  "generated_at": "2026-05-07T09:53:20Z",
  "window_days": 14,
  "min_samples_for_learned": 5,
  "lane_count": 23,
  "lanes": {
    "pr_smoke": {
      "samples": 6,
      "static_floor": 4.0,
      "learned": true,
      "p50": 2.4,
      "p90": 3.2,
      "p95": 3.3,
      "min": 1.4,
      "max": 3.4,
      "mean": 2.4
    },
    "mutation": {
      "samples": 0,
      "static_floor": 60.0,
      "learned": false
    }
  }
}
```

---

## Consumer output

Stdout is a JSON object. `schema_version` is this producer's field
(`learned_estimate.v1`), not the history file's integer `schema_version`.

```bash
python3 scripts/ci/learned_estimate.py --lane pr_smoke
```

```json
{
  "schema_version": "learned_estimate.v1",
  "lane": "pr_smoke",
  "learned": true,
  "estimate": 4.0,
  "estimate_source": "static_floor (higher than learned)",
  "static_floor": 4.0,
  "p50": 2.4,
  "p90_warning": 3.2,
  "p95_hard_planning": 3.3,
  "samples": 6
}
```

When history is missing or the lane has too few samples:

```json
{
  "schema_version": "learned_estimate.v1",
  "lane": "mutation",
  "learned": false,
  "estimate": 60.0,
  "static_floor": 60.0,
  "samples": 0,
  "reason": "only 0 samples; need 5 to learn"
}
```

---

## Wiring status

The minimal end-to-end story:

1. ✓ Aggregator + consumer scripts (PR 16).
2. CI workflow change to upload `ci-actuals.json` artifacts to a
   discoverable location. **Pending.** PR 08's
   [`scripts/ci/emit_ci_actuals.py`](../../scripts/ci/emit_ci_actuals.py)
   produces the right files; what's missing is a step in `ci.yml` (or a
   sibling) that runs it after `cargo xtask gates` and uploads the JSON.
3. A scheduled job that downloads recent actuals artifacts and runs
   `aggregate_lane_history.py` to update
   `.ci/metrics/ci-lane-history.json`. **Pending.**
4. ✓ PR Plan reads the history when present (delivered alongside the
   rollout finalize PR). When the consumer reports `learned: true`, the
   planner substitutes `p50 × 1.15` (clamped to the static floor); it
   falls back to the static floor otherwise.

Steps 2 and 3 land as follow-ups once the upload location is decided
(artifact retention vs. committing back to the repo). The planner is
ready for either path.

---

## Manual run (for review / debugging)

```bash
# Run aggregator on a directory of synthetic actuals.
#
# Samples are attributed by `lane_id`. A bare `gate_name` counts only when it
# is literally a lane id; a real gate name such as `fmt` or `clippy_full` is
# dropped as unmapped, and a run where nothing attributes exits non-zero
# (#6217).
mkdir -p /tmp/actuals
echo '{"jobs":[{"lane_id":"pr_smoke","gate_name":"fmt","actual_lem":2.1}]}' > /tmp/actuals/run-1.json
echo '{"jobs":[{"lane_id":"pr_smoke","gate_name":"clippy_full","actual_lem":1.9}]}' > /tmp/actuals/run-2.json
python3 scripts/ci/aggregate_lane_history.py \
  --actuals-dir /tmp/actuals --output /tmp/hist.json

# Query.
python3 scripts/ci/learned_estimate.py --history /tmp/hist.json --lane pr_smoke
```
