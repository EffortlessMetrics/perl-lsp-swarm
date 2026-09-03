# Context: #11639 — deterministic executor-model measurement protocol

## Problem

Controller #9547 (Cargo executor measurement and model-selection) currently combines
protocol design, cross-host execution, raw evidence capture, interpretation, and the
final architecture decision across its child graph. Child BMD-01 (#11639) owns the
first missing instrument: one versioned, deterministic measurement contract plus a
repository-owned harness that can prepare and execute one declared experiment cell
while retaining exact subject, environment, cache, storage, process, and timing
identities — without normalizing away the real behavioral differences between the
current `cargo-safe` paths and the candidate models.

Without this instrument, native-host observation lanes (#11640/#11641) have no
schema to record against, and the #11642 decision has no admitted-evidence format.

## Status: implemented this lane — protocol and harness only

Verified live on 2026-08-31 against `origin/main@f0c15033dd` (head at lane
start) and refreshed against `origin/main@32a40405fb` after that landed
mid-lane (#14538, LSP4IJ claim registry; no overlap with this surface — the
candidate was rebased and the non-Rust inventory regenerated from the merged
tree). Live GitHub state:

| Fact | State when verified |
|---|---|
| #9547 controller | open; forbids implementation PRs against itself |
| #11639 | open; `SPEC_READY · IMPLEMENTABLE_ON_CURRENT_MAIN` (maintainer review pinned `main@cf145b234`, 2026-08-22); `status:blocked` label applied by coderabbitai[bot] at issue creation, before the review verdict; no blocking issue exists |
| #11640 / #11641 | open, depend on this protocol, no open carriers |
| #11642 → #9548 → EXE train (#11647…#11663) | open; `.spec/11661-*` records the whole downstream as `BLOCKED_BY_PREREQUISITE` |
| Open PRs referencing #11639 | none (`gh search prs` empty) |
| Existing measurement symbols | no `build_executor_measurement`, no `build_measurement` module on current main |

## Current-main facts the builder consumes (`main@32a40405fb` post-rebase;
unchanged on `f0c15033dd`)

### The mechanism being measured (NOT changed by this claim)

- `scripts/cargo-safe:5-18` derives devplane, cargo-home, target, build, sccache,
  tmp, and lock roots from ONE directory identity (`DEVPLANE` or default).
- `scripts/cargo-safe` selects its branch from the first command token:
  a direct leaf (`check|clippy|test|...`) performs disk admission and may hold a
  whole-Cargo-process `flock`; an `xtask` first token exports environment and falls
  through with neither control. These are materially different systems and the
  harness represents them as distinct `execution_model` rows.
- `justfile:5` binds `cargo_safe := "./scripts/cargo-safe"`; untouched here.

### House patterns reused (no new envelope)

- Schema-versioned typed records mirroring `.ci/receipts/schemas/*.schema.json`
  with a struct-vs-schema cross-check test: `xtask/src/tasks/session_receipt.rs`.
- Fail-closed receipt doctrine: every field the harness cannot actually verify
  reports `None` / explicit `NotProven`, never a plausible fact.
- Library module + separate `*_tests.rs` / `tests.rs` layout:
  `xtask/src/publication_drift/`, `xtask/src/ci_route_plan/`.
- Deterministic JSON+human projections derived from one typed value.

## One-claim boundary

In scope: `xtask/src/build_measurement/` (typed protocol, provider seams, fixture
runner, projections, validation laws), its schema file
`.ci/receipts/schemas/build-executor-measurement.v1.schema.json`, this `.spec`
packet, and the regenerated non-Rust inventory.

Out of scope (stop boundary, per issue and controller): changing `scripts/cargo-safe`
or any build behavior, selecting a model, running real native-host observation
matrices (#11640/#11641), the decision (#11642), the executor (#9548/#11647…#11663),
caller migration (#9549/#9554/#9559/#9563), or production defaults.

## Why this approach

The controller graph requires the instrument to exist before observations and
before the decision. Representing the current wrapper as the three materially
distinct rows it actually implements (raw private worktree, direct leaf, xtask
environment-only) plus five candidate models keeps decision law 1 (subject
correctness) and decision law 2 (separate private/cache/capacity scopes)
enforceable in type, not prose. All observation lives behind injected providers so
fixture cells prove measurement semantics deterministically on any host, while
real native execution stays with the host lanes.

## Alternatives rejected

- **Single `cargo_safe` execution_model row**: rejected — collapses direct-leaf
  disk/lock controls and xtask environment-only bypass (issue falsifier 1,
  controller falsifier "one row represents all current cargo-safe behavior").
- **Real `cargo`/`sccache` invocations in this PR's tests**: rejected —
  nondeterministic across hosts; the issue mandates fixture cells and injected
  providers; real matrices belong to #11640/#11641.
- **Timing sleeps as the concurrency oracle**: rejected — issue requires
  deterministic barriers.
- **Deriving selected/private/cache/capacity paths from one `DEVPLANE` identity in
  the harness itself**: rejected — reproduces the exact mistake the programme
  documents; path scopes stay independently declared per cell.

## Prior art / duplicates

- `.spec/11661-cargo-executor-command/` (merged via PR #11998) — same family,
  spec-first precedent, downstream BLOCKED_BY_PREREQUISITE receipts.
- `session_receipt.rs` schema mirror + fail-closed doctrine.
- No `build_measurement` module or `build_executor_measurement` contract existed
  prior to this bundle.

## Links

- Issue: [#11639](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11639)
- Parent controller: #9547; programme: #3230 / #10250; campaign: #11869
- Downstream: #11640 / #11641 (observations) → #11642 (decision) → #9548 →
  #11647/#11650/#11653/#11660/#11659/#11661/#11662/#11663 → consumers
  #9549 / #9554 / #9559 / #9563
- Host/process observer context: #11606

## Scope boundary

In scope: this directory's `context.md`, `acceptance.md`, `checklist.md`, the
`xtask::build_measurement` module and its tests, the schema JSON, and the
regenerated inventory doc. Everything else is a named successor's claim.
