# Checklist: #11076

## What landed

```text
crates/perl-subprocess-runtime/src/process/
  mod.rs          module contract, re-exports, PROCESS_DOMAIN_SCHEMA_VERSION
  encoding.rs     canonical encoder + FNV-1a 128 fingerprints
  identity.rs     schema version, PrivatePath/SecretValue/PrivateBytes,
                  correlation ids, OwnerDomain, ExecutionProfile,
                  ExecutableIdentity, CwdPolicy, SubjectIdentity,
                  AuthorizationEvidence, PlatformRequirement
  environment.rs  EnvironmentProjection, code-loading vocabulary
  plan.rs         ProcessPlan + builder and every policy type
  validation.rs   the pure validator, PlanRejection, ValidatedProcessPlan
  event.rs        ProcessEvent, ProcessEventKind, EventLedger
  result.rs       StreamEvidence, TerminalDisposition, ProcessResult
  port.rs         ProcessSupervisor, ProcessHandle, drop contract
  fake.rs         FakeSupervisor, ScriptedRun
  legacy.rs       containment record for the pre-domain seam
crates/perl-subprocess-runtime/tests/process_domain_contract.rs   49 tests
```

## Schema version and digest

- `PROCESS_DOMAIN_SCHEMA_VERSION = 1`.
- Locked canonical fingerprint of the `valid_linux_one_shot` fixture:
  `4e12acd15f88bdd2955ceeb7a54819b3`. Moving the encoding's meaning without
  moving the version fails
  `the_canonical_encoding_of_a_fixture_plan_is_locked_to_the_schema_version`.

## Removed or forwarding helpers

None. Nothing on `main` was deleted, renamed, or rewired. `SubprocessRuntime`,
`OsSubprocessRuntime`, `mock`, `resolve_program`, and the Windows resolver are
untouched; the only edit to `lib.rs` is documentation plus `pub mod process`.

## Legacy containment

`process::legacy::LEGACY_CONTAINMENT` records `SubprocessRuntime::run_command`
and `OsSubprocessRuntime` as `OwnerDomain::LegacyAdapter`, closed to new
consumers, removal owned by `#1975`, with the ten capabilities they cannot
express. `SubprocessRuntime`'s own rustdoc states the same. Enforcement of "no
new private spawn" is #1975's ratchet, not this PR's.

## Deliberate non-normalizations

- No async arrangement is imposed. `ProcessHandle` is synchronous; #11078 may
  add an async adapter over it if the runtime architecture calls for one.
- No serde, no schema file, no receipt producer. `process_execution.v1` is
  #11085's.
- No `Instant`/`SystemTime` use anywhere in the domain, so it stays
  wasm32-compatible like the rest of the crate.
- Env var names are compared byte-exact; no case folding, which is correct on
  the Linux profile this train targets and is a known gap for a future Windows
  profile.

## Handoffs

| Next node | What it can now build against |
|---|---|
| #11078 (P2, Linux spawn) | `ValidatedProcessPlan` as its sole input; `EventLedger` for ordering; `ObservedSettlement` to report what the child did |
| #11082 (P3, lifecycle) | `ControlState` + `TerminalDisposition::elect` as the pure precedence rule its OS mechanics populate; `CleanupDisposition`/`TreeDisposition` for what it proves |
| #11085 (P4, receipts) | `ProcessResult`, `Limitation`, `EvidenceClass`, and the canonical encoder for a redacted public projection |
| #1975 (P5, inventory) | `LEGACY_CONTAINMENT` as the seed row set for the seam ledger |
| consumer lanes (#10258, #9689, #9691, #10666, #4900) | `FakeSupervisor` for deterministic tests before any OS lane exists |

## Verification run

```bash
cargo fmt -p perl-subprocess-runtime -- --check
cargo clippy -p perl-subprocess-runtime --all-targets --locked -- -D warnings
cargo test -p perl-subprocess-runtime --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc -p perl-subprocess-runtime --no-deps --locked
cargo check -p perl-lsp-perltidy -p perl-lsp-rs-core --all-targets --locked
just pr-fast
```
