# Linux parallel admission qualification, 2026-10-15

This is bounded behavioral evidence for Steven's release-readiness campaign,
not caller migration, general workload sizing, or release approval. Default
retirement remains held pending measured caller migration. No production repair
was justified by these observations; the added tests protect previously uncovered
cross-process contention and cancellation combinations.

## Subjects and method

- Successor: `c8b1705a938fd1ff2e28645fded3b9439dc99818`, plus the test file
  committed with this report. Production source was unchanged.
- Compared main: `4cc7960e61b685ce3b7e4cb9fe43b929bf900972`. Its lease and
  tree lifecycle code is identical to the successor. Capacity policy differs:
  the successor permits an explicitly declared environment byte-budget reserve
  below 40 GiB, while retaining normal percentage-policy defaults.
- Negative policy control: `ba7268eef4438addac3dc347999a4252fbccad49`.
- Linux 6.18.44, CPython 3.12.14, installed Cargo/Rust 1.95.0
  (`cargo 1.95.0 (f2d3ce0bd 2026-03-21)`); no provisioning/downloads.

`scripts/tests/test_cargo_admitted_parallel.py` invokes the production Python
entrypoint as independent processes, with real Git repository/worktree identity,
actual filesystem capacity checks, scope-bound budget files, and kernel child
ownership. Neither capacity nor resource planning is mocked in this suite.
The fake Cargo writes exactly three 16-KiB payloads and forks a finite detached
helper. The opt-in real Cargo test compiles a dependency-free Rust binary offline,
which launches that same payload. Cargo home, target, build, and temporary files
are fixture-private. An independent outer subreaper contains failing controls;
its cleanup validates kernel waitability before signaling bound pidfds.

Declare only this fixture's aggregate growth: 8 MiB fake / 128 MiB real, with
256 MiB reserve. These declarations are not an OS quota or measured portable
upper bound. File sizes and allocated blocks are sampled approximately every
10 ms while waiting; transient unsampled peaks and filesystem metadata are
excluded. No shared build cache is used or cleaned.

## Observations and regression discrimination

Run against the successor, completed around 03:57 UTC:

```sh
CARGO_ADMITTED_SMALL_REAL_TOOLCHAIN=/workspace/shared/rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin \
  python scripts/tests/test_cargo_admitted_parallel.py -v
```

**4 passed, 1 explicitly skipped; 5.474 seconds.**

| Probe | Actual result |
| --- | --- |
| Six simultaneous same-slot callers | Exactly one launches; five refuse with 75. |
| Leader exits while detached helper lives | A new caller still refuses with 75; original lease and marker inodes persist. |
| Natural descendant completion and reuse | Helper observes lease present; kernel ECHILD precedes original-marker removal; a subsequent request succeeds. |
| Linked sibling worktree | Same common lease, different target/build domain; denied sibling allocates no outputs. |
| Repeated SIGINT then SIGTERM during settlement | TERM-resistant detached child is reaped; status 130, ECHILD, original lease/marker removed; unrelated caller child survives. |
| Real offline Cargo build plus contender and cancellation | Same refusal/settlement behavior; 2.106 s cancellation; sampled peak and retained allocated file bytes 9,408,512 (logical 9,291,550). |
| Fake alternate-profile cancellation | 2.093 s; sampled peak and retained allocated file bytes 159,744 (logical 82,304). |

The same five tests against extracted main source, selected with
`CARGO_ADMITTED_PARALLEL_OWNER=/absolute/path/to/main.py`, produced **4 passed,
1 skipped, 5.584 seconds**, including real Cargo. This is separate main evidence,
not a claim that either commit contains the new tests already.

A controlled wrong implementation replaced only
`settlement = tree.finish(stop=interrupted or sys.exc_info()[0] is not None)`
with `settlement = {"tree_settled": True, "cancelled": False}` in a disposable
copy selected through `CARGO_ADMITTED_PARALLEL_OWNER`. The parallel regression
failed in 0.300 s at the intended refusal assertion: **0 != 75**. The second
request was incorrectly admitted while the first detached helper remained alive.
Independent fixture cleanup completed. This demonstrates discrimination against
leader-only release; it is not a defect observed in the successor.

Separate mocked policy arithmetic used 1 TiB total capacity, no sizing overrides,
and 600/100 GiB free. Main and successor admitted 600 GiB with the existing
42,949,672,960-byte reserve and refused 100 GiB at 90.234375% used. `ba7268`
refused both for missing an explicit reserve. Thus the predecessor is a negative
compatibility control, not a negative lifecycle control. These synthetic policy
observations make no host-admission or measured-growth claim.

## Review and remaining limits

An independent read-only review ran the default opt-out suite: 3 passed, 2
explicit skips. It found no blocking oracle flaw; its setup-cleanup finding was
repaired by registering subreaper restoration and temporary-directory cleanup
immediately after acquisition, before the final successor run. The independent
review did not repeat the real Cargo build. The full 136-test replay remains
owned elsewhere and was not duplicated.

This host lacks `/proc/<pid>/task/<tid>/children`. An initial default-profile
cancellation probe timed out at eight seconds without settlement proof; the
test's initial cleanup also assumed that unavailable table. Cleanup now uses
parent candidates plus kernel waitability. Default-profile cancellation is
explicitly skipped here; the tested cancellation profile is explicitly
`CARGO_ADMITTED_CHILD_DISCOVERY=proc-parent-pidfd`, without silent fallback or
production changes. Default-profile natural completion and contention did run.
Windows ownership, actual Windows builds, broad dependencies, sustained child
churn, hard disk enforcement, crash/power-loss recovery, and caller migration
remain unqualified. Source review noted an unbounded inner drain in `finish()`;
no sustained-churn reproducer established a defect, so it was not changed.

Live ownership lookup `gh pr list --limit 60 --json
number,title,headRefName,url` failed: `Post "https://api.github.com/graphql":
Forbidden`. No alternate API was used. Git source fetch succeeded. Existing
owner branches were preserved; this proof uses its own branch based on the
requested successor. Publication/review through GitHub API remains unverified;
no merge, workflow dispatch, release, deployment, or protection change occurred.

Fixture target/build/home/temp resources are released after verified test-owned
process cleanup in the corrected suite. Final process inspection also found
orphan zombie PID `984` (`cargo`, PPID 1), consistent with the initial failed
cleanup probe; it is not a running consumer and cannot be reaped by this worker.
Its kernel entry awaits PID 1 reaping. No live fixture process remained at that
inspection. The isolated source worktree and local negative-control/log
evidence are retained for handoff. Existing shared caches and owner resources
remain retained and untouched.

## Independent challenge and scoped hardening, 2026-10-15

Review subject: `c297b09e608dac8f95115ab1b6d6ab669453c0a9`, based on
`c8b1705a938fd1ff2e28645fded3b9439dc99818`. Refreshed `origin/main` resolves to
`4cc7960e61b685ce3b7e4cb9fe43b929bf900972`. Git advertises the original owner
branch `test/linux-admission-adversary-20261015` at the exact review subject.
The independent review and test hardening use a separate review branch; neither
that owner branch nor production source is modified.

The exact original suite was replayed on Linux 6.18.44, CPython 3.12.14 with the
already installed 1.95.0 toolchain using the command above: **4 passed, 1 skipped,
6.047 seconds**. The default opt-out replay had **3 passed, 2 skipped**. These are
fresh execution observations on the original candidate, separate from the earlier
runs above. Default-profile cancellation remains explicitly skipped for the same
missing proc child table; no fallback was added.

A separate read-only proof review and executable counterexamples found narrow
oracle gaps rather than a demonstrated production defect:

- A forwarding fake Cargo passed the original opt-in “real Cargo” test without
  compiling Rust. The strengthened fixture requires the Rust program to write
  its `current_exe()` witness, matching the expected target executable with an
  ELF header. That same fake now fails for the missing Rust witness. This binds
  the observation to execution of the fixture binary; it is not toolchain
  provenance attestation.
- The linked sibling case now asserts different build paths as well as targets,
  and no refused build-directory allocation. A disposable common-build-path
  mutant fails the new comparison. Existing storage tests already cover this
  path partition and alternating real-worktree builds; contention alone still
  does not establish divergent-source compiler freshness.
- A new alternate-profile case cancels after the leader's product receipt while
  the detached helper holds the owner. It then requires a fresh helper launch,
  one new payload execution, renewed contender refusal, and natural settlement
  with lease presence observed by that helper. A disposable mutant that poisons
  the target on cancellation passes initial settlement but fails fresh reuse.
- Cancellation now binds pidfds to the live payload leader and helper before
  interruption, requires exit readiness for both afterward, and preserves the
  unrelated-child survival control. These independent observations supplement
  owner-reported ECHILD; they do not independently enumerate every Cargo/Rust
  intermediary or prove absence of all possible descendants.

The hardened suite on unchanged production source, with the same opt-in command,
produced **5 passed, 1 skipped, 8.193 seconds**. Real cancellation took 2.109 s;
retained and sampled allocated-file maxima were 9,437,184 bytes (logical
9,315,399). The original 9,408,512-byte observation is **9.408512 decimal MB**,
not an exact peak. Sampling excludes unsampled transients and filesystem
metadata; neither value proves an upper bound on total storage or portable
workload sizing. The declared aggregate budgets and reserve remain fixture-only.

Live owner/review/hold refresh remains **NOT_PROVEN**: `gh api
repos/EffortlessMetrics/perl-lsp-swarm/commits/c297b09e608dac8f95115ab1b6d6ab669453c0a9/pulls`
returned `Get "https://api.github.com/repos/EffortlessMetrics/perl-lsp-swarm/commits/c297b09e608dac8f95115ab1b6d6ab669453c0a9/pulls": Forbidden`.
No alternate API was used. Git source/branch observations do not establish live
PR ownership, review disposition, protections, or user holds. The default-retirement
hold and every existing qualification limit above remain in force. No broad CI,
workflow dispatch, integration, release, deployment, or protection change was made.

Final independent read-only diff review found no blocking oracle issue after the
repairs; it reviewed the execution and negative-control results without repeating
the real build. Affected policy/path controls also ran with
`python scripts/tests/test_cargo_admitted_storage.py`: **26 passed, 1 skipped**
(27 tests, 0.304 seconds). Its optional alternating real-worktree build was not
repeated. No production repair was warranted. Fixture processes and private
build/home/temp resources were released by test cleanup; the isolated source
worktree is retained for the review handoff, with shared caches untouched.
