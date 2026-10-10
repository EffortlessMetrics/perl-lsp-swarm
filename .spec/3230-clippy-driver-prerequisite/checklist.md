# Evidence checklist

- [x] Read AGENTS, relevant provider skills, storage contract and actual linked issues.
- [x] Inspect current executor owner/export surface and unchanged production refusal.
- [x] Check exact storage/consumer remote heads and allocate separate test worktree.
- [x] Audit all workflow push triggers: existing pushes restricted to main/master;
  `recovery/clippy-driver-contract-3230` is quiet. No PR/event/CI label yet.
- [x] Execute pinned installed 1.95 upstream driver against a recording fixture.
- [x] Preserve positive exact dispatch and negative wrong-shape/no-work controls.
- [x] Run default suite separately: 2 pass, 8 explicit installed-driver skips.
- [x] Run opt-in suite: 10 pass; 8 real dispatch tests, 2 production refusal tests.
- [x] Run storage prerequisite suites: 54 tests, 53 pass, 1 opt-in build skipped.
- [x] Execute temporary wrong-route controls: generic Clippy allowlist rejected by
  pre-resource refusal assertions; dropped subcommand rejected by exact dispatch.
  Both fail as assertions, with no instrument errors; no mutation retained.
- [x] Fresh differentiated proof/authority reviews independently reran both test
  modes and checked upstream/live issue authorities. One documentation finding
  (table versus implemented renderer) repaired and re-reviewed; no open findings
  for the bounded preparation claim. No production or merge judgment.
- [x] Initial prerequisite preserved at 75d64037 and documented in all three existing issues.
- [x] Staged admission preserved on quiet branch at 6b4024cbe6057dc2d495a9ebb800fae1c157c4fd.
- [x] Updated existing #3230/#11660/#11663 and consumer #17484 with revised scope and exact downstream failure.
- [x] Finite existing-owner Linux admission; real fail/clean fixture qualification.
- [ ] Canonical typed executor/parity, native Windows Clippy, exact hook/resolver qualification.

Initial prerequisite commands (no product build at that stage):

```text
python3 scripts/tests/test_clippy_driver_contract.py -v
CLIPPY_CONTRACT_TOOLCHAIN_ROOT=/workspace/.cloud-tools/rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu python3 scripts/tests/test_clippy_driver_contract.py -v
python3 -m unittest discover -s scripts/tests -p 'test_cargo_admitted*.py' -q
git diff --check
```

Installed POSIX observation: rustc 1.95.0, commit
`59807616e1fa2540724bfbac14d7976d7e4a3860`, host
`x86_64-unknown-linux-gnu`; cargo-clippy/clippy-driver both
`clippy 0.1.95 (59807616e1 2026-04-14)`. Bounded version probes did not provision tools.

SHA256 observation (these are evidence, not a portable trust allowlist):

| Executable | SHA256 |
| --- | --- |
| cargo | `841072d1d92f9e841d9ba5b0814182a0adf064acf4527cd120967b7bc49dcb66` |
| cargo-clippy | `b71b1263063707608daa1e9b231ad9d87507c11d6aa178ede2b01d8372d5effb` |
| clippy-driver | `81bd985ee50d78e9d8c5810f6ca836614d1bf95e0e8142f1ec5f42eb250ed729` |
| rustc | `bff349e72704ff70bc08a234a3847338e797065bbedde5e556808bc87b7bf7c6` |

Staged follow-up checks:

- Combined admitted tests: 72 total, 71 pass, 1 separate opt-in build skip.
- New finite route suite:18 tests pass, including configuration/identity,
  contention, all-exit retention and early/late cancellation.
- Both fresh final reviews found the bounded candidate ready. Startup
  cancellation/post-probe configuration and loader/include/target/SYSROOT findings
  were repaired and re-reviewed. One final wording precision (renderer source
  binding versus tool/config revalidation) was corrected.
- Real native fail/clean pair passed; latest reproducible proof directory:
  `/workspace/clippy-qualification/clippy-native-lnlsrkm_`.
- Retained `*-stdout.txt`, `*-stderr.txt`, `*-budget.json`, `*-receipt.json`
  contain product101/0, Clippy-specific diagnostic, clean package artifacts,
  source/owner hashes, exact native group absence, captured lease identity and
  verified owner release postcondition. Fixture retained, not deleted.
- Fixture-specific reserve 4 GiB/growth 2 GiB covers every checked actual destination;
  default 40 GiB remains unchanged. No default build or expensive CI admitted.

```text
CARGO_HOME=/workspace/.cloud-tools/cargo RUSTUP_HOME=/workspace/.cloud-tools/rustup RUSTUP_AUTO_INSTALL=0 python3 scripts/tests/qualify_admitted_clippy.py --proof-root /workspace/clippy-qualification
```

Native Windows Clippy, general automatic tree settlement, canonical typed executor
completion/parity and exact hook/resolver lint qualification remain NOT_PROVEN.
Source worktree, installed shared toolchain/cache and owned fixture artifacts are
retained. Both fixture leases released only after independent matching-owner
native verification; no outstanding fixture consumer was observed.

## Exact hook/resolver consumer attempt

After route correctness/fresh review, checked out detached exact
`b5046c390bd6ea141b530bc896448c11c57f6cf2`, tree
`9f6958b3a3936a04fa5c35ec1597d052b20d925b`, at
`/workspace/clippy-hook-consumer`. Used immutable production route 6b4024cbe and
`clippy -p perl-ci-hygiene --all-targets --profile agent --locked --offline -- -D warnings`.
Default 40 GiB preflight still denied. Separately frozen budget used reserve 8 GiB /
aggregate estimated peak growth 16 GiB (not a measured forecast): prior cold 70-package
trace and mixed profiles/tests output<1GiB from issue #17484 comment 6094107977;
large estimate conservatively covers native-target Clippy/all-target dev units,
no downloads/fullworkspace/product execution. All destination roots were on the
actual workspace volume with sufficient 24 GiB admission headroom; jobs 2.

Actual product exit 101: strict workspace Clippy `print_stderr` at
`crates/perl-ci-hygiene/src/change_set.rs:236`, and `print_stdout` at 411, 422.
No source fixes were made to this qualification consumer. 96 dependency artifact
events / 76 package IDs were observed; sole repo artifact package was
`perl-test-must` (an explicit dev-dependency). No completed hygiene artifact or
package pass is claimed. Distinct Clippy workload is not the prior 70-package
normal/build product-free baseline; no performance/no-less-work parity claim.

Evidence: `/workspace/clippy-consumer-proof/{preflight.stderr,budget.json,
clippy.stdout,clippy.stderr,ownership.json}`. Exact launched group is absent,
but this dependency-bearing workload needs independent all-consumer verification;
lease remains retained at `/workspace/clippy-consumer-proof/devplane/
5b4b67c9a262-linux-b65dcb543d0ae605/cargo-active`. Captured identity/UUID marker
are in ownership.json; no cleanup or generalized fixture release applied.
Worktree/source/build/cache/trace artifacts are retained with reason: reviewable
qualification failure. Parent owns consumer fixes and final native verification.

Existing-issue updates:

- [#3230 implementation/evidence](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3230#issuecomment-6094379204)
- [#11660 canonical boundary](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11660#issuecomment-6094383511)
- [#11663 remaining parity](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11663#issuecomment-6094384275)
- [#17484 exact consumer failure](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17484#issuecomment-6094384950)

All 39 push workflows are main/master restricted. Production commit 6b4024cbe
workflow-runs query returned no runs. No PR/merge/CI label or expensive hosted work.
Exact storage writer remote verified unchanged fd8aa27ce2ebf2a1e95b2eae4109be394252b729.

## Independent finished-attempt settlement

Follow-up root-owned verification recovered executor connectivity on the same
native host `5b4b67c9a262`; a transient transport disconnect was never used as
owner-death evidence. Exact consumer group 3801 and fixture groups 2727/2759/3085/
3117 were absent. Two native roster scans in the same visible init PID namespace
found only known pre-existing provider processes and verifier ancestors; no active
compiler, Cargo, Clippy, build script, detached helper or product consumer.
Same-UID executable/cwd/descriptor/mapping/environment associations were checked;
the two pre-existing UID0 provider daemons were explicitly accounted as runtime
infrastructure with no workload workers (their descriptors are unavailable to the
agent UID). No OS security or process ownership settings were changed.

Consumer inode (device 27, inode 1080777) and its original empty unique marker
`owner-faf2a535561b4eb9b0a433949498a757` still matched the active-owner receipt.
The existing release helper removed only that marker and `cargo-active`; native
postcondition confirms lease absent. Both older and reproducible fixture pairs'
leases also remain absent. 749 artifact files /84,032,428 logical bytes across nine
recorded target/build/temp roots retain their original identities/content hashes.
Consumer source remains clean at exact b5046c390. All worktrees, outputs and shared
Cargo/rustup resources are retained; no artifact/cache cleanup was attempted.

Full receipt: `/workspace/clippy-settlement/settlement-final.json`, SHA256
`61532fdb1d6a994e41a588645026d20ec4f66a23aec0cb1a12506bfee2fc1b54`.
The committed `settlement-receipt.json` preserves the native process roster,
original inode/marker, release postcondition, per-root inventory identities/counts
and manifest hashes; full file-level inventories remain local. These establish
this bounded manual owner settlement, not generic automatic tree proof.

To make future retained-lease recovery practical, Clippy now emits original
`lease_identity` and `lease_marker` in the resource JSON before setup probes;
positive and retained setup-failure tests check them. The existing matching-owner
release helper and builtin storage paths are unchanged. No storage PR candidate
(including parent-reported 3ff48c85 after integrated 4e57090b) is modified here.
The supported manual procedure and refusal cases are documented in CARGO_STORAGE.

Smallest next parity qualification: after the original owner fixes the three
consumer lints, use one exact Linux package/lock/source/profile/native-target/
environment/config/toolchain subject, separate admitted private output and
scope-bound budget, then record cold/warm compilation-artifact identities and
freshness with matching selected units. Do not compare this 76-package Clippy/dev
workload against the earlier 70-package normal/build baseline as a speed gain.
Canonical selected-work/result parity still needs accepted executor contracts.
The next native cell is Windows/MSVC Clippy driver/compiler identity and forced
child-environment projection, with native process-tree/cancellation/release via
active #17482 launcher ownership. Do not compete for laptop builds or implement a
parallel Windows supervisor; Linux manually verified settlement cannot fill it.
