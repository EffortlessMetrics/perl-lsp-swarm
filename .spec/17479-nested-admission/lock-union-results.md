# One lock-union native qualification

The manual, unactivated Linux slice qualifies the existing borrowed-guard union
measurement on clean source `d7791bfc6150e52d6ca6c8f79807985964bb956b`.
Direct diagnostic proof and the exact owning test both passed. The optional
repaired preparation control remains **NOT_PROVEN**: its warm-bootstrap premise
failed and the original owner was cancelled before the gate started.

Commands, fixed inputs and exclusions are in [lock-union.md](lock-union.md).
Historical source, actual Cargo artifacts, closed owners, samples and raw-file
digests are in [lock-union-evidence.json](lock-union-evidence.json). Raw logs and
immutable evidence copies remain in `/tmp/17479-finite-qualification` and the
private receipts/cache roots. No active ownership registry is tracked.

| Attempt | Source | Result | Executed tests | Total elapsed |
| --- | --- | --- | --- | --- |
| Initial direct | `41d45437` | Changed Cargo.lock refused | 0 | 666.212s |
| Repaired direct | `d7791bfc` | Four current warnings and successful terminal | 0 | 4.001s |
| Owning union | `d7791bfc` | Exact named test and actual harness capture passed | 1 | 838.253s |
| Optional preparation control | `d7791bfc` | Unexpected bootstrap compiler work cancelled | 0 | 6.007s |

The initial direct run's post-invocation input check found that Cargo had added
its generated header and serialized dependency lists over multiple lines.
The original and native rewritten locks parse to identical TOML: package
versions, checksums and resolved closure are unchanged. An independent
regression failed on the old bytes, then passed with the captured canonical
bytes (SHA-256 `ff2a47a9ba4466ff072f8b5c2e61193fc9d3dee0c659a1eb27612f1bc309a5d3`).
Only the template and regression changed in the repair. Exact byte/identity
revalidation and dependency-provenance validation were retained. That failed
attempt proves refusal and closure; its discarded inner output establishes
neither successful compilation nor the four diagnostics.

The repaired direct gate took **1005ms**. It emitted exactly four current
warnings: `let_underscore_lock` at lines **5/6**, and
`clippy::let_underscore_lock` at lines **10/11**, with the expected current
manifest, source and span text, then `build-finished: true`. All **10** Cargo
artifact records were `fresh: true`. This is an admitted cached invocation and
diagnostic replay, not evidence of new compiler work or test execution.
Its hash-verified snapshot was copied after settlement; no manual live-copy
claim is made for this short stage.

The owning stage rebuilt the actual xtask main-bin test harness and ran exactly
`tasks::check_lint_policy::tests::lock_partition::the_two_rows_jointly_cover_every_borrowed_guard_discard`:
**1 passed, 0 failed, 0 ignored, 4791 filtered out**, runtime **0.96s**.
The gate took **835915ms**. Existing Rust assertions retain both lint ownership,
borrowed-guard coverage, and held-guard silence. The inner diagnostic stream is
captured by the Rust test and is not separately visible in this owning log;
the direct diagnostic proof remains a separate measurement.

Cargo reported the rebuilt (`fresh: false`) main-bin executable under the
private `build/debug/deps` root, **378816488 bytes**, SHA-256
`cba366bdbb19caa89b61af943dbbcb15adf8b34974a0500f3030d924d90ab2de`.
The adapter checked current manifest/source, test profile and actual executable,
then preserved a mode-0444 copy under original live owner **36421**, matching
lease and marker. The source-bound snapshot was also copied while those
identities and kernel process start matched; snapshot SHA-256
`53537c73d8196763137969d0269089e5ac9edfc08e3533cb1b006450a85c4ccf`.

The optional control received fresh admission with a reviewed **2 GiB additional
plus 8 GiB reserve** warm estimate based on that unchanged harness, source,
native tools, profile/features and resource roots. The existing-owner xtask
bootstrap unexpectedly began compiling **ring 0.17.14** before the gate.
The observer distinguished actual `rustc --crate-name` compilation from version
probes and cancelled through the original owner's pidfd. No gate log or receipt
was produced and **zero tests ran**. This demonstrates that a warm test harness
does not establish warm bootstrap reuse. Its cause is not established here;
mode/cache reconciliation is a precise handoff to #17483. No retry, different
executor, storage-policy change or profile change was used to finish it.

All four attempts separately reported positive **kernel ECHILD (__WALL)**,
matching original lease/marker release, and no errors or pending bound children.
The cancelled stage reaped Cargo and rustc with exit -15. Marker absence and
final lease absence were checked; product/observer status alone was not closure
proof. The optional control's retained snapshot was copied after settlement;
its evidence is cancellation/closure, not control behavior.

| Attempt | Sampled additional volume growth | Sampled aggregate peak memory | Minimum free bytes |
| --- | --- | --- | --- |
| Initial direct refusal | 215252992 B (0.200 GiB) | 9552027648 B (8.896 GiB) | 26378145792 |
| Repaired direct | 49152 B | 6158557184 B (5.736 GiB) | 26579521536 |
| Owning union | 382001152 B (0.356 GiB) | 10931298304 B (10.181 GiB) | 26197516288 |
| Cancelled control | 0 B sampled | 6718849024 B (6.257 GiB) | 26197516288 |

Sampling was best effort at 2s, measures volume/cgroup aggregate including
cache/copy growth, and supplies neither an upper bound nor compiler RSS. There
were no observer errors or memory-event counter increases. Only the optional
control was cancelled. These attempts have different preparation costs and
are not a controlled timing benchmark or cold/aggregate sizing proof.

Default **40 GiB** admission still denied. Supported fresh exact scoped budgets
used **4 GiB additional + 8 GiB reserve** for direct and **14 GiB additional +
8 GiB reserve** for owning; existing caches were already charged to free space.
The original native 1.95.0 pin, one build job and ordinary profiles were retained.
Forecasts were explicitly unverified before launch. Cancellation limits stayed
14 GiB aggregate memory, below 10 GiB free, or 5400s; existing gate timeout 3600s.

Preparation runs **zero Cargo/compiler invocations**, copying four fixed files
and creating private output roots. Before this slice, the developer owning test
used one raw Clippy invocation in a newly generated temp fixture. Its ordinary
path remains unchanged. The admitted direct topology is existing-owner Cargo
run for xtask, then the fixed Clippy row (**2 Cargo invocations**). The admitted
owning topology is that bootstrap, the exact Cargo-test row, then the retained
Clippy measurement (**3 Cargo invocations**). All descendants reuse the original
owner and lease. This binds inputs/configuration/closure/output ownership and
permits fixed-root reuse; it does not eliminate the required compiler measurement
or prove an invocation-count or elapsed-time speedup.

Cheap proof on the repaired source: **170 run, 169 passed, 1 opt-in product-build
skip**, including 14 lock controls, 17 prior fixture controls and 35 nested
controls. Six semantic mutants were assertion-rejected with zero instrumentation
errors before the canonical-only repair; their production adapter/owner bytes
are unchanged by that repair. Syntax, pinned Rust formatting and diff checks
passed. Independent review confirmed the repair, direct replay, exact owning
pass/artifact/closure, and truthful cancelled-control boundary.

Five other lock measurements, parser collapsible-if, the LSP JSON-RPC probe,
full canonical preparation/runtime, Windows/macOS, cold sizing, and hosted
activation remain excluded. The earlier b2 two-resolver and 4c/a33
disallowed-fields proofs retain their original sources and denominators; this
new result does not requalify them. Main was observed at
`2f33ac783b5bb0da7c8bc12644bc47d565c21c7c`; this branch has not been restacked or
claimed as qualification of the full current-main composition. Caches and
immutable evidence are retained; no native compiler or owner lease remains.

Fresh evidence publication observed main at `20074ba2b70e8b96252b52b827bd7df464639326`
(#17508, panic-identity data only). Workflow and touched implementation bytes
were unchanged by that advance. The exact-source proof and composition exclusion
remain unchanged; no restack was performed.
