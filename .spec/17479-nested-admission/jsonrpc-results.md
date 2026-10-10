# JSON-RPC finite compiler proof

Native source `17b67e88d989de813a4d5b535a6f35bdba72a327` passed both original owning integration tests: **2 passed,
0 failed, 0 ignored, 0 filtered**, 18.85 seconds in libtest. Whole admission took
23.595456 seconds; the gate took 21759 ms. Actual phases were
`generate-lockfile --offline` (0), neutral `check --quiet --locked --offline` (0),
and rejected check (101), with native `E0432` for the unresolved import of
`perl_parser_core`. Both manifests retain the original reviewed direct versions
and checksums. The actual generated lock is recorded in the evidence; no lock
seed is checked in. Both target and build roots are isolated per fixture mode.

Before/after invocation topology: two separate supported owning admissions need
2 bootstraps + 2 Cargo tests + 1 generator + 2 checks = **7**. This batch uses
1 bootstrap + 1 Cargo test + 1 generator + 2 checks = **5**. Compiler measurements
are unchanged. Across actual attempts the count is **4 + 5 + 5 = 14**, excluding
read-only metadata and tool-version probes. Sampled process counts are lower
bounds. This is finite batching, not full canonical runtime certification.

The first attempt on `77b8c245fe4e5d24c6e80e09cca43a48c8e18223` was **1 passed,
1 failed**: actual generation and neutral compilation succeeded, but a
103443-byte phase receipt exceeded the metadata reader. Negative compilation
was not launched. The bounded repair changes only phase-receipt reading;
plan and budget readers retain their ordinary 64 KiB ceiling.

The second attempt on `e054b2e78e2e2b2a1f467ddcb564427b6083cd1e` was **1 passed,
1 failed**: Cargo reused the neutral same-package unit across shared output
roots and the negative check returned 0. The stronger oracle refused that
false success. The final repair gives each mode separate target and build roots.
Original manifests, wrapper bytes, compiler commands and Rust oracles are
retained. Both failed traces and owner settlements are in the evidence.

The first owning build emitted 415 artifacts, 384 fresh, including **31 binary
targets (30 automatic plus main)**. Both later owning builds retained **415/415
fresh**, including the actual integration test-kind artifact. The winning
read-only harness copy is bound to the original live owner and immutable plan;
SHA-256 `1f2112e955eee377b229ee69cf19c27de3eefa0dfd0fb65d57b2f1500891b0f4`. Ring fingerprints and rlib identity
were unchanged. No sampled ring compiler appeared. All three original owners
proved `kernel ECHILD (__WALL)` and released matching lease/marker identities,
with no cancellation, pending descendant or settlement error. `memory.events`
was unchanged on each attempt.

Winning resource observations: sampled peak aggregate memory
**10557952000 bytes (9.832859 GiB)**;
peak additional volume **114130944 bytes**;
minimum free **22929928192 bytes (21.355160 GiB)**.
The first attempt measured 2008567808 bytes additional volume and 9.735188 GiB
aggregate memory; the second measured 733184 bytes and 9.610218 GiB. Forecasts
were explicitly unverified: initial 14 GiB growth + 8 GiB reserve, then warm
10 GiB + 8 GiB after complete integration build. Monitoring remained free<10 GiB,
aggregate memory>=14 GiB or wall>=5400 s cancellation through the original pidfd.
No policy bypass, profile change, cache deletion or cold-sizing claim.

Cheap controls: **201 run, 200 passed, one opt-in skip**. Independent review
repeated 12 JSON-RPC and 12 builtin-owner controls. Seven weakened guards failed
assertions with zero instrument errors. Independent review verified the final
native packet and accurate distinction of both failed attempts.

Main advanced to `1f973039d812fe080abddc44e461112d7b95a6fa`. The exact upstream
owner function and complete builtin control file were incorporated before native
experiments, preserving controlling-terminal/session behavior and protecting
the shared caller group during cancellation. This is owner-delta composition,
not a claim that every current-main file was merged. A real native read-only
`ci-scope` observation against genuine base `03efc00774b7b6b5631efa42f05e7701ce245301`
selects the nine package closure; it is recorded as planning evidence, not a
runtime result. Its metadata probe and a separate read-only inventory are
excluded from the experiment's build invocation counts.

Remaining qualification: actual canonical five-mode all-target reconciliation
(17483), preparation/planner owners (17460), full DAP/parser preparation proofs,
the nine-package routed no-run/runtime closure, parser all-target diagnostic
occupancy and conditional docs, dynamic Cargo fallbacks, Windows (17482), hooks
(17480/17481), and cold capacity. Obsolete explicitly ignored architectural Cargo
calls and noncompiler metadata/tree probes do not count as active compiler tests.
No public comment, PR, merge or hosted CI activation was performed. The manual
policy stays unactivated; expensive final CI remains a candidate-ready step.

Evidence SHA-256: `6a5acbd6718daa1aec3bd121f1be558d92cb07b28001d277d5ef00e3878b9efc`.
Source patch SHA-256: `0ee63e73f13372c801519f7f35d1d22679d61e785f6d4b0bf02c27804742dbfb`.
Raw current and failed stage streams, generated locks, resource samples, native
artifacts, exact commands, snapshots and source/control hashes are preserved in
`jsonrpc-evidence.json` and its retained manifests.
