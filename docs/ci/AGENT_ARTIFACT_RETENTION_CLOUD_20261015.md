# Cloud agent artifact retention: bounded leaf sample

Observed 2026-10-15 on Linux x86_64 host `1759bc6c25ae`, Debian 13,
16 GiB cgroup memory limit and four CPU quota equivalents. Source is current
main `4cc7960e61b685ce3b7e4cb9fe43b929bf900972`; isolated branch
`codex/cloud-artifact-retention-20261015`. No production change is proposed.

**Result:** this small repository crate replaces its edited artifacts in place.
There is no retained target/build growth after the cold run across three real
edit/revert cycles. Growth is not inherently cumulative under the canonical agent
profile. This sample does not establish the cause of larger worktrees' retained
bytes, or qualify parser/LSP graphs, multiple harnesses, feature/profile variants,
the six-package campaign, or a universal storage allowance.

## Reproduction and admission

Exact executed command, from the canonical repository root:

```bash
scripts/cargo-admitted --budget-file /workspace/scratch/artifact-retention/budget.json \
  test -p perl-test-must --lib --profile agent --locked --offline --message-format=json
```

The [instrument](evidence/agent-retention-20261015/measure.py) captures environment,
preflight and all nine runs. [Machine evidence](evidence/agent-retention-20261015/results.json)
contains every stdout/stderr, scope/budget, per-root endpoint sizes, sampled timeline,
file size/inode/hash manifests, compiler messages, source/config hashes and settlement.
This is an experiment instrument, not a newly supported build interface.

Installed rustc 1.95.0 (`59807616e`), Cargo 1.95.0 (`f2d3ce0bd`), native
`x86_64-unknown-linux-gnu`, Python 3.12.14. No crate features or dependencies;
`--lib` selects one test harness with 12 tests and excludes integration/doctests.
Agent inherits dev opt-level 1 and line tables; incremental=false; build override
has debug=false and incremental=false. Two jobs, no compiler cache wrapper.
No feature/profile/toolchain changes between cells. Cargo messages confirm one
compiler artifact per cell, empty features and debuginfo="line-tables-only". There are no build
scripts, proc macros or nested Cargo builds in this selected graph.

Private target/build initially absent, one unchanged pair throughout; no clean,
new target per edit, backdated restoration, or shared sibling artifact reuse.
Cargo home is preseeded; this is **cold compiled outputs, not cold downloads**.
Temporary storage is under `/workspace/scratch/artifact-retention/temp`, on the
same workspace filesystem. No concurrent cargo/rustc/sccache process was found
before execution. Supported `proc-parent-pidfd` discovery was explicitly selected.

Default preflight returned 75 before allocation: free 32,006,373,376 B versus
42,949,672,960 B required, shortfall 10,943,299,584 B. Then the supported exact-scope
budget route admitted the request with 26 GiB reserve plus 1 GiB total additional
growth. This is a conservative operator declaration independently reviewed against
the dependency-free offline request and prior tiny-crate retained evidence
[#3230](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3230#issuecomment-6104276397).
It is `basis_verified=false`, not a measured peak bound or default policy change.
No guard was removed and no provisioning/download was required.

## Endpoint results

All rows exited 0, ran 12 passing tests with zero ignored, and reported original-owner
kernel ECHILD (`__WALL`), tree_settled=true, no errors and matching lease release.
The immediate next invocation was admitted. Final lease absence was checked separately.
Sizes below include directories, deduplicated by `(st_dev, st_ino)` across roots.

| Cell | Seconds | Compiler unit | Target logical / allocated B | Build logical / allocated B |
| --- | ---: | --- | ---: | ---: |
| Cold | 0.953 | rebuilt | 12,288 / 12,288 | 2,831,605 / 2,854,912 |
| Warm | 0.250 | fresh | 12,288 / 12,288 | 2,831,605 / 2,854,912 |
| Edit 1 | 0.410 | rebuilt | 12,288 / 12,288 | 2,831,557 / 2,854,912 |
| Revert 1 | 0.410 | rebuilt | 12,288 / 12,288 | 2,831,605 / 2,854,912 |
| Edit 2 | 0.403 | rebuilt | 12,288 / 12,288 | 2,831,557 / 2,854,912 |
| Revert 2 | 0.430 | rebuilt | 12,288 / 12,288 | 2,831,605 / 2,854,912 |
| Edit 3 | 0.402 | rebuilt | 12,288 / 12,288 | 2,831,557 / 2,854,912 |
| Revert 3 | 0.417 | rebuilt | 12,288 / 12,288 | 2,831,605 / 2,854,912 |
| Final warm | 0.239 | fresh | 12,288 / 12,288 | 2,831,605 / 2,854,912 |

Cargo home starts at 249,562,192 logical / 301,121,536 allocated B; cold adds
3,886 logical / 4,096 allocated B. All later endpoints remain 249,566,078 /
301,125,632 B. Temp remains 4,096 / 4,096 B. Excluding preexisting cache/temp and including that initial cache delta, total
additional retained allocation is 2,871,296 B.

Each edit appends a distinct `[retention-N]` suffix to both panic diagnostic format
strings. This changes executable behavior and bytes while retaining existing
substring assertions. Every revert restores exact original source bytes using a
current write time. No source edit remains. The executable path remains
`build/agent/deps/perl_test_must-f8695df40ad9fbe3`; original SHA-256
`5357062cee3929107a7a6c7551e88a1ed2c53c13a37729eacae8ddf621584fbc` returns after
every revert. Each edit has a different digest and replacement inode. Endpoint
manifests show one current executable, no old executable hashes; target contains
only the profile lock file and directories. Empty locks are zero allocated bytes.
No nonempty duplicate content remains across target/build. The retained executable
and fingerprints support warm reuse; their presence is expected cache retention,
not demonstrated stale duplication. No cleanup repair is justified by this sample.

## Sampling and accounting limits

Logical bytes are `st_size`; allocated bytes are `st_blocks * 512`. Directory
metadata is included, symlinks are not followed, hardlinks are counted once.
The periodic sampler covers target/build/temp only, requested wait 20 ms **after**
each scan. It recorded 193 samples; largest start-to-start gap 47.911 ms, longest
scan 0.864 ms. No scan races/errors or observer failure were recorded. Cache is
endpoint-only. Maximum sampled target/build/temp allocation was 2,920,448 B
(transient build output), versus 2,871,296 B after settlement including temp.
Minimum sampled workspace available bytes was 32,002,998,272 B, above the declared
reserve. These are sampled lower bounds on transient peaks, not quotas or proofs
of continuous reserve. No peak RSS claim or speedup qualification is made; n=1
per cell, observer overhead and kernel cache effects remain.

The experiment journal is outside measured roots. At measurement completion its
regular files, including raw logs/results, instrument, declaration and preserved
original source, occupied 306,707 logical / 372,736 allocated B. This is explicitly
additional evidence overhead, excluded from route sizes. Later tracked evidence/report
copies and Git publication overhead are additional storage; no whole-experiment
peak is claimed. Filesystem free movement can include unrelated writers; overlay
physical backend usage, open-but-unlinked files and unobserved transient/cache
peaks are not proven by recursive stat accounting.

## Ownership, review and cleanup dependencies

Refreshed AGENTS.md, authority registry, current Cargo storage/admission source and
[#17673](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17673),
[#17655](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17655),
[#17658](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17658) and
[#3230](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3230) owner evidence.
The local profile worker retains production ownership. Default guard retirement
remains held pending measured caller migration; its owner branch
`task/retire-blanket-disk-guard-20261015` is unchanged. Existing campaign, hook,
missing-child-table publication holds and owner branches are untouched.

Independent review checked the sizing declaration, real 12-test denominator,
profile/feature/toolchain identities, endpoint manifests, executable replacements,
scan intervals and original-owner settlement. It corrected the initial cache-delta
wording and nominal sample-interval wording before publication. This is bounded
measurement review, not production/campaign approval.

Private target/build and temp: retained for reproducibility; lease: released.
Shared Cargo home and evidence: retained. No old worktree/cache/DEVPLANE cleanup.
Any future reclamation depends on #11606 positive ownership/consumer checks and
#17623 lifecycle policy, exclusive ownership of both roots and every consumer,
and explicit authorization. Absence of a lease alone is insufficient. Global
Cargo cache GC does not reclaim target/build outputs. Repeated profile/features,
separate worktrees and larger graphs need their own evidence before calling their
retention unavoidable or stale. Next useful owner action is to apply this same
endpoint/hash attribution to the local worker's measured growing graph.

Publication is documentation/evidence only. `just agent-preflight` storage layout,
Python instrument syntax and diff whitespace checks pass; these do not execute a
publication gate. This saved checkout has only sample Git hooks and no configured
core.hooksPath, so installed hook qualification is missing. Normal commit/push
use no bypass flags. All 39 push-trigger workflows were parsed: each is limited
to main/master; this isolated task branch selects none. No PR was opened because
that would select broader CI. Production integration and hook provisioning remain
with their owners; this report does not claim required protected checks passed.
