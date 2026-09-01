# Context: #11076 — versioned ProcessPlan, ProcessEvent, ProcessResult, and supervisor ports

## Origin

P1 of the shared-process train under controller #4842, itself the first
implementation slice of process epic #4836. The remaining train — #11078 Linux
spawn, #11082 lifecycle and cleanup, #11085 receipts and conformance, #1975
direct-spawn inventory — all depend on the contract this packet establishes.

Reconciled against `origin/main@e175dc3` on 2026-09-01.

## Reconciliation findings on current main

- `git grep 'ProcessPlan\|ProcessSupervisor\|ProcessResult'` over `crates/`
  returns nothing: the domain is absent, matching the last research pass
  recorded on #4836 (2026-08-14).
- `crates/perl-subprocess-runtime` is the crate to evolve, as #4842 states. It
  is a genuinely clean substrate: `[dependencies]` is empty,
  `#![deny(unsafe_code)]`, and the Windows binary-planting defence
  (`resolve_command_invocation`, #2764/#3028) already fails closed rather than
  falling back to a bare program name.
- Its whole public execution surface is
  `SubprocessRuntime::run_command(program, args, stdin)` plus
  `OsSubprocessRuntime::{new, with_timeout, with_bounded_timeout}`. There is no
  cwd, no environment projection, no authorization, no budget, no cancellation,
  no process group, and no terminal-cause precedence — `wait_with_timeout`
  polls `try_wait` every 50 ms, calls `child.kill()` on the immediate child
  only, and returns a `SubprocessError` whose only machine content is a string.
- Direct consumers are `perl-lsp-perltidy`, `perl-lsp-rs-core`, and
  `perl-lsp-rs`. None is migrated by this packet.
- No open PR or branch collides with this claim (`is:pr` search for
  ProcessPlan/ProcessSupervisor/#11076 returns nothing).
- `cargo xtask check-architecture` and `cargo xtask semantic-scorecard`, named
  in the issue's verification block, are not current subcommands. The current
  spellings used here are `cargo xtask gates --tier pr-fast --receipt`
  (`just pr-fast`) plus the crate-focused `fmt`/`clippy`/`test` commands.

## Old owner → new owner

| Concern | Old owner on main | New owner |
|---|---|---|
| what to execute | caller's `&str` program and `&[&str]` args | `ProcessPlan::{executable, argv}` with `ExecutableIdentity` |
| where it runs | ambient process cwd | `CwdPolicy`, enforced per profile |
| environment | fully inherited, implicit | `EnvironmentProjection` (declarative) |
| authorization | none | `AuthorizationEvidence` (opaque; #1753 owns semantics) |
| output bounds | none — `wait_with_output` is unbounded | `CaptureBudget` per channel |
| deadline | `OsSubprocessRuntime::timeout_secs` | `DeadlinePolicy` |
| cancellation | none | `CancellationPolicy` + `ProcessHandle::cancel` |
| cleanup | `child.kill()` on the immediate child | `TerminationPolicy` + `TreeDisposition` |
| how a run ended | `Result<SubprocessOutput, SubprocessError>` | closed `TerminalDisposition` |
| what a run does not prove | nothing recorded | `Limitation`, `EvidenceClass` |

## Claim boundary

This packet is types, validation, ports, and fakes. It performs no
operating-system spawn, changes no live caller, and adds no receipt producer.
The legacy seam stays compilable and is contained rather than deleted, because
deleting it would rewrite every live consumer inside a contract PR.
