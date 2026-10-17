# Checklist: #11639 — deterministic executor-model measurement protocol

## Step 0 — authority and basis (done)

- [x] Re-verify #9547 controller split and #11639 readiness on live GitHub
      (maintainer `SPEC_READY · IMPLEMENTABLE_ON_CURRENT_MAIN`, 2026-08-22).
- [x] Confirm no open/closed carrier PR for #11639; no rival candidate.
- [x] Pin basis `origin/main@f0c15033dd`, refreshed/rebased to
      `origin/main@32a40405fb` when it landed mid-lane (#14538, no overlap);
      confirm `scripts/cargo-safe` split
      behavior still current (direct leaf vs xtask environment-only).
- [x] Worktree `agent/build-measurement-protocol`, writer key
      `build.measurement_protocol`, single writer.

## Step 1 — spec packet

- [x] `context.md`: problem, status table, current-main facts, claim boundary,
      rejected alternatives, prior art, links, scope.
- [x] `acceptance.md`: predicates 1–13 mapped to proof commands; explicit
      non-claims.
- [x] `checklist.md`: this file.

## Step 2 — falsifiers first (issue implementation order 2)

- [x] F1 false-cache: B builds with A's artifacts; cache stats excellent,
      subject/output identity oracle fails the cell.
- [x] F2 different-subject: `satisfies_row` rejects changed candidate/toolchain/
      profile/features.
- [x] F3 omitted-wait: record without admission/queue/lock wait segment fails
      reconciliation.
- [x] F4 wrong-filesystem: growth path on another volume than the admitted
      free-space check → disk admission refused.
- [x] F5 missing-lock: absent flock primitive → NOT_PROVEN, never locked success.
- [x] F6 zero-work: proof cell with zero selected tests → NOT_PROVEN despite
      exit success.
- [x] F7 host-inheritance: wsl_or_git_bash record cannot satisfy native_windows.
- [x] F8 input-order: canonical identity invariant under list input order.
- [x] F9 stale/concurrent counters: intervening unrelated sccache user →
      `Unattributed` NOT_PROVEN, never a clean delta.
- [x] F10 deterministic render: second render byte-identical; JSON and human
      derive from one record.
- [x] F11 raw vs normalized digests distinct and both present.

## Step 3 — typed schema, providers, runner, projections

- [x] `model.rs`: protocol version, WorkflowClass, ExecutionModel (8), Operation
      (7), HostProfile, SubjectIdentity, PathScope/FilesystemIdentity,
      TimingDecomposition + tolerance, LockObservation, ProcessObservation,
      CacheObservation (baseline/delta/server identity/attribution),
      WorkObservation, MeasurementCell, MeasurementRecord, NotProven.
- [x] `providers.rs`: ClockProvider, FilesystemProvider, LockPrimitiveProvider,
      ProcessObserver, CacheMetricsProvider, CommandRunner seams (fixture
      injectable).
- [x] `runner.rs`: prepare/execute one declared cell; canonical identity;
      reconciliation; admission laws.
- [x] `render.rs`: deterministic JSON + human views from one typed record.
- [x] `mod.rs` wiring; lib.rs registration.

## Step 4 — fixture cells

- [x] Bounded fixture command runner (no real cargo/sccache).
- [x] Deterministic concurrency barrier in the provider contract (no sleeps as
      oracle).

## Step 5 — schema and inventory

- [x] `.ci/receipts/schemas/build-executor-measurement.v1.schema.json` +
      struct-vs-schema cross-check test (`additionalProperties: false`).
- [x] Regenerate `docs/policy/NON_RUST_INVENTORY.md` (--write) and verify
      (--check path via `non-rust check`).

## Step 6 — proof

- [x] `cargo fmt -p xtask -- --check`
- [x] `cargo test -p xtask --all-targets --locked build_measurement` (≥1 match)
- [x] `cargo clippy -p xtask --all-targets --locked -- -D warnings`
- [x] `cargo xtask check-devex-docs`
- [x] `git diff --check`

## Step 7 — reviews and publication

- [x] Review lens 1: false cache/correctness comparison hunt (F1/F9 tests are
      the receipts).
- [x] Review lens 2: omitted-evidence/flattering-measurement hunt (F3/F4/F5/F6).
- [x] Publish PR ready-for-review referencing controller #9547 and successors;
      arm SQUASH auto-merge.

## Stop/transfer rules

Stop when completion would require: selecting a model, touching `cargo-safe`,
real host matrices (#11640/#11641), the decision packet (#11642), or executor
implementation (#9548+). Transfer: #11640/#11641 consume
`build_executor_measurement.v1` records produced by this harness.

## Rollback

Single module + schema + docs; revert commit removes no live behavior. No data
migrations, no callers, no policy changes.
