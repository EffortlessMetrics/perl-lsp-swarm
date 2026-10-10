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
- [ ] Preserve staged admission commit and update existing issues with revised scope.
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
- Fresh authority review confirmed startup cancellation and post-probe config
  repairs. Fresh prerequisite review found loader/include/target and SYSROOT
  seams; all repaired, final re-review recorded with preserved candidate.
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
