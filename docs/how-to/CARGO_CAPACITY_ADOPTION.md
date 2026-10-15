# Adopt measured Cargo capacity admission

The canonical owner is `scripts/cargo-admitted` → `scripts/cargo_admitted.py`
(#3230). Retirement means replacing an implicit universal reserve with an explicit
sizing decision, not lowering a constant or treating a large cache as future growth.
Do not activate retirement until representative workloads and their callers have
qualified budgets. Source preparation and host activation are separate outcomes.

## Entrypoints and boundaries

Inventory at main `4cc7960e61b685ce3b7e4cb9fe43b929bf900972`:

| Entry | Capacity behavior | Adoption owner/action |
| --- | --- | --- |
| `scripts/cargo-admitted`, direct Python, admitted just recipes | Python percent policy defaulted to 40 GiB and 85% used; environment byte-budget also defaulted/clamped reserve to 40 GiB | This change removes implicit reserve and clamp. Supply scope-bound budget, or explicit compatibility host reserve. `cargo_cohort.py` consumes an already held admission lease rather than choosing a new capacity policy. |
| `scripts/cargo-safe` heavy verbs (`build`, `check`, `test`, `run`, `bench`, `doc`, `clippy`, `nextest`) | Separate Bash default 40 GiB/85%, shared target and optional sccache; checks only devplane before its lock | Migrate callers through canonical execution under #9567/#3230. Do not chain wrappers or remove this guard independently. |
| `justfile` `cargo_safe` alias and direct legacy calls; `.cargo/config.toml` guidance; repository skills/tasks | Legacy projections or guidance, not additional threshold implementations | Adapter owners must preserve selected obligations, arguments, failure status and narrow rerun. Inventory aliases with `rg -n 'cargo_safe|cargo-safe|cargo-admitted' justfile .cargo .agents scripts`. |
| `scripts/ci/pr_smoke_disk_guard.py` and CI workflow | Separate measured tier: 85 GiB footprint + 5 GiB reserve; not a 40 GiB default | Keep its independent measured protection; a focused local result cannot resize the hosted broad tier. |
| Historical `docs/swarm/source-syncs` examples, storage tests and qualification fixtures | Historical settings or synthetic quantities; not live policy | Do not promote these values into host budgets. |

`install`, external subcommands and raw Cargo are not protected simply because a
legacy wrapper name appears in a recipe. Caller migration must preserve execution
coverage rather than assuming every legacy path was admitted. The above search is
an inventory aid, not an executable caller classifier.

## Smallest existing model

For a qualified exact request, require `free_bytes >= reserve_bytes +
expected_growth_bytes` at the slot, target, build, Cargo home and temporary
destinations. Recheck under the original repository lease before allocation.
Apply the aggregate growth bound once at each destination, not once per directory
on a shared filesystem. Applying the whole bound on separate volumes remains
conservative. No new reservation service or volume apportionment is introduced.

Keep four observations distinct:

- **Available capacity:** current free bytes on each actual destination volume.
- **Retained state:** existing allocated cache/output bytes, already reflected in
  available capacity. Record logical and allocated size separately when relevant.
- **Additional peak growth:** new high-water storage above the starting allocation,
  including output, downloads, temp, link transients and sampling uncertainty.
- **Reserve:** host operating headroom plus separately accounted concurrent growth
  outside the admitted operation. A faster CPU does not supply a disk bound.

The repository lease excludes overlapping admitted requests in the same common
repository/host/DEVPLANE slot. Different DEVPLANE roots, raw Cargo, other repositories
and external artifact consumers are outside that exclusion domain. Coordinate their
growth and memory explicitly; do not infer a host-wide reservation from preflight.
Changing a budget must never change settlement or lease-release authority.

## Qualify before activation

| Workload | Required measurement boundary |
| --- | --- |
| No-build inspection/Python/formatting | Run directly through its existing non-Cargo route; bootstrapping xtask is a build and needs its own bound. |
| Focused check, Clippy, named test | Exact package/target/profile/toolchain, dependency closure, source and configuration identity; cold, warm, edit/repeat/restore. |
| Broad package/all-targets or workspace proof | Measure that actual closure; never extrapolate a dependency-free fixture or named target. Preserve required coverage. |
| Concurrent work | Same-slot exclusion plus independently coordinated requests on shared destination volumes; measure combined peak storage/memory and contention. |
| Cancellation or failed build | Product failure separate from descendant settlement, original-lease release, immediate repeat and consumer ownership. Native Windows needs its own qualified process-tree authority. |

Collect command, source/config/toolchain/profile, starting caches and volumes,
duration/queue time, retained allocated bytes, sampled peak growth and sampling
interval, memory/CPU observations, exit/product status, settlement and matching
lease release. A sampled peak is not an exact maximum. Preserve assertions,
overflow checks, required targets and other proof coverage while improving size.
Neither `basis_verified:false` nor a budget reference independently verifies sizing.

## Exact adoption steps

1. Join profile/artifact and hook measurements on the intended native host. Review
   the measured envelope and safety margin for each selected closure. Until then,
   retain existing production guards; this source candidate is not activation proof.
2. Choose task-owned native paths and inspect free space and existing consumers.
   Do not move or clean active outputs. Set `DEVPLANE`, `CARGO_HOME`, and temporary
   paths only for the selected invocation; no global configuration change is needed.
3. Unset capacity environment settings for the scope-bound route. Run
   `python scripts/cargo_admitted.py --preflight <exact Cargo arguments>`.
   With no sizing this returns 75 and emits a DENY scope without allocating or
   launching Cargo. Capture the emitted scope, rather than reconstructing it.
4. Create the existing schema-v1 budget file described in
   [constrained Cargo builds](CONSTRAINED_CARGO_BUILDS.md), with that exact scope,
   positive integer reserve/growth byte counts and a reference to the reviewed
   measurement. Do not copy example quantities or fabricate verified measurements.
5. Run `python scripts/cargo_admitted.py --preflight --budget-file ABSOLUTE_JSON
   <exact Cargo arguments>`, then the identical execution without `--preflight`.
   Preflight is a snapshot, not a reservation. A changed scope requires requalification.
6. Prove insufficient-space refusal at each destination, same-slot exclusion,
   capacity change after preflight, truthful failure/cancellation, settlement and
   matching release, then immediate repeat. Run focused regression first; use the
   existing admission route for any representative Cargo build.
7. Coordinate adapter migration with #9567 and hook owners, preserving required
   proof. Replace legacy default40 entrypoints only after parity and the native
   ownership route are qualified. Do not bypass a hook to publish this candidate.
8. Activate through the normal reviewed integration path only when all selected
   callers have budgets. Roll back source/caller changes if qualification fails;
   preserve caches, original leases and investigation evidence for owner review.

Compatibility `percent` and environment `byte-budget` policies require explicit
positive finite `MIN_FREE_GB`. Byte-budget also requires total growth and its
evidence reference. These remain operator host policies, not scope-bound measured
profiles. No automatic small-workload sizing is supplied by this change.

## Regression evidence ceiling

`python -m unittest discover -s scripts/tests -p 'test_cargo_admitted*.py'`
exercises existing budget scope, destination, recheck and lease controls plus
missing-sizing refusal even with abundant free space, retained preflight scope,
and a declared byte-budget boundary below the old floor. Fixture byte counts are
arithmetic controls only. Native Windows skips Linux ownership fixtures; a green
Python result does not qualify Windows descendant settlement or representative
Rust workload sizing. Broad builds are unnecessary to test this policy arithmetic.
