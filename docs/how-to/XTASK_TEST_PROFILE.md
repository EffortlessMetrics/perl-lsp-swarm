# xtask test-profile build cost

The selected `profile.test.package.xtask.opt-level = "z"` override reduces
active xtask artifact bytes by **125,144,386 (11.35%)** versus inherited opt1,
while preserving line tables, assertions, overflow checks, dependencies and the
same test selection. This is a bounded disk-first repair for explicit test-profile
builds under [#3230](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3230).
Canonical agent-profile calls retain their existing settings; this does not
reduce dev/agent bootstrap cost or establish a universal storage allowance.

The earlier opt0 candidate reduced compiler CPU more, but increased active bytes
21.6%. It remains measured below as a CPU-first tradeoff, not the selected default.
Opt-z is slower than opt0 to compile and its integration runtime observations
were slower than either earlier observation. **Warm runtime parity is not proven.**
Disk-first selection accepts this observed cost; it is not an overall test-loop
speedup claim. No test or helper binary was disabled, and no debug data was stripped.

## Selected size trial and tradeoffs

Same Windows host, installed toolchain, two Cargo jobs, offline execution and
private worktree resources as the earlier trial below. All **35 xtask units**
rebuilt at opt-z; all **334 dependency artifact records** retained identical
profiles/filenames/features and were fresh. Profiles differed from opt1 only in
optimization level. The same **70 unique active artifact files** were measured.
[Cargo documents size optimization as an experimental choice, not a size guarantee](https://doc.rust-lang.org/cargo/reference/profiles.html#opt-level).

| Active artifact bytes | Original opt1 | Earlier opt0 | Selected opt-z |
| --- | ---: | ---: | ---: |
| rlib | 136,622,196 | 175,944,060 | 106,394,930 |
| rmeta | 36,711,338 | 36,622,771 | 36,711,338 |
| Executables | 220,849,664 | 271,529,984 | 181,015,552 |
| PDBs | 708,829,184 | 856,965,120 | 653,746,176 |
| **Total** | **1,103,012,382** | **1,341,061,935** | **977,867,996** |

Opt-z is 27.08% smaller than opt0. PDBs remain the largest category; removing
them or changing the line-table contract was not part of the experiment.

| Rustc-process CPU / elapsed | Original opt1 | Earlier opt0 | Selected opt-z |
| --- | ---: | ---: | ---: |
| Normal CLI | 681.48 / 709.14s | 127.72 / 134.26s | 506.61 / 535.37s |
| Owning test harness | 911.11 / 821.21s | 183.92 / 190.20s | 755.23 / 766.71s |

Compared with opt1, these two opt-z compilers used 25.7% and 17.1% less CPU.
This excludes external linker CPU. Opt-z library CPU/elapsed was 230.66/140.37s;
the original library counter is unavailable. Opt-z peak working sets were
2,192,015,360 bytes for the library, 3,649,241,088 for the CLI and 4,906,590,208
for the harness. Other Cargo workloads were present on this shared host, so
these single compile observations are not portable performance guarantees.

| Selected opt-z invocation | Wrapper elapsed | Retained file bytes at end | Sampled peak file bytes |
| --- | ---: | ---: | ---: |
| All 35 units rebuilt; dependencies retained | 909.89s | 3,099,277,800 | 3,347,103,323 |
| No-op repeat; all 369 records fresh | 2.03s | 3,099,277,800 | 3,099,277,800 |
| One comment added to ci_subject; only that unit rebuilt | 4.11s | 3,099,376,104 | 3,099,638,792 |
| Final comment-only manifest clarification; all 369 records fresh | 2.17s | 3,099,376,104 | 3,099,376,104 |

The opt-z rebuild began at 2,873,969,493 retained file bytes: end growth was
225,308,307 bytes and sampled additional peak was 473,133,830 bytes. These are
three-profile co-residence observations, not opt-z-alone footprint or reclaimed
space. Sampled process-tree RSS peaked at 8,638,816,256 bytes; shared pages may
be counted repeatedly and sampling can miss transients. The declared allowance
was 24 GiB growth plus 40 GiB reserve, and estimated RAM allowance 24 GiB.
Those conservative declarations remain unverified bounds. H: had about 2,275 GiB
free and the host about 134 GiB available RAM at the start.

All **62 selected tests passed**: 48 unit, 9 ci_scope_tests, 5 ci_subject.
The first opt-z integration wrapper took **47.34s**, including a 3.39s compile
after source restoration; suite runtimes were **13.63/29.36s**. A no-build
repeat took **41.30s**, with suite runtimes **13.45/25.45s**. Earlier opt1
was 23.11s (8.61/13.11s suites), and opt0 19.85s (5.56/12.67s suites).
The slower runtime persisted in the repeat. Shared-host Git/metadata/I/O
contention is plausible but unproven; no cause or runtime improvement is claimed.

The 4.11s edit control proves narrow reuse of this real xtask graph, not edit
performance parity: no matching opt1 edit run was taken. Only ci_subject was
rebuilt; the other 368 records stayed fresh. The appended comment was restored
byte-for-byte before the 62-test validation, with matching source SHA-256
`a05a1876f56a13b59d8274b115c5ab2cf2393d5090ccfd455e782c80db716fad`.
All 4,742 owning-harness tests compiled, while 48 were executed. No full workspace,
full owning-harness runtime, other features, or other platforms were added.

Measured opt-z manifest blob: `5e9ab41e7202a8d9a22400c2a9d2fa1e8700ca75`;
working-file SHA-256: `14c14b09fb870a9168a6bc9e3031d39779d88e398ca28ae5f7e397dfd1a583bc`.
The final manifest changes only its explanatory comment: blob
`402f8d5c329b9ad88acafda5c95b8ad792b3ac1b`, working-file SHA-256
`83a06d8abfc1e711b53a206d4a8455ee344a659d688d63bcded5399415d83ba4`.
The final all-fresh repeat verifies reuse under that exact file. Parsed TOML,
with this one package override removed, equals the original baseline.

Evidence: `H:\Temp\task6-build-footprint\size-z-summary.json`, all
`xtask-size-z-*.json`/Cargo logs, `z-xtask-*.compiler.json`, the edit-source
receipt and artifact snapshots. Measurements use the same file-byte and sampling
limits below; no exact allocated peak, quota, or Windows settlement proof is inferred.

## Earlier opt0 CPU-first trial (historical observations)

The following before/after tables describe opt1 versus opt0, not the final opt-z
selection. Their distinct source identity and dependency-cache conditions remain
preserved for comparison.

Measurements were taken on 2026-10-15 UTC, against base
`4cc7960e61b685ce3b7e4cb9fe43b929bf900972`. The measured candidate `Cargo.toml`
Git blob is `f4af07ee507651088c828f22d1d5b359261f073d`; its working-file SHA-256
is `021a21350d0728db32173ca72d74fab5fec75444cb07e460a8cd66c310f40cb5`.
Host: native Windows `SZ-Corsair-RS`, Ryzen 9 9950X3D (16 cores/32 threads),
253.6 GiB usable RAM, approximately 130 GiB initially available. Compiler:
installed `rustc 1.95.0 (59807616e 2026-04-14)`, LLVM 22.1.2,
`x86_64-pc-windows-msvc`; repository toolchain pin unchanged. Both versions used
two Cargo jobs, disabled incremental compilation, default features, the same
lockfile and cached Cargo home, and offline execution. Other bounded desktop
workloads could run concurrently; these are single observations, not a portable
performance guarantee.

The canonical Python entrypoint ran this selection with an exact matching
scope-bound budget file:

```powershell
python scripts/cargo_admitted.py --budget-file <matching-scope.json> test -p xtask --bin xtask --test ci_subject --test ci_scope_tests --no-run --profile test --locked --offline --message-format=json-render-diagnostics -vv
```

The initial dependency-cold baseline omitted only the diagnostic JSON/verbosity
options. Its subsequent warm run with those options rebuilt no units.
[Cargo automatically builds binaries when selecting integration tests](https://doc.rust-lang.org/cargo/commands/cargo-test.html#target-selection).
The actual closure was **35 xtask units**: its normal library, 31 normal binaries
(the CLI and 30 helpers), the owning CLI test harness, and two integration
harnesses. Target count alone is therefore an inadequate sizing input. Cargo
JSON verified that all 35 candidate units changed only optimization level:
line-tables-only, debug assertions and overflow checks remained enabled.
All 334 non-xtask Cargo artifact records retained the same profile and filenames
and were fresh in the candidate rebuild. Counts distinguish package, target,
features, full compiler profile and artifact filenames, preserving host/build
variants. No target was disabled to obtain these results.

Read-only handles held before compiler exit supplied Windows process CPU and
peak-working-set counters:

| Compiler unit | Before rustc CPU | After rustc CPU | Before elapsed | After elapsed | Before peak WS | After peak WS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Normal xtask CLI | 681.48s | 127.72s | 709.14s | 134.26s | 3.44 GiB | 3.24 GiB |
| Owning test harness | 911.11s | 183.92s | 821.21s | 190.20s | 4.64 GiB | 4.48 GiB |

These are rustc-process CPU values, excluding external linker CPU, not total
process-tree CPU. The observed reductions were 81.3% and 79.8%. The candidate
library took 44.86s elapsed/58.75s rustc CPU; its predecessor's final CPU counter
was missed and is unavailable, rather than reconstructed from later samples.

Cargo-reported active xtask artifacts contained 70 unique files in both cases:

| Artifact class | Before bytes | After bytes |
| --- | ---: | ---: |
| rlib | 136,622,196 | 175,944,060 |
| rmeta | 36,711,338 | 36,622,771 |
| Executables | 220,849,664 | 271,529,984 |
| PDBs | 708,829,184 | 856,965,120 |
| **Total** | **1,103,012,382** | **1,341,061,935** |

The extra 238,049,553 bytes are an explicit cost of the faster test build.
PDBs account for most active bytes even with line tables only. Stripping them
would change the accepted backtrace/debug contract and was not used. No
speculative codegen-unit or dependency changes were added.

Whole target/build/temp observations, with hardlinks deduplicated:

| Run and cache condition | Wrapper elapsed | Retained file bytes at end | Sampled peak file bytes |
| --- | ---: | ---: | ---: |
| Before: empty test-profile output, cached Cargo home | 1,740.40s | 2,172,661,370 | 2,172,661,370 |
| Before: warm repeat | 1.58s | 2,172,661,370 | 2,172,661,370 |
| After: xtask units rebuilt, dependencies retained | 241.69s | 2,873,969,493 | 2,980,235,283 |
| After: warm repeat | 1.16s | 2,873,969,493 | 2,873,969,493 |

The cold rows have different dependency-cache conditions and are **not** a
paired full-cold speedup. A full dependency-cold candidate run was not repeated:
unchanged dependency compilation would not answer the selected package question.
The candidate cache retains predecessor identities where Cargo leaves them;
aggregate retained/peak bytes describe that co-residence, not candidate-alone
storage. No cache cleanup or reclaimed capacity is claimed.

File bytes use `GetCompressedFileSizeW` and are not exact filesystem allocation
(cluster rounding and metadata are excluded). Disk samples requested one-second
intervals; process-tree RSS samples requested 200ms intervals, with scan overhead.
Transient peaks can be missed. Sampled aggregate tree RSS was 8,594,714,624 bytes
before and 8,218,836,992 after; summed RSS can count shared pages more than once.
Cargo-home transient growth, exact whole-host peaks, and a quota were not proven.

Both versions passed the same existing 48 `tasks::ci_scope::tests::` cases,
9 `ci_scope_tests` cases and 5 `ci_subject` cases. All 4,742 owning-harness cases
compiled; only the selected 48 were executed. The integration wrapper took
23.11s before and 19.85s after (individual suite times 8.61/13.11s and
5.56/12.67s). These Git/metadata-heavy single observations establish successful
behavior, not a general runtime speedup. Runtime inherited
`CARGO_NET_OFFLINE=true` and used `--test-threads=1`. Full owning-harness runtime,
workspace tests, other features and other platforms were not run.

Resource planning remained the existing authority. Qualification declared a
40 GiB reserve plus 24 GiB additional growth for this xtask request, and 1 GiB
growth for a dependency-free `perl-test-must` probe; `basis_verified:false` stayed
honest. No universal requirement is inferred from those estimates. The tiny probe
passed all 12 cases, cold/warm in 5.56/2.95s, with sampled peak file bytes
3,667,243/3,629,474. Default percentage admission refused the C: Cargo home at
96.68% used despite more than 123 GiB free; exact scoped budget-file support was
used without changing policy.

Read-only inventory found 29 installed toolchain directories. Selected logical
sizes were Cargo home 2.22 GiB, pinned toolchain 1.50 GiB, a legacy F: agent
target 3.85 GiB, and an existing H: admission slot 7.34 GiB. The latter two held
0.89/1.35 GiB of PDBs and 0.96/2.74 GiB of rlibs. These path-counted figures can
count hardlinks repeatedly; they are not deletion proposals. The selected
dependency graph also contains syn 2/3, rand 0.9/0.10, rand_core 0.9/0.10 and
windows-sys 0.60/0.61. Dependency consolidation would cross other contracts and
was left untouched. Distinct profile/build-unit identities, the separate agent
namespace, and retained worktrees remain additional amplification sources.

Refreshed current-main investigation also found the proposed missing-Git-metadata
warm-build repair already landed as
[PR #17609](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17609), commit
`ee028f61a1ab989eeb73f80dcac112cd753079fc`, closing #17605. Production
`crates/perl-lsp-rs/build.rs` and its original regression suite are unchanged
between that commit and refreshed main. Production SHA-256 is
`1cb79ea1c5e2927a6a8fbb1a36f4d8e3c3307614e3b35217a1aab779502f2bb9`.
No duplicate production repair was made. The separately supplied independent
fixture commit `019d6498ba9d1a93caed3a3703fdc962dd7ad2db` was inspected in a
separate retained task checkout; no fixture Cargo run was added after the request
to prioritize the real warmed graph.

Saved dependency records also show feature/profile variants: proc-macro2 with
and without span-locations, quote with and without default, serde with and
without alloc, and serde_json raw_value/float_roundtrip versus preserve_order.
These are observations in `dependency-variants.json`, not removable-duplication
claims: host/build and target roles plus feature semantics require separate review.

## Adopt or roll back the selected opt-z override

1. Integrate the small manifest override through normal review and protected
   integration. Default test-profile callers receive it automatically. Explicit
   agent/dev/release callers retain their settings; no caller or front door was
   migrated here. Select the smaller opt-z artifacts with the observed runtime
   cost below; the historical opt0 result is a separate CPU-first option.
2. Keep using `cargo-admitted` and its private worktree target/build resources.
   For qualification, use the exact command and a matching scope-bound budget as
   described in [constrained Cargo builds](CONSTRAINED_CARGO_BUILDS.md). Local
   measurement code generated those qualification files; this patch adds no
   automatic ordinary-caller sizing and does not require a new contributor API.
3. Requalify changes to host/toolchain/features/jobs/target closure. Native
   Windows descendant settlement remains [#17482](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17482):
   successful legacy-wrapper exit and lease absence are observations, not a
   qualified process-tree completion guarantee. No ordinary-loop activation or
   blanket-guard removal is established by this source repair.
4. Preserve the separate bootstrap and caller owners #17484/#17485 and #9567.
   They remain the route for reducing bootstrap/selection amplification. Hooks,
   storage admission, global configuration and protection were not edited.
5. Roll back by removing this one package override (or reverting its scoped
   commit). Preserve generated outputs for owner verification; do not clear
   other worktrees, caches, leases or research to adopt or revert it.

Local evidence is retained at `H:\Temp\task6-build-footprint`: run/budget JSON,
Cargo logs and artifact hashes, process counters, exact compiler versions,
`cache-inventory.json` and checked `summary.json`. Its source worktree is
`F:\Temp\task6-build-footprint`; outputs are the worktree-private target/build
paths in the emitted descriptors under
`H:\cargo-devplane\SZ-Corsair-RS-win32-9fbfb9e6d90e0995\worktrees\84413c1a355e99f5556b12cdcd74333ad3670a6c4189b80b9b31c1deb249abe3`.
Every successful measured invocation observed its lease absent afterward.
Outputs/evidence remain retained with reason; cleanup and native ownership
qualification remain separate decisions. No Bun/security research was touched.
