# CI Actuals

> **Context**: This document is part of perl-lsp's [Industrialized AI](why-industrialized.md) CI architecture. The choices here are responses to operating at 1000+ PRs/day, not premature optimization.

`ci-actuals.json` records what each gate actually spent in CI. It is the actuals
counterpart to [`ci-plan.json`](pr-plan.md): the plan forecasts cost; the actuals
record what was spent. Together they let later PRs derive learned LEM estimates.

> Companion: [pr-plan.md](pr-plan.md), [lem-budgeting.md](lem-budgeting.md).

---

## What it does

`scripts/ci/emit_ci_actuals.py` walks `target/receipts/` for gate receipts produced by
`xtask`, extracts `gate_name`, `tier`, `status`, `duration_ms`, and runner, and converts
each receipt into a per-job actuals entry with:

- `lane_id` = the lane this job ran in, from `--lane-id` (see below)
- `actual_minutes` = `duration_ms / 60000`
- `actual_lem` = `actual_minutes × runner_multiplier`
- `estimated_lem` = `policy/ci-lanes.toml`'s `base_lem`, but only when the gate *is*
  itself a whole lane (a 1:1 gate). A gate that is one of many inside a lane has no
  floor of its own and reports `null`.
- `delta_lem` = `actual_lem − estimated_lem`

`totals.estimated_lem` sums each represented lane's `base_lem` **once**. A lane floor is
a property of the lane, not of every gate inside it, so adding it per job would multiply
a shard lane's whole budget by its gate count.

### `--lane-id` is required for a sample to be usable

Gate names and lane ids are different namespaces, and gate names are **N:1** into lanes:
`fmt`, `clippy_full`, and `unit_foundation_full` all run inside `merge_gate_shards`. The
mapping therefore cannot be recovered from the gate name, and must not be guessed —
`compile_all_targets`/`check_all_targets` and `docs_build`/`docs_gate` are near-miss
pairs that a fuzzy match would bind to the wrong lane.

The invoking workflow knows its lane, so it passes `--lane-id`, and the emitter stamps it
on every job. `scripts/ci/aggregate_lane_history.py` attributes samples by `lane_id`,
accepts a bare `gate_name` only when it is *literally* a known lane id, and drops
anything else rather than inventing a lane for it.

Omitting `--lane-id` for a multi-gate lane produces actuals whose samples the aggregator
cannot attribute. An unknown `--lane-id` is refused at emit time.

The aggregator distinguishes two reasons for attributing nothing, because they have
opposite correct responses (#6217):

| Condition | Response |
| --- | --- |
| samples arrived, none attributed, and **no** artifact in the window carries `lane_id` | **warn**, exit 0 — every artifact predates the wiring, which is mechanical and self-resolving. Expires on `LANE_ID_ROLLOUT_DEADLINE`, and the warning names that date. |
| samples arrived, none attributed, and artifacts **do** carry `lane_id` | **error**, exit 1, from day one — the wiring exists and is still producing nothing usable |
| nothing arrived, or samples attributed to real lanes | quiet, exit 0 |

Collapsing those into one hard failure would red the daily aggregation for the whole
14-day window while pre-wiring artifacts age out, and a chronically red scheduled
workflow is an ignored one. The warn expires so that a workflow which never got its
`--lane-id` cannot sit warning indefinitely, which would recreate the original silence
by a slower route.

### Observation admission and refusal

The emitter accepts whole `gates.v1` envelopes with metadata, a nonempty gate
population and a matching `summary.total_gates`. The separate `ci_gate_shard.v1`
summary is ignored; it is not another execution. Unknown versions, standalone
gate objects, malformed files and missing observations are refused, not silently
dropped from the lane.

Each consumed gate needs its name, tier, status and an observed unsigned 64-bit
integer `duration_ms`. Zero is measured zero, including a producer-reported skipped
gate; missing or invalid timing is not zero. Runner multipliers, lane floors and
derived costs must be finite and nonnegative. The existing lane attribution and
once-per-lane floor remain unchanged, as does actuals schema 1.

The history collector sums per-job actuals. Publishing a measured gate beside an
unmeasured gate would therefore train a partial lane as a cheap complete sample,
even if its total were null. The emitter refuses that publication instead.

Before validation it invalidates only the exact `--json-out` entry, so a refused
invocation cannot leave an old report for an always-upload step. Source receipts,
policies and neighboring files are preserved. A conflicting output path that names
an input is refused without deleting that input. Successful output is strict JSON,
published by a same-directory atomic replacement; publication errors remove the
invocation's temporary file.

This validates supplied observations, not acquisition of every expected sibling
job or unique physical execution. Equal receipt content is not a safe deduplication
key. Gate timing also excludes setup overhead and is not a billing measurement.

---

## Wiring

The script is meant to run after `xtask gates` in CI workflows:

```yaml
- name: Run gates
  run: cargo xtask gates --receipt target/receipts/receipt.json

- name: Emit CI actuals
  if: always()
  run: |
    python3 scripts/ci/emit_ci_actuals.py \
      --receipts-dir target/receipts \
      --workflow "${GITHUB_WORKFLOW}" \
      --sha "${GITHUB_SHA}" \
      --pr "${{ github.event.pull_request.number || 0 }}" \
      --json-out target/ci/ci-actuals.json

- name: Upload ci-actuals
  if: always()
  uses: actions/upload-artifact@v4
  with:
    name: ci-actuals
    path: target/ci/ci-actuals.json
    retention-days: 30
    if-no-files-found: warn
```

The current shard caller in `.github/workflows/ci.yml` supplies
`--receipts-dir target/receipts/shards` and `--lane-id merge_gate_shards`. Its
telemetry step remains non-blocking and its artifact upload runs even after errors;
refusal therefore leaves no actuals file to upload. This producer repair changes
neither workflow triggers nor qualification policy.

---

## Output schema

```json
{
  "schema_version": 1,
  "repo": "perl-lsp",
  "sha": "abc",
  "pr": 8137,
  "workflow": "CI",
  "totals": {
    "actual_lem": 25.0,
    "estimated_lem": 36.0,
    "delta_lem": -11.0
  },
  "jobs": [
    {
      "lane_id": "pr_smoke",
      "gate_name": "pr_smoke",
      "tier": "pr_fast",
      "status": "pass",
      "runner": "ubuntu_24_04",
      "duration_ms": 120000,
      "actual_minutes": 2.0,
      "actual_lem": 2.0,
      "estimated_lem": 4.0,
      "source_path": "target/receipts/receipt.json"
    }
  ]
}
```

---

## Why receipts, not workflow timestamps

The repo already produces structured gate receipts via `xtask`. Reading those is more
durable than scraping GitHub Actions timestamps from inside a job (which require either
log parsing or extra calls to the Actions API). Receipts also distinguish "the gate
ran" from "the job took N seconds" — the gate's own duration is a cleaner signal than
job overhead like cache restore and toolchain setup.

---

## Roadmap

| PR | Change |
|---:|---|
| 08 | Original Python script and documentation; shard wiring has since landed. |
| 13 | Soft LEM warnings using actuals. |
| 16 | Use percentile actuals (`p50`, `p90`, `p95`) to derive learned estimates. |

The first phase is purely observational. Enforcement based on actuals comes only after
a calibration window has accumulated.
