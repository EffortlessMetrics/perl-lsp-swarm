# Canonical routed preparation: source/control handoff

This slice is source/control proof only. It does not establish full nine-package
runtime success or replace the qualified native resolver, field, lock, JSON-RPC,
and occupancy receipts on their previous exact source commits. No Cargo product
build, hosted CI, policy activation, profile/storage change, or new owner is part
of this slice.

## Contract and topology

The existing unactivated `perllsp_workspace_prepare.py --runtime` first checks
finite row membership and the current source-command mapping, then builds and
captures Cargo's actual perllsp artifact through the existing live owner. It
prepares field diagnostics (one command), five distinct lock diagnostic modes,
JSON-RPC lock generation and its two compiler phases, and parser occupancy
(one command). These are ten invocations: nine compiler frontends and one lock
generator. Only outputs validated by the existing helpers are frozen with their
raw stdout/stderr, native status, actual argv/cwd, source, snapshot and original
owner/lease/marker. Runtime projects their original records to the existing
owning tests; field/lock oracles run again on the original diagnostics, JSON-RPC
preserves its generated-lock/native-phase checks, and occupancy keeps its raw
record protocol. Partial or stale projections never fall back to compilation.

Before: the routed integration run can invoke these native diagnostic producers
from test helpers; count-ratchet can fall back to `cargo xtask` if Cargo's existing
xtask executable projection is absent. After the proposed manual adapter:
known producers run once per finite preparation row; prepared test helpers
replay without compiling; an admitted missing xtask executable projection
refuses the fallback. Production gate policy is unchanged. Both compile and
runtime adapter invocations still build perllsp; no savings or full-graph
invocation count is claimed for this source-only slice.

`canonical-source-map.json` records the exact current 37 row commands, ten
prepared measurement rows, fixed nine packages, 56 matched Rust source hashes,
classified metadata/fake/shell/conditional routes, and both canonical gates:

- `unit_routed_full_build`: perllsp build then scoped `cargo test --locked --tests
  {package_args} --no-run`; existing timeout 1500 seconds and zero retries.
- `unit_routed_full`: perllsp build then scoped `cargo test --locked --tests
  {package_args}`; existing timeout 1500 seconds and one policy retry. The manual
  preparation itself has no retry/adoption after failed measurements.

The manual adapter is fixed to nine packages. It does not replace the planner's
arbitrary `{package_args}`. The helper/selection owner must supply its exact
current route mapping before this contract can be narrowed to that candidate.
No helper source or its already-passed 448 affected xtask tests is duplicated.

## Explicit closure limits

Production mapping deliberately retains six unresolved dispatch groups:
ci_hygiene's missing/stale binary fallback; lsp_ux_smoke's agent-profile fallback;
compare's scanner benches; edge_cases's dynamic test/example commands; the
build/test/doc/dev/parse_rust/bump_version CLI entrypoints; and
publish/clean/dead_code commands. Their presence denies broad routed runtime
before the perllsp build. These are classification gaps, not six demonstrated
active runtime builds. `missing_active_rows: []` means no additional active
producer was demonstrated by this review; it does not certify their absence.
The constructor-pattern scan binds reviewed source and detects newly matching
files; it is not an exhaustive Rust call graph or a subprocess sandbox. Source
hash inventory and catalog membership do not increase the native denominator.

The previous locked no-deps metadata inventory declares 995 integration tests.
Local/default package metadata excludes six DAP test-helper feature targets and
one parser incremental target, but selected xtask dependencies enable parser
incremental. Thus 989 integration targets is a *predicted* unified route, pending
actual Cargo artifacts/features, plus 42 declared library/binary targets after
excluding parser CLI. No 1,031-executable runtime denominator is asserted.

## Bounded plan before any broader native run

1. Receive helper owner's exact command/package/test selection; reconcile actual
   planner route and current manifests with finite preparation rows. Classify
   reachable dynamic dispatch; unresolved commands must refuse, not become
   implicit admission. Keep library/bin/integration coverage distinct.
2. Establish a fresh storage and memory admission for that exact route with the
   storage owner. Workspace observation here: 33,770,192,896 bytes capacity,
   22,338,953,216 bytes free; /tmp is a separate 9,441,349,632-byte filesystem with
   8,776,167,424 bytes free. cgroup memory.max is 17,179,869,184 bytes and sampled
   memory.current 11,013,554,176 bytes. These are observations, not capacity proof.
   The default 40 GiB reserve currently denies the workspace. Previous 12+8 GiB
   leaf admission cannot be reused for hundreds of linked/copied harnesses.
3. Only after closure/capacity permit, use one existing-owner manual adapter
   `--runtime` for the exact source/route, zero preparation retries, existing
   1500-second gate bound, and no automatic hosted trial. Capture all ten actual
   measurements, compiler/test artifacts, unified features and actual named
   runtime results; reconcile expected/observed targets, counts, ignored/filtered
   tests, original-owner liveness/settlement, peak memory and free-space growth.
   A larger inventory is not a reason to repeat the five already-qualified
   native slices. Any selected full route must prove its own current results.

Expected residuals even after these source controls: six dispatch classifications,
exact helper candidate routing, actual current Cargo artifact/feature graph,
existing xtask executable binding and every selected owning runtime oracle,
capacity for the full route, Windows runtime, and canonical planner/policy
activation by its owner. The prior outer 60-minute/78-GiB observation proves
neither deadlock nor ENOSPC and is not a current capacity forecast.

## Completed validation

- Existing-owner Python suite: 217 tests run, 216 passed, one explicit opt-in
  native contract skipped; 53.479 seconds. No Cargo product compilation.
- Perllsp adapter suite: 30 passed; 3.362 seconds.
- Five new routed controls: 5 passed, including ten injected preparation calls
  followed by every helper's frozen runtime replay with zero compiler calls;
  missing each row and changed source/status/owner/argv/cwd refuse.
- Independent read-only reviewer separately passed the same five controls and
  found no blocking frozen replay defect. Reviewer explicitly requires binding
  reviewed shell routes and their transitive scripts before clearing broad
  closure blockers; six unknown groups cannot be cleared alone.
- `git diff --check` passed. Native Cargo, Rust xtask fallback execution, full
  routed runtime, canonical policy activation, and hosted CI were not run.

Read-only remote audit: current main remains
`1f973039d812fe080abddc44e461112d7b95a6fa`, branch base remains
`bf14554128a0491f18a7fb00795effb29aaed3c0`. The previous exact native receipts
remain evidence for their previous subjects only.
