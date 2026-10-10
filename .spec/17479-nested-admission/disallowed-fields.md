# One generated compiler fixture under the existing owner

This manual, unactivated slice names only the dependency-free
`configured_field_is_rejected_by_clippy` fixture. The finite plan's existing
rows-only request schema remains unchanged. Two fixed rows are added:
`xtask-disallowed-fields-fixture` and `xtask-disallowed-fields-test`. The owning
row requires the fixture row to be present before admission. No arbitrary
manifest, raw argv, temporary crate, path override or second executor is admitted.

The checked-in fixture template is also included by the existing Rust test's
developer path. Preparation copies its manifest, standalone lockfile, source
and Clippy configuration into admitted temp `disallowed-fields-17479`, and
creates private `target` and `build` directories. Preparation runs no compiler,
refuses while a Cargo lease is active, and preserves differing existing files
for diagnosis. Every file is bounded. The owner snapshots template/generated
file subjects, discovered configuration, adapter/interpreter, cwd identity and
both output directory identities before admission; it revalidates them before
and after each leaf. No compiler output is written into the tracked template.

The fixed leaf retains offline, quiet, lib-only, no-deps JSON Clippy and
`-D warnings`. Canonical owner tool identities and resource controls are used;
only this row's cwd/configuration and two output roots select the bound fixture.
An intentional nonzero exit counts as semantic success only with a current
error diagnostic identifying `clippy::disallowed_fields`, the exact manifest
and source, a primary span on the template's field access, and one unsuccessful
terminal compilation. Wrong lint, warning, zero exit, signal, malformed JSON,
missing terminal, stale source or changed configuration/roots refuse.

The Rust owning test selects the native isolated Python adapter only with
complete admission bindings. It receives the actual validated nonzero result
and current raw JSON, then retains its existing failure-plus-lint oracle.
Partial admission cannot fall back to its ordinary developer Cargo command.

## Two separate native qualifications

Use a clean committed worktree and the same explicit native environment as
[the resolver qualification](qualification.md), including the observed trust
directory, installed native toolchain, one job and worktree receipts as temp.
Prepare the fixed inputs without acquiring a compiler lease:

```bash
python3 -I scripts/ci/disallowed_fields_prepare.py --prepare
```

For the direct fixture use the existing owner and explicit manual gate:

```bash
python3 scripts/cargo_admitted.py \
  --nested-plan "$PWD/.spec/17479-nested-admission/disallowed-fields-direct-plan.json" \
  --preflight \
  run -p xtask --bin xtask --locked -- gates \
  --tier merge-gate --gate disallowed_fields_direct \
  --gate-policy .spec/17479-nested-admission/disallowed-fields-gates.yaml \
  --receipt --receipt-path "$TMPDIR/disallowed-fields-direct.json"
```

Regenerate an exact scoped budget from this preflight, repeat preflight with
`--budget-file`, and execute the exact admitted command only after review of
the remaining-growth estimate and host controls. The reviewed warm direct
proposal is 4 GiB additional growth with an 8 GiB reserve. It covers possible
xtask recompilation/linking and the tiny fixture/cache/temp work, without
recharging retained cache already reflected in current free space.

The owning stage is a fresh admission: change the plan filename to
`disallowed-fields-test-plan.json`, gate to `disallowed_fields_owning`, and
receipt filename to `disallowed-fields-owning.json`. Its Cargo row selects
exactly one named xtask main-bin test, serially, with current nonzero named-test
evidence required. Cargo JSON identifies the actual test executable, which is
copied read-only with digest and source/owner receipt before lease release.

The reviewed warm owning proposal is 14 GiB additional growth plus the same
8 GiB reserve: 6 GiB dependencies/feature variants, 3 GiB main test harness and
link scratch, 3 GiB amplification allowance, and 2 GiB temp/evidence/cache.
Both proposals remain unverified forecasts. Use fresh free-space observation,
exact source/scope and a best-effort monitor; the proposed experiment cancels
the bound original owner below 10 GiB free, at 14 GiB aggregate memory against
the 16 GiB cgroup limit, or at 5400s. These are not hard quotas. Scope budgets do
not change global storage policy or profiles.

## Proof boundary

Cheap controls pass for missing configuration, changed access, wrong diagnostic,
corrupt JSON, altered inputs/roots, live ownership and exact owning-test output.
Five assertion-rejected semantic mutants remove fixture revalidation, accept a
wrong diagnostic or zero exit, or omit terminal/access-span proof. No Cargo is
run by those controls. Direct native proof and selected owning-test proof remain
separate; direct success alone proves no executed-test population. Actual direct
and exact owning-harness results, with their separate source identities and
retained first compilation failure, are in
[disallowed-fields-results.md](disallowed-fields-results.md). The owning stage
proves one named test.

This slice does not qualify other compiler fixtures, arbitrary loader variants,
the canonical nine-package population, planner coverage, cold sizing, Windows,
or hosted activation. Existing source/cache are preserved; each stage requires
its own current owner scope and positive kernel settlement proof.
