# Finite compiler-fixture native result

The manual, unactivated Linux slice qualifies one dependency-free generated
`disallowed_fields` fixture under the existing nested-admission owner. Direct
native proof passed on clean source `4c625ef475d0f3d3cb67c87b9ffa30fdc80b96bb`.
The exact owning xtask main-bin test passed on clean repaired source
`a33a353a91fdf52c4dc6dada8e4248fb9a08fe7e`. Installed native Rust 1.95.0,
ordinary profiles, one build job and the explicit trust directory were retained.

Commands and binding rules are in [disallowed-fields.md](disallowed-fields.md).
Historical subjects, actual Cargo artifact, closed owner identities, resources,
raw-evidence digests and measurements are in
[disallowed-fields-evidence.json](disallowed-fields-evidence.json). Raw logs and
manifests remain in the local qualification archive; no live claim registry is
added to the repository.

## Three distinct attempts

| Attempt | Exact source | Result | Executed tests | Total elapsed |
| --- | --- | --- | --- | --- |
| Direct native fixture | `4c625ef4` | Expected current Clippy error accepted | 0 | 632.173s |
| First owning harness | `4c625ef4` | E0382 compilation failure | 0 | 344.109s |
| Repaired owning harness | `a33a353a` | Exact named test and artifact capture pass | 1 | 1520.442s |

The direct run emitted one current error `clippy::disallowed_fields` identifying
the bound manifest/source and primary `range.start` access on line 4, followed
by `build-finished: false`. Its deliberate native failure is accepted only by
the strict semantic oracle. This is compiler evidence, not test execution.
The direct gate took 1005ms; xtask bootstrap took 10m31s.

The first owning attempt found a genuine source defect in the existing
`failed_preparation_prevents_runtime_in_every_tier_and_keeps_backstops` test.
Its inner loop moved `tier` into config, then borrowed it on the next iteration.
The compiler-suggested one-line repair uses `tier: tier.clone()`; independent
review confirms this changes only test code. The failed attempt is preserved.
It proves neither a runtime failure nor deadlock, ENOSPC or failed closure.

The repaired stage compiled the actual main-bin harness and ran exactly
`tasks::check_lint_policy::tests::config::disallowed_fields::configured_field_is_rejected_by_clippy`:
**1 passed, 0 failed, 0 ignored, 4791 filtered out**, runtime **0.93s**.
The gate took **855923ms**; its preceding xtask bootstrap took **11m02s**.
The fixture executes native Clippy through the finite row and retains its
deliberate-failure oracle. Rust captures the inner diagnostic output; that
output is not independently exposed in the owning raw log. The direct raw
diagnostic and current named owning pass remain separate evidence.

Compilation proves the repaired test module now compiles. This selection does
not execute the repaired all-tier negative control; its behavioral coverage
remains pending. Filtered tests are not passing test evidence.

## Artifact and closure

Cargo reported the actual test executable under the private intermediate
`build/debug/deps` root, size **377331808 bytes**, SHA-256
`12aa6a9d2081cd2297881a24072e75eee731917972e4fb6b60bc8bae5197dd1e`.
Its current main-bin manifest/source and test profile were checked. The adapter
captured a read-only copy with matching digest and receipt while the original
owner, lease and marker were live. The owner snapshot was also copied while
those identities matched. No output-path guess substitutes for Cargo evidence.

All three owners separately reported positive kernel `ECHILD (__WALL)`, no
errors or pending bound children, and matching original lease release. Lease
absence was checked after settlement. Product status or observer exit alone
was not used as closure proof. Private cache and the evidence copy are retained.

## Topology and resources

Preparation copies four fixed bounded inputs and creates private output
directories; it runs no compiler. The ordinary developer test retains one
generated-fixture Clippy call. The admitted direct path is existing-owner Cargo
run for xtask, then native fixture Clippy. The admitted owning path is
existing-owner Cargo run for xtask, the exact Cargo-test row, then native fixture
Clippy inside that test. Descendants use the original lease and owner. This
provides truthful command/configuration/output ownership; it does not reduce
the fixture compiler invocation count or establish a timing speedup.

| Attempt | Sampled additional volume growth | Sampled aggregate peak memory | Minimum free bytes |
| --- | --- | --- | --- |
| Direct | 321622016 B (0.300 GiB) | 8771866624 B (8.170 GiB) | 27037138944 |
| First owning failure | 10022912 B (0.009 GiB) | 8504397824 B (7.920 GiB) | 27348656128 |
| Repaired owning pass | 754626560 B (0.703 GiB) | 10172014592 B (9.474 GiB) | 26593902592 |

No cancellation, observer error or memory-event counter increase occurred.
Sampling was best effort at 2s, reflects volume/cgroup aggregate rather than
compiler RSS, includes evidence-copy growth and is not a hard quota. These
warm attempts have different preparation costs and are not a controlled
benchmark or cold aggregate sizing proof.

Each stage used a fresh independently reviewed exact scoped estimate: direct
4 GiB additional growth plus 8 GiB reserve; owning 14 GiB additional growth plus
8 GiB reserve. Existing cache was already reflected in initial free space.
Default 40 GiB admission still denied. Supported scoped budgets changed no
global storage policy or profiles; forecasts had `basis_verified: false` before
launch and do not qualify other workloads.

Cheap proof passes: complete owner suite **156 run / 155 passed / 1 opt-in
product-build skip**, including 17 fixture controls and 35 nested controls.
Five semantic mutants were rejected by assertions, with no instrumentation
errors. Formatting and diff checks passed. Controls cover changed inputs,
configuration/output roots, partial admission, wrong diagnostics/terminals,
zero/ignored/wrong-name output and absent or invalid artifact capture.

## Remaining boundary

The earlier two-resolver proof remains at tested source
`b2f4ea124fa0c5aa3e436d164f634a23b2fe861e`, in its separate
[results](qualification-results.md) and [evidence](qualification-evidence.json).
Its two named behavioral passes are not combined with this fixture's one test.

Full canonical nine-package preparation/runtime, fresh genuine planner scope,
other compiler fixtures and arbitrary loader variants, Windows/macOS, cold
aggregate disk/RAM sizing, execution of the repaired all-tier control and
hosted activation remain NOT_PROVEN. The 995 integration target declarations
plus 43 eligible lib/bin declarations are inventories, not executed tests.

Storage landed at main `2f33ac783b5bb0da7c8bc12644bc47d565c21c7c` after this
source was prepared. The active experiment was not restacked. Later composition
must reconcile current owner/main bytes and repeat affected proof. No PR-head
update, Ready, merge or hosted CI dispatch occurred.
