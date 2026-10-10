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
- [ ] Preserve exact commit on quiet branch and document decisions in existing issues.
- [ ] Production Clippy admission/qualification: BLOCKED_BY_PREREQUISITE.

Commands actually run (no raw Rust build):

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

No immutable product binary exists because no product build was performed. Driver
dispatch is prerequisite evidence, never a lint/product receipt. Native Windows,
real lint fail/clean, capacity/process-tree behavior and hook/resolver qualification
remain NOT_PROVEN. Resources: source worktree retained for review; no production
target/build/cache/lease allocated; test-owned recording fixtures removed normally.
