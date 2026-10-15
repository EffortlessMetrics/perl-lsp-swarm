# Independent two-crate Git metadata regression

`scripts/tests/test_git_metadata_two_crate.py` independently exercises the exact
production `crates/perl-lsp-rs/build.rs` in a tiny library, then consumes its
compile-time revision from a second crate. It supplements the original
`test_lsp_revision_build.py`; neither production code nor that suite is changed.

The original defect is already resolved by
[#17609](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17609), merged as
[`ee028f61a1`](https://github.com/EffortlessMetrics/perl-lsp-swarm/commit/ee028f61a1ab989eeb73f80dcac112cd753079fc).
[#17605](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17605) is closed.
This fixture does not take over #3230's footprint implementation or owner branches.

## Run only the focused fixture

Select an installed Cargo/Rust toolchain and host-supported admitted child-discovery
profile first. Use a new absolute proof directory; existing evidence is never
overwritten or cleaned. The suite calls the repository's production Python
`cargo_admitted.py` entrypoint for each offline Cargo invocation.

```bash
GIT_METADATA_REAL_BUILD=1 \
GIT_METADATA_PROOF_ROOT=/absolute/new/proof-directory \
  python3 scripts/tests/test_git_metadata_two_crate.py -v
```

Without the opt-in, all three tests skip and no build proof is established.
`GIT_METADATA_BUILD_SCRIPT=/absolute/build.rs` selects a historical or intentionally
incorrect script without editing production source. The test copies it byte-for-byte.

The fixture declares an operator sizing allowance of 1 GiB additional peak storage
and an 8 GiB reserve for two tiny dependency-free crates and one build script,
using an installed toolchain with no downloads. These are fixture-specific declared
values, not measured peak bounds or repository workload defaults. Admission still
checks actual destinations and available capacity; lifecycle/lease failures are
failures, never retried as raw Cargo. The caller owns the sequential lane and all
later consumers. All source, private target/build paths and logs are retained.

## Observed evidence, 2026-10-15

Source basis: `4cc7960e61b685ce3b7e4cb9fe43b929bf900972`.
Production build-script SHA256:
`1cb79ea1c5e2927a6a8fbb1a36f4d8e3c3307614e3b35217a1aab779502f2bb9`.
Fixture SHA256:
`5a44c669d080ab5eb64626ca55fb3f2048870ae2802f1dcd8f45863a22eba119`.

Linux host `eb397e26097a`, installed Cargo `1.95.0 (f2d3ce0bd 2026-03-21)`
and rustc `1.95.0 (59807616e 2026-04-14)`, two jobs,
`CARGO_ADMITTED_CHILD_DISCOVERY=proc-parent-pidfd`, automatic installation disabled.
Toolchain binaries came from
`/workspace/shared/rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin`;
Cargo home was `/workspace/shared/cargo`. No repository Rust package was built.

Command above with proof root `/workspace/git-metadata-proof-final`:
**3 tests passed in 12.001s, 27 Cargo observations.** The instrument checks Cargo's
JSON `compiler-artifact.fresh` separately for library and consumer, and checks the
consumer's actual printed compile-time identity against Git commands after each
transition. It does not infer freshness from elapsed time or unchanged output.

| Scenario | Observed discrimination |
| --- | --- |
| Missing optional packed refs | Cold build; unchanged warm invocation reuses both crates. |
| Commit, then repeat | Updated consumed revision; repeat reuses both crates. |
| Consumer-only source edit | Library stays fresh; consumer recompiles; repeat reuses both. |
| Linked worktree | `.git` is a file and `.git/HEAD` is absent; warm reuse works. |
| Linked vs primary commits | Linked commit refreshes output; unrelated primary commit leaves both crates fresh. |
| Common tag creation, packing, packed-tag deletion | Correct tag/commit labels after changes; unchanged repeats reuse both crates. |
| Packed branch becomes loose | New linked commit refreshes output; repeat reuses both crates. |
| Detached HEAD, detached commit | Correct consumed revision after each transition; unchanged repeats reuse both. |
| Git discovery unavailable | `unknown unknown` on both builds, with deliberate conservative reruns; restored discovery recovers the correct revision and warm reuse. |

All 27 execution logs report kernel `ECHILD (__WALL)` settlement and matching
lease release; no fixture `cargo-active` directory remained. Retained final proof
files used **28,049,408 allocated bytes**, deduplicated by device/inode, excluding
directory blocks. This is retained usage, not a sampled or exact peak measurement.
`results.json`, `*.log`, and `*.preflight.log` remain under that local proof root.

Historical control: extracted `ee028f61a1^:crates/perl-lsp-rs/build.rs` into
`/workspace/git-metadata-old-build.rs` and ran the same command with that script
and proof root `/workspace/git-metadata-proof-red`. **3 expected failures in
3.812s**: absent-packed warm/recovered-repeat rebuilt both crates with Cargo's
`the file .../.git/packed-refs is missing`; linked warm rebuilt both with
`the file .../.git/HEAD is missing`. The printed revision remained correct;
the failure was the freshness assertion.

Freshness retention control: replaced only the call to `track_revision_inputs()`
in an external script copy with a watch of `build.rs` alone. Running just
`GitMetadataTests.test_absent_packed_refs_commit_and_consumer_edit` with that copy
and proof root `/workspace/git-metadata-proof-no-watch` produced **1 expected
failure in 1.259s**. Cold and warm worked, but after a commit both crates stayed
fresh and printed stale `acc091d` instead of `a96f7a5`. Removing metadata watches
therefore cannot satisfy this fixture.

Independent read-only review inspected the current fixture digest, final 27 rows,
historical red logs and exact production-script copy identity; no findings.
The reviewer did not rerun Cargo. `git diff --check` passed.

## Boundaries

This is a tiny Cargo/production-build-script regression, not a full LSP performance
claim. Discovery is suppressed from the nested crate using
`GIT_CEILING_DIRECTORIES`; admission still sees a repository. Recovery changes that
environment variable. Literal exports, metadata appearance under an unchanged
environment, exotic Git layouts, Windows, and full-package behavior are not proven
by this suite. Existing production fallback behavior remains authoritative.

No broad CI, PR-triggered builds, deployment, release, cache adoption, cleanup,
profile change or production `build.rs` edit is part of this evidence.
