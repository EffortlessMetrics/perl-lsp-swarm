# Finite JSON-RPC qualification plan

This manual, unactivated policy qualifies the two original tests in
`xtask/tests/lsp_jsonrpc_dependency_probe.rs` under one original owner. It does
not change canonical gate policy, profiles, storage policy or hosted CI.

The compiler-producing test retains actual offline lock generation, the original
Rust registry/direct-version/checksum validator, neutral compilation, and the
original indirect-taxonomy rejection oracle. Admission additionally requires
native status 101 with the specific E0432 `perl_parser_core` diagnostic. The
second test retains the original invalid-source lock controls. No lock is
checked in or copied from a template: the negative fixture receives bytes from
the successful current native generator, and both measured outputs are frozen
before either check. Each phase receipt binds source, command, cwd, snapshot,
original process, lease and marker. Adapter exit 75 becomes a Rust I/O error.

Both private target and build roots are selected for each fixture command. The
neutral and negative phases share those outputs within their original owner;
the package-local source and manifests remain separate, byte-bound inputs.
The actual integration harness must be captured from Cargo's test-kind artifact
and copied read-only before owner release. A two-name success summary without
all three current phase records cannot produce successful owning evidence.

Source topology before: two separate admitted owning test runs would need two
root bootstrap invocations, two owning Cargo test invocations, one native lock
generator and two native checks (seven total). This batch needs one bootstrap,
one owning Cargo test, one generator and two checks (five total). The generator
is not compiler-producing. These are source counts until actual execution
receipts establish them. The owning integration target may compile all 31
automatic xtask binaries plus the primary binary; no retained main-unit harness
freshness proves that integration footprint. Qualify the complete selected
owning build closure rather than pretending it is only one small executable.

## Remaining actual source obligations

The canonical `.ci/gate-policy.yaml` still declares `compile_all_targets`
(`just check-all-targets`), the routed no-run preparation, package-selected runtime,
DAP preparation and parser preparation. `justfile`'s five-mode all-target
reconciliation is owned by issue 17483; planner/preparation by draft 17460;
Windows launcher/runtime by 17482; lightweight hook by 17480; full-hook false
pass by 17481. This work does not replace those owners or turn its manual plan
into canonical activation.

Additional compiler-producing consumers still include parser
`collapsible_if_occupancy.rs` (exact all-target/incremental Clippy with allow cap
and force-warn), conditional parser `missing_docs_ac_tests.rs`, and dynamic
Cargo fallbacks in xtask/test support. Metadata/tree probes are not compiler
measurements. The obsolete raw `--package perl-lsp` architectural test is
explicitly ignored and must not be counted as active runtime coverage. Existing
selected LSP resolver and main-harness lock/lint qualifications do not prove
this remaining portfolio, the full package-selected runtime, Windows or cold
capacity. Keep the all-target diagnostic status/hit semantics and exact command
coverage intact in any future finite slice.

## Native admission and proof limits

Use existing inherited capacity admission with jobs 1 and incremental disabled,
retained roots and native Rust 1.95. A scoped 14 GiB additional-growth forecast
plus 8 GiB reserve is an unverified bound for all implicit owning binaries,
integration linking and the two fixture variants. Monitor aggregate cgroup
memory, filesystem free bytes, original-owner liveness and ring fingerprints;
settle through that owner's pidfd if pressure or unexpected ring compilation
appears. No cache deletion, profile change or global storage-policy bypass.

Native evidence must distinguish bootstrap/owning/phase invocation counts,
Cargo artifact freshness, measured diagnostics, actual generated closure,
read-only harness provenance, sampled aggregate memory, peak additional volume,
minimum free space and ECHILD owner settlement. Sampled processes are lower
bounds, not exact invocation accounting. Cold and cross-platform proof remain
separate. Perform no public comment, PR, merge or hosted CI activation.
