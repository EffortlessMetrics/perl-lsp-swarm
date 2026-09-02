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
crates/perl-subprocess-runtime/tests/process_domain_contract.rs   85 tests
```

## Schema version and digest

- `PROCESS_DOMAIN_SCHEMA_VERSION = 1`.
- Locked canonical fingerprint of the `valid_linux_one_shot` fixture:
  `1ec851f73284fbebad7abfd4c5662ac8`. Moving the encoding's meaning without
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

## Privacy tiers

Two tiers, now explicit in the types' own documentation:

- **fingerprinted** — `PrivatePath`, `PrivateBytes`: the raw value never
  reaches a canonical encoding, `Debug` output, or a public identity, but a
  digest does, so differing content gives differing plan identities;
- **excluded** — `SecretValue`: contributes nothing at all, not even a digest,
  because a digest of a low-entropy secret is a guessable secret.

A password or token belongs in a `SecretValue` environment addition, never in
stdin bytes.

## Deliberate non-normalizations

- No async arrangement is imposed. `ProcessHandle` is synchronous; #11078 may
  add an async adapter over it if the runtime architecture calls for one.
- No serde, no schema file, no receipt producer. `process_execution.v1` is
  #11085's.
- No `Instant`/`SystemTime` use anywhere in the domain, so it stays
  wasm32-compatible like the rest of the crate.
- `PROCESS_DOMAIN_SCHEMA_VERSION` stays at 1 through the post-review repair.
  Version movement protects *shipped* meaning; v1 has never been on `main`, so
  there is no consumer for an amendment to be incompatible with.
- Env var *set membership* (allow/deny/remove) is byte-exact; only
  code-loading *detection* folds ASCII case, which is the fail-safe direction.
  A future Windows profile will need case-insensitive membership too.

## Handoffs

| Next node | What it can now build against |
|---|---|
| #11078 (P2, Linux spawn) | `ValidatedProcessPlan` as its sole input; `EventLedger` for ordering; `ObservedSettlement` to report what the child did |
| #11082 (P3, lifecycle) | `ControlState` + `TerminalDisposition::elect` as the pure precedence rule its OS mechanics populate; `CleanupDisposition`/`TreeDisposition` for what it proves |
| #11085 (P4, receipts) | `ProcessResult`, `Limitation`, `EvidenceClass`, and the canonical encoder for a redacted public projection |
| #1975 (P5, inventory) | `LEGACY_CONTAINMENT` as the seed row set for the seam ledger |
| consumer lanes (#10258, #9689, #9691, #10666, #4900) | `FakeSupervisor` for deterministic tests before any OS lane exists |

## Generated artifacts

`docs/policy/NON_RUST_INVENTORY.md` is regenerated: the three `.spec/` files
this packet adds are tracked non-Rust files, and the `non_rust_inventory_check`
merge gate requires the committed snapshot to match. They classify as
`documentation` under the existing `non-rust-root-governance-docs` entry, so
the unclassified count is unchanged at 2239 — no new policy debt.

## Base integration

`origin/main` moved to `384f8052` while this claim was in flight, and #14536
regenerated `docs/policy/NON_RUST_INVENTORY.md` — the same generated file this
packet touches — producing a real content conflict. Resolved by merging the
base branch in and regenerating the snapshot with `cargo xtask non-rust
inventory --write` rather than hand-editing it. Both sides survive: main's
vscode rows and this packet's `.spec/` rows are present, and the unclassified
count is unchanged at 2239.

## Verification run

```bash
cargo fmt -p perl-subprocess-runtime -- --check
cargo clippy -p perl-subprocess-runtime --all-targets --locked -- -D warnings
cargo test -p perl-subprocess-runtime --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc -p perl-subprocess-runtime --no-deps --locked
cargo check -p perl-lsp-perltidy -p perl-lsp-rs-core --all-targets --locked
just pr-fast
cargo xtask non-rust inventory --check
```
