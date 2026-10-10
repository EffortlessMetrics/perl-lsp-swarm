# #17479 / #17483: measured package-context cache reuse

**Reuse is proven for the exact repaired owning-to-control transition on source `4bb59c8e24a3221141f64b71a4669b0f318b7d52`, using the retained private Linux Rust/Cargo 1.95.0 roots.** Both selected named tests passed. The control's outer Cargo bootstrap positively reported all three ring units up to date; nested Cargo reported all 384 artifacts fresh, including ring and the actual xtask test harness. Ring fingerprint files and the rlib's full identity/hash were unchanged before and after control. No residual mismatch was observed in this experiment.

## Composition and source

Main advanced from `20074ba2b70e8b96252b52b827bd7df464639326` to landed [#17506](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17506), `95a722963611dfeb02dc119cea5fbf4851ba4af9`. Its owning changes add bounded Linux Clippy admission, direct tool/configuration identity checks, original marker binding, process-tree settlement and native controls. This branch already contained the underlying Clippy machinery with nested and builtin Cargo ownership extensions. The reviewed merge preserved those extensions and incorporated the landed `marker_identity` settlement field and both replacement-marker tests. The only other resulting source-tree delta from the preceding candidate was the landed panic-identity registry data. The branch's existing workflow bytes remained unchanged.

The composed admission suite ran 176 controls: 175 passed and one opt-in product-build test skipped, independently repeated. The finite 17-field package-context normalization remains unchanged from `6f4a2058928d33386dc0cf3f3939f0fc57b1475b`. Fresh main remained `95a722963…` at evidence preparation. This proves the selected admission/cache slice of that composition; full current-main workload qualification remains pending.

## Native results

| Phase | Exact named test | Test runtime | Whole admitted phase | Sampled aggregate memory peak | Observed volume growth |
|---|---|---:|---:|---:|---:|
| Establish cache / owning | `the_two_rows_jointly_cover_every_borrowed_guard_discard` | 0.95 s | 1,217.734 s | 10.955 GiB | 387,608,576 bytes |
| Change to control | `failed_preparation_prevents_runtime_in_every_tier_and_keeps_backstops` | 8.34 s | 14.203 s | 7.249 GiB | 381,677,568 bytes |

Each phase executed exactly one named test: one passed, zero failed/ignored, 4,791 filtered. The control's 16 internal tier/failure cases do not count as 16 tests. Owning nested Cargo emitted 384 current artifact records, 383 fresh; only the xtask main-bin test harness was rebuilt. Ring was fresh. Control emitted 384/384 fresh records, including that same harness. The outer control bootstrap completed in 0.26 seconds.

The first outer bootstrap needed a ring rebuild from retained post-cancellation state. Its captured dirty reason was `UnitDependencyInfoChanged`; its dev build took 11m17s. It then established the library's dependency on run-buildscript fingerprint `5294891653738825643`. The subsequent owning nested invocation, control outer invocation and control nested invocation reused the established ring state. **This new trace does not establish why the earlier cancelled control originally dirtied ring.** The earlier run-buildscript JSON and actual original nested process environment remain unavailable.

Different outer caller metadata was supplied for the owning and control phases. Sampled resource-bearing Cargo environments lacked the watched caller package fields. The control additionally captured Cargo replacing itself with xtask at the same PID/start: package context changed from absent to reconstructed xtask metadata. The Python adapter inherited xtask metadata, and its leaf Cargo again lacked caller metadata. The first observer did not record the same-PID exec transition; its adapter observation supports only the narrower claim. The bound plans differed only in `request` and `request_subject`; source, tools, configuration, features/profile inputs and resource roots agreed.

Both phases bound live snapshots to their original owner PID/start, lease and unique marker, checking identities before and after copying. Actual Cargo-reported harnesses were copied read-only before release. Their original artifact path/full identity/hash matched across the mode change, SHA-256 `cba366bdbb19caa89b61af943dbbcb15adf8b34974a0500f3030d924d90ab2de` (378,816,488 bytes). Each original owner separately proved `kernel ECHILD (__WALL)`, zero pending/error entries, and matching original marker/lease release. Neither phase was cancelled; memory-event counters stayed unchanged.

## Admission and scope limits

Each exact phase used a fresh scope-bound budget: conservative unverified 14 GiB additional growth and an 8 GiB reserve, one job, incremental compilation disabled. Existing caches were charged to observed free space. Monitoring sampled at 0.25 seconds and would cancel through the original owner's pidfd below 10 GiB free, at 14 GiB aggregate memory, or at 5,400 seconds. The control also would cancel attributable ring compilation after cache establishment. Observed minimum free space was 24.043 GiB in owning and 23.687 GiB in control. These are best-effort aggregate observations, not process RSS, quotas or cold/portfolio sizing.

Build obligations and invocation topology are unchanged: owning uses outer Cargo bootstrap, exact nested Cargo test and its fixed Clippy fixture; control uses outer bootstrap and exact nested Cargo test, plus tool/version probes inside the selected control. Native compilation was reused in the measured control. Its retained harness copy and logs still consumed storage. The two phase timings are not a before/after speedup comparison.

[#17483](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17483) can use this as one measured compatibility cell for package-context normalization. General five-mode compatibility, cold/warm aggregate sizing, the other five lock measurements, full preparation/runtime and Windows/macOS remain pending. Profiles and storage policy were not changed; caches were retained. No public comment, PR, merge or hosted workflow was activated.

The [evidence packet](cache-reuse-evidence.json) records current Cargo artifacts, positive freshness traces, before/after unit bytes and rlib identity, actual process contexts, live snapshot and artifact provenance, exact results, original-owner closure and raw-file hashes. The earlier [context analysis](ring-context-results.md) and [cancelled-attempt evidence](lock-union-evidence.json) retain their historical limitations.
