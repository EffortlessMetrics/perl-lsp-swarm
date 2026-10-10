# Rust agent workflow audit — 2026-10-10

Dated reproducible engineering evidence for
[#3390](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3390),
[#3230](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3230), and
[#11606](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11606).
This note is not a command manual, execution framework, or admission decision.

## Subject and evidence ceiling

Audited checkout: `ac383369c40494bd873d9657e8c881b7e8792000`.
Read `AGENTS.md`, the authority-status registry, relevant `.agents/skills`, current
source and live issue plans. Source inspection and Python TOML parsing only:
no Cargo build, hosted CI, cleanup, hook installation, or settings mutation.
Reported environment evidence from the audit: `df -h /workspace` showed
`overlay 32G 2.5M 30G 1% /workspace`. Its exact capture timestamp was not retained.
The approximately 30 GiB available was below the admitted route's 40 GiB reserve;
this observation does not establish workload growth or fleet capacity.
Refresh source and prerequisite behavior before implementing a packet.

## Ranked verified gaps

| Priority | Current source evidence | Existing owner / next action |
| --- | --- | --- |
| P0 | `justfile` routes agent recipes and PR-fast through `cargo-safe`; other checks use raw Cargo. `scripts/cargo-safe` shares target/build paths by checkout basename, and the `xtask` alias bypasses its heavy-command flock case. AGENTS requires private target and intermediate paths. | #3230 / #9549; consume #17477 / PR #17478 and #17479 / PR #17460 before front-door migration. |
| P0 | `scripts/cargo_admitted.py::validate_args` refuses Clippy and aliases. Raw AGENTS Clippy and legacy agent-Clippy therefore do not form a supported admitted route. | #3230; canonical executor #11660 and adoption #11663 when qualified. |
| P0 | Windows `gates.rs::shell_command_process` uses `cmd /C`; `unit_routed_full` policy uses `$PWD`, `env` and a compound build/test command. | #17482: launch semantics and actual runtime-result validation together. Native Windows proof remains required. |
| P1 | `justfile::check-all-targets` contains five distinct modes. Policy allows 900 seconds; `ci.yml::check-all-targets` allows 35 minutes. The current job has 38 non-comment Cargo-test command lines across 42 steps (including unnamed `uses` steps), beyond the historical four-contract tail description. | #17483: inventory the complete tail and retain per-mode completion evidence. |
| P1 | Pre-push new-branch and package-name resolution invokes heavyweight xtask. TOML inspection found 16 unconditional product/parser dependency roots in xtask. Package lookup still has a directory-name fallback. | #17480 / #17481 / #17484 own first-mile repair. |
| P1 | `gates.rs` dispatches fmt, publication, layer/count, inline-completion and commit tasks in-process. Moving only the gate file cannot remove the product graph. The policy shard includes product-dependent children. | #17485 after #17484: one shared engine and one real policy-lane consumer; retain explicit product children. |
| P1 | `targeted_checks.rs` selects only `crates/*`, uses library-only tests and Clippy without all-targets, and blocks in `duct.run()`. Non-crate changes can return “No crate changes.” | #9549; consume topology/route facts #12125 / #12126 without claiming full coverage. |
| P1 | Pushed worktree removal does not dispose of external private target/build resources. Ordinary porcelain status omits ignored evidence. `target-gc --apply` correctly refuses. | Worktree apply #10263; external-state planning #11671; host/process observation #11664 / #11666. |
| P2 | Agent recipes select the agent profile, agent-PR-fast does not; targeted children use defaults. Doctor recommends legacy routing and checks external target location rather than both canonical resources. | Compiler-profile worker plus #3230 / #8606 front-door reconciliation. |
| P2 | Shell gates have tree termination/watchdogs; targeted checks, admitted `subprocess.call`, and internal gate closures do not share that enforcement. | Consume existing process/executor owners, preserving uncertain-consumer lease retention. |

## Reported measurements and unmeasured hypotheses

#17483 reports 7m17s plus 7m00s for the first two compile modes: 857 seconds,
leaving 43 seconds in a 900-second envelope. These are reported samples, not
reproduced timings or universal budgets. Current source comments record earlier
hosted cancellations after green compilation; they do not measure this checkout.

Product-independent bootstrap should avoid unrelated compiler work, but elapsed
savings, compiler-unit counts, peak additional disk growth, cache performance and
cold/warm throughput remain unmeasured. Private paths prevent a known sibling
freshness hazard; they do not prove every artifact consumer is settled or that
worktree-path reuse is safe. No issue closure or green wrapper replaces execution
and currentness evidence.

## Reproduce without compiling the product

1. Record `git rev-parse HEAD`; inspect `AGENTS.md` and
   `docs/agents/CARGO_STORAGE.md` before comparing front doors.
2. Read `justfile`, `scripts/cargo-safe` and
   `scripts/cargo_admitted.py::validate_args`; compare both target/build paths,
   accepted subcommands and the alias lock boundary. Do not invoke a build to
   demonstrate a source-level refusal.
3. Read `xtask/Cargo.toml` with Python `tomllib`; enumerate unconditional
   dependency keys beginning with `perl-` or `tree-sitter`, excluding entries
   marked optional. The count above is direct dependency roots, not transitive
   compiler units or a measured build cost.
4. Read the five `check-all-targets` recipe commands and
   `.ci/gate-policy.yaml::compile_all_targets`. Bound the hosted job block from
   `check-all-targets:` to the next job; count all step markers (including unnamed `uses` steps) and non-comment
   Cargo-test command lines. These counts describe source, not executed test populations.
5. Inspect `gates.rs::shell_command_process`, internal task dispatch and
   `targeted_checks.rs::run_checks`. Preserve platform and compile/runtime
   distinctions; source inspection cannot qualify native Windows behavior.
6. Inspect the cleanup predicate, ignored-file treatment, external resource
   descriptor and retired target-GC apply path. Do not run cleanup to inspect it.
7. Record `df -h` for the actual destination and compare the current floor.
   Free bytes do not establish additional peak growth or reservation safety.

## Limits and use

Existing issues remain the work and decision history. This note does not replicate
PR status, assign workers, or add a dependency tracker. Before acting, read the
current owner issue and actual landed interfaces. Preserve all existing compile
modes and proof populations. A local wrapper success, compile-only result, pushed
branch or returned parent does not establish runtime completion or cleanup safety.

The stable audit lesson is to verify the actual command, executable, subject,
profile/features, both writable build resources and terminal consumers together.
Measure any speed or storage claim separately from command movement. Useful next
issue updates should reference this evidence rather than require a repeat audit.
