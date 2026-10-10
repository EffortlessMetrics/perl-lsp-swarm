# Perl-token Cargo profile observations — 2026-10-10

Exact source/admission candidate: 4e0cae40cc5390cddf438f51a565f4f04cb43d75,
fetched from recovery/cargo-storage-17477-cloud and checked out in three private
detached experiment worktrees. Rust/Cargo 1.95.0, Linux x86_64, jobs=2,
incremental forced off, line-tables-only debug, default features, offline.

Independent scoped review found no actionable defect for this use: the explicit
budget binds host/cwd/worktree/paths/argv/jobs/environment; default floor remains;
post-lease recheck and identity-guarded release remain. This is not full candidate
integration review. Python discovery: 54 tests, OK, one opt-in real-build fixture
skipped. Canonical xtask policy qualification remains pending with its owner.

Command in each cell:

```bash
scripts/cargo-admitted --budget-file <captured-exact-budget.json> build -p perl-token --lib --profile agent --locked --offline --message-format=json --timings -vv
```

Each request first refused under default 40GiB policy, then passed its separately
captured scoped declaration: reserve16GiB + expected additional growth1GiB,
basis_verified:false. Basis: dependency-free library, no build.rs, 91840 bytes of
Rust source, offline/no downloads, exact prior pilot672KiB; 1GiB is a conservative
operator envelope, not a measured forecast. All destinations were checked.
Measurement subprocess had a sampled1GiB-growth/16GiB-reserve stop and120s ceiling.
No stop fired. These bounds qualify only this exact leaf experiment.

| Profile overrides | Phase | Command wall s | Cargo unit s | Peak summed tree RSS MiB | Sampled volume peak growth KiB | New/fresh units |
|---|---|---:|---:|---:|---:|---|
| opt1 / CGU16 | artifact-cold | 2.118 | 0.84 | 146.0 | 716 | 1/0 |
| opt1 / CGU16 | unchanged warm | 0.617 | 0 | 84.5 | 48 | 0/1 |
| opt1 / CGU16 | controlled edit | 0.938 | 0.35 | 145.3 | 684 | 1/0 |
| opt0 / CGU16 | artifact-cold | 0.794 | 0.28 | 143.4 | 776 | 1/0 |
| opt0 / CGU16 | unchanged warm | 0.493 | 0 | 83.1 | 40 | 0/1 |
| opt0 / CGU16 | controlled edit | 1.550 | 1.01 | 142.8 | 496 | 1/0 |
| opt1 / CGU64 | artifact-cold | 2.126 | 0.44 | 145.5 | 700 | 1/0 |
| opt1 / CGU64 | unchanged warm | 0.601 | 0 | 78.6 | 48 | 0/1 |
| opt1 / CGU64 | controlled edit | 1.838 | 1.10 | 146.4 | 392 | 1/0 |

All nine exit0. Exactly one rustc invocation per cold/edit run and none per warm
run. Verbose commands confirm opt1/CGU16, opt0/CGU16 (rustc omits the default
opt0 flag), opt1/CGU64, line tables and no incremental argument. Cargo HTML timings
retain phase data. Controlled edit appends the same inert comment to span.rs in
each worktree; source hashes agree across matching phases. No code behavior claim.
Build target+intermediate allocated bytes after all runs: A1175552, B1 1335296,
B2 1175552. Existing shared Cargo cache is included in raw before/after resource
accounting, not counted as new build footprint. Timing reports themselves add
retained output bytes. Source/worktree allocation occurred before build timing.

Measurement limitations: one observation per cell/phase; cells executed A,B1,B2
sequentially; OS caches and host scheduling are uncontrolled. Cold means artifact
cold, with existing dependency/toolchain caches. RSS is summed sampled process-tree
resident memory, including wrapper/compiler processes; shared pages may be counted
more than once, and peaks between10ms samples may be missed. Volume growth is
sampled statvfs and includes evidence logs/unrelated host effects; it is not exact
peak allocated package bytes. No GNU time is installed. No test/dev dependencies,
runtime performance, release, linker executable or full-core proof was run.

Conclusion: no consistent useful winner and no profile patch justified. Opt0's
cold observation improves but its edit observation regresses; CGU64 has no stable
advantage. Do not extrapolate subsecond leaf codegen to high-volume LSP work.
Strong positive evidence is exact warm reuse: zero compiler invocations in every
unchanged repeat, despite wrapper/timing latency. Broader measurements need their
own growth envelope and #9178/#11639 coordination; no core escalation authorized
by these observations. Release/bench/dist remain unchanged. No hosted CI.

Resources: base checkout unchanged. Experiment worktrees profile-a/profile-b1/
profile-b2 and their private target/build resources retained with reason: exact
source-edit/cache/timing evidence; no consumer remains and all leases released.
Evidence preserved in the accompanying raw logs/scopes/JSON/HTML and instrument.
No cleanup, repository profile patch, branch publication or PR update performed.


## Durable provenance and reproduction

This note follows [metrics provenance](../project/METRICS_PROVENANCE.md).
Wall times and compiler counts are measured from the captured subprocess logs;
RSS/volume peaks are sampled measurements with the limits stated above. MiB/KiB
and rounded seconds are derived from raw byte/second observations. The1GiB
growth bound is estimated; the prior672KiB pilot is reported by the admission
owner. Performance-selection confidence is low; no global toggle is accepted.

Evidence archive: [perllsp-profile-observations.zip](https://chatgpt.com/api/library/files/libfile_fdbd5cf8ddec8191bf6a25e516544495/download)
(131361 bytes), SHA256
`0bde5941e71523e4b4bd99d1b32c92291e1e522de67bc128252c169b0e16c9c2`.
The archive contains REPORT.md, results.json, SHA256SUMS.json, nine budget files,
initial/admitted preflight logs, stdout JSON/stderr logs, Cargo timing HTML,
measurement script and storage-suite log. SHA256SUMS.json hashes each other
file; the outer archive hash binds the manifest too. This repo note extends the
archived report with durable publication/reproduction references.

Raw manifest SHA256: `4d6903b0ee0eec2d3d9998aadf7881bf251a0bc16bda7ab8e0a663f3c5a25215`.

Activate the existing cloud toolchain without installation:

```bash
export CARGO_HOME=/workspace/.cloud-tools/cargo
export RUSTUP_HOME=/workspace/.cloud-tools/rustup
export RUSTUP_AUTO_INSTALL=0
. /workspace/.cloud-tools/cargo/env
export DEVPLANE=/workspace/profile-devplane
export TMPDIR=/workspace/profile-temp
export CARGO_BUILD_JOBS=2
```

A/B1/B2 use these explicit environment pairs, respectively:

```text
CARGO_PROFILE_AGENT_OPT_LEVEL=1 CARGO_PROFILE_AGENT_CODEGEN_UNITS=16
CARGO_PROFILE_AGENT_OPT_LEVEL=0 CARGO_PROFILE_AGENT_CODEGEN_UNITS=16
CARGO_PROFILE_AGENT_OPT_LEVEL=1 CARGO_PROFILE_AGENT_CODEGEN_UNITS=64
```

The instrument removed inherited capacity and compiler-wrapper/output overrides.
For each phase it called the Bash entrypoint with `--preflight` and the exact
build argv, retained the returned scope, wrote a caller-owned budget, then called
`--preflight --budget-file <file>` and finally the build command shown above.
Each worktree retained its own target and build directory. Matching phases have
identical source bytes across variants. The edit appends
`// Controlled profile experiment: semantics-preserving source edit.`
to crates/perl-token/src/span.rs. This triggers compilation without changing
behavior. Budget scope does not attest source/cache/config identity; the sizing
basis was manually rechecked for the inert edit. Do not replay these policies on
another host/worktree/command or use them as general host admission.

## Programme boundaries and next workload

These observations supplement [#9178](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9178)
and [#11639](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11639).
They do not complete either programme, select a compiler-cache/executor model,
or validate their full compatibility/concurrency matrix. The leaf command did
not invoke sccache, downloads, build scripts, tests or an executable linker.

Next representative workload: `build -p perl-lsp-rs-core --lib --profile agent
--locked --offline --message-format=json --timings -vv`, default features, with
fresh separately qualified storage/memory bounds and the accepted measurement
protocol. Repeat counterbalanced trials and measure codegen versus setup costs;
add matched runtime/correctness proof before selecting opt-level changes. Do not
broaden this leaf's budget to core or test dev-dependencies. Keep release/bench/
dist settings and required qualification intact.

Publication is an evidence-only branch based on
`ac383369c40494bd873d9657e8c881b7e8792000`; it does not import the storage
candidate or modify Cargo settings/workflows. No hosted CI was requested.
