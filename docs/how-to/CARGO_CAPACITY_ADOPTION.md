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
| `scripts/cargo-admitted`, direct Python, admitted just recipes | Python percent policy defaulted to 40 GiB and 85% used; environment byte-budget also defaulted/clamped reserve to 40 GiB | Default percent behavior is retained until caller migration. Only explicitly selected byte-budget requires its own reserve and removes the clamp; scope-bound budget remains preferred. `cargo_cohort.py` consumes an already held admission lease rather than choosing a new capacity policy. |
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
   This uses the retained compatibility default and may pass or refuse capacity;
   either result emits scope without allocating or launching Cargo. Capture the emitted scope, rather than reconstructing it.
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

Compatibility `percent` keeps its default 40 GiB reserve/85% used ceiling.
Explicit environment `byte-budget` requires positive finite `MIN_FREE_GB`, total
growth and its evidence reference. These remain operator host policies, not scope-bound measured
profiles. No automatic small-workload sizing is supplied by this change.

## Regression evidence ceiling

`python -m unittest discover -s scripts/tests -p 'test_cargo_admitted*.py'`
exercises existing budget scope, destination, recheck and lease controls plus
ordinary default preflight success and insufficient-space refusal, explicit
byte-budget missing-reserve refusal, and a declared boundary below the old floor. Fixture byte counts are
arithmetic controls only. Native Windows skips Linux ownership fixtures; a green
Python result does not qualify Windows descendant settlement or representative
Rust workload sizing. Broad builds are unnecessary to test this policy arithmetic.


## Caller migration and exact remediation

No ordinary caller found on the inspected main automatically supplies a **measured**
budget. Removing the default now would replace normal commands with manual sizing,
so default retirement is held. A budget file generator alone is not measured sizing.

| Caller | Automatic measured sizing today | Current behavior and next action |
| --- | --- | --- |
| Direct `cargo-admitted`/Python, `just bench-ripr-quality` and `bench-ripr-e1` | None | Retained percent default; status 75 `insufficient disk headroom at PATH free_bytes=... required_free_bytes=... shortfall_bytes=... used_pct=...`. Use a qualified exact budget only for a measured closure; adapter owner must automate selecting/revalidating it before default retirement. |
| Legacy just agent/package recipes through `cargo_safe`; hooks that bootstrap xtask or choose `pr-fast` | None | Legacy guard remains; do not tune it or skip proof. #9567/#17484/#17825/#17866/#17899 own projection/selection migration. |
| Installer carrier `531fc5d7b98379326b9f01f8de92af03ea67c8b4`, #17480/#17861 | None | Direct admitted `perl-ci-hygiene` avoids heavyweight xtask, but separate install/check budgets remain operator-qualified. Preserve original writer and interrupted leases. |
| `scripts/tests/test_lsp_revision_build.py` opt-in fixture | Automatic file creation, operator declaration only | 8 GiB reserve/1 GiB growth are fixture estimates, not measured general sizing. Preserve its preflight contract; it is outside the admission-suite glob. |
| `scripts/tests/qualify_admitted_clippy.py` | Automatic file creation, fixture declaration only | 4 GiB reserve/2 GiB growth apply only to its offline dependency-free fixture. |
| Footprint-worker `perl-test-must`/xtask experiments | No | Explicit scope-specific declarations and measurements; evidence below does not qualify ordinary callers automatically. |
| Explicit environment `byte-budget` without reserve | No | Status 75: `byte-budget requires an explicit reserve in MIN_FREE_GB; prefer --preflight then --budget-file for a scope-bound qualified reserve and total additional growth`. Supply the reviewed reserve plus growth/evidence, or the exact scoped file. Do not guess values. |

Interim worker evidence, not a universal allowance: native Windows SZ-Corsair-RS,
base `4cc7960e61`, installed Rust/Cargo 1.95.0, `test -p perl-test-must --lib
--profile agent --locked --offline`, jobs 2 and incremental disabled. Twelve tests
passed; cold/warm elapsed 5.560/2.949 seconds, sampled file-storage peak
3,667,243/3,629,474 bytes, cold own-tree peak RSS 280,207,360 bytes. Task-private
H: target/build/temp with existing C: Cargo home; declared 40 GiB reserve + 1 GiB
growth, `basis_verified:false`. C: had 123.6 GiB free but 96.68% used, so the
percentage heuristic refused. These are worker-reported sampled observations,
not exact allocated high-water marks or a proven portable bound. Evidence is
retained under `H:/Temp/task6-build-footprint`. Edit/repeat, concurrent envelope,
xtask comparison and Windows native descendant settlement remain unqualified.

## Why native Windows skips tests

At this candidate's 136-test run, 81 skips were intentional: 30 staged-Linux-Clippy,
16 Linux process-local ownership, 14 Linux kernel-child ownership, seven Linux
route controls, seven Linux ownership, six POSIX-only fixtures (open-directory,
symlink replacement, executable, dangling link, non-UTF-8 path and trailing space),
and one opt-in real offline Cargo A/B/A test. The other 55 passed. Junction tests
can additionally skip where host privileges are unavailable; report actual counts.

Run the same suite on supported native Linux/Python 3.11+ with child-table/pidfd
capabilities, and exercise the alternate discovery profile's missing-child-table,
detached TERM-ignore, ambiguity, unrelated-process preservation, ECHILD,
original-lease release and repeat controls. Enable `CARGO_ADMITTED_REAL_BUILD_TEST=1`
only after declaring the small offline fixture's installed toolchain and resource
needs. Run the opt-in revision fixture separately when relevant. Linux results do
not qualify Windows process-tree ownership (#17482); its native launcher authority
and cancellation/settlement/release proof remain a separate adoption prerequisite.
