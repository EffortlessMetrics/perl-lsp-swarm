# Context: #11661 — expose the canonical Cargo executor command and durable receipts

## Problem

Issue #11661 (train position EXE-06) requires one canonical operator/automation
command that exposes the programmatic Cargo executor transaction (#11660) through a
typed CLI, publishes one versioned durable `cargo_operation_result.v1` operation
object, derives human/JSON/explain/exit from that one object, and emits a narrow
rerun/reproduce packet. No legacy-wrapper or caller migration belongs to this claim.

The command must not turn free-form Cargo text into a bypass, must not flatten typed
result planes into one green/red string, and its receipt conventions must consume
existing repository authorities rather than create a competing generic envelope.

## Status: spec-only this lane; live cut `BLOCKED_BY_PREREQUISITE`

Verified live on 2026-08-23 against `origin/main@f1e74db90` (current head) and
against live GitHub state:

| Prerequisite | Role for #11661 | State when verified | Open PR claiming it |
|---|---|---|---|
| #11642 | accepted executor-model decision feeding #11647's mandatory model identity | **open** | none |
| #11647 (EXE-01) | pure domain: `CargoOperationSubject/Request/Result`, result planes, composition laws | **open** | none |
| #11650 / #11653 / #11659 | build-state allocation, host-capacity reservations, native process supervision | **open** (all three) | none |
| #11660 | canonical programmatic Cargo transaction the command must invoke | **open** | none |
| #9550 (+ parent #8606) | canonical command identity/metadata registry ("exact naming follows current command conventions") | **open** | none |

Searches performed: repository-wide `rg "cargo_executor|CargoExecutor"` over
`xtask crates .github justfile scripts` returns zero symbols; `gh pr list --state
all` (all states incl. drafts) contains no PR referencing #11647/#11660/#11661 or
the canonical-executor seam. No rival candidate exists.

Implementing the command today would require inventing, in this lane:

1. the subject/request/result vocabulary that is #11647's owned claim;
2. the executable transaction that is #11660's owned claim;
3. command identity/metadata rows ahead of any #9550 registry authority to bind to.

Each duplicates a sibling lane's unlanded authority and creates exactly the
"competing envelope" this issue prohibits. The repository's controlling precedent is
the 2026-08-20/21 deep review on #10817 (applied in merged `.spec/10817-*` /
PR #11971): where structural prerequisites are unlanded, compile the required
`.spec` packet, pin current-main evidence, pre-design discriminating fixtures, gate
the cut behind a named wake event, and do not freeze a durable wrong seam. This
packet applies that ruling to EXE-06.

## Current-main facts the future builder consumes (`main@f1e74db90`)

### Legacy surface still live (replacement owner #11662, not this claim)

- `scripts/cargo-safe:5-18` derives `devplane`, cargo-home, target, build,
  sccache, tmp, and lock roots from ONE directory name (`DEVPLANE` or default);
  #9548's ownership amendment records this as the mistake the typed scopes must
  not recreate.
- `scripts/cargo-safe:60-66` runs UNLOCKED when `flock` is absent — the exact
  silent-fallback the executor must replace with typed
  `QUEUE/LOCK/SETUP/INSTRUMENT NOT_PROVEN/BLOCKED`.
- `justfile:5` binds `cargo_safe := "./scripts/cargo-safe"`; pr-fast/test/clippy/
  nextest/gates flows route through it. None of these callers move in EXE-06.

### Receipt/evidence authorities to consume (no new envelope)

- Schema-versioned receipt objects + JSON-schema cross-check tests are the house
  pattern: `xtask/src/tasks/session_receipt.rs:54-96` mirrors
  `.ci/receipts/schemas/session-start.schema.json`, enforced by the struct-vs-
  schema test at session_receipt.rs:549-584. The `cargo_operation_result.v1`
  object follows this pattern (schema file under `.ci/receipts/schemas/`,
  fail-closed `Option` fields per session_receipt.rs:23-29 doctrine).
- Single-writer atomic publication already exists as a convention:
  `xtask/src/publication_drift/mod.rs:246-263` — unique `NamedTempFile::new_in`
  under the output parent → write → `sync_all` → `persist(rename)` → no shared
  predictable `.tmp`. The receipt publisher reuses this shape inside an
  operation-owned output root with content/operation-bound final names.
- Gate receipts land under dated per-gate paths via `gates --receipt`
  (`justfile` gate-receipt targets writing `.receipts/$DATE/$gate.json`);
  retention classification stays with existing policy owners.
- Command registration today = clap variants on the `Commands` enum in
  `xtask/src/main.rs:76ff`; until #9550 lands its typed registry, that enum IS
  the current command convention, and the post-wake builder registers there and
  records the row #9550 later absorbs.

### What does NOT exist yet (wake dependencies)

No `cargo_executor*` module, no `cargo_operation_result` type, no
`CargoOperationSubject/Request/Result` types, no executor transaction API, and no
command-registry metadata authority exist on current main.

## Why this approach

The issue's own train position places it strictly after "#11660 + #9550", its
implementation order begins with the `.spec` tables, and its stop conditions name
predecessor semantics and "inventing a generic evidence framework". Compiling the
packet now gives the post-wake builder reviewed ground — exact consumption seams,
failure tables, fixture designs, and retarget notes — instead of a frozen guess at
three sibling lanes' APIs.

## Alternatives rejected

- **Register `cargo-executor run|validate|explain|reproduce` now against a locally
  invented request/result vocabulary**: rejected — duplicates #11647's owned types
  and #9550's owned identity authority before either exists; spelling frozen now
  would become a deprecated-alias/deletion liability under #9550's own falsifiers.
- **Implement a minimal internal transaction behind the command**: rejected — the
  transaction is #11660's claim; even a minimal spawn+lock+env implementation makes
  state/capacity/process claims owned by #9548/#11650/#11653/#11659 and creates a
  rival candidate seam.
- **Define `cargo_operation_result.v1` field-by-field in Rust now**: rejected —
  plane vocabulary and composition laws belong to #11647; this packet records the
  receipt CONTRACT (what must be bound, atomicity, validation) and defers exact
  type names to the landed domain.
- **Migrate `scripts/cargo-safe` or any Just/Nix/hooks caller in the same PR**:
  rejected by the issue (non-goals) and by the campaign brief; #11662 owns adapter
  reduction.

## Prior art / duplicates

- `.spec/10817-client-configuration-observations/` (merged via PR #11971,
  commit f1e74db90) — same dependency shape, same sanctioned spec-first ruling;
  structure reused here.
- `session_receipt.rs` fail-closed receipt doctrine (post-review hardening on
  PR #3866): every unverifiable field stays `None`, never a plausible fact.
- `publication_drift` atomic-write convention (above) — prior art for the
  receipt publisher.
- No `.spec/11661-*` packet existed prior to this bundle.

## Links

- Issue: [#11661](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11661)
- Parent controller: #9548; goal umbrella: #11869
- Train: EXE-01 #11647 → composition #11660 → **EXE-06 #11661** → adapter #11662
- Model decision: #11642; command identity: #9550 / #8606
- State/capacity/process identities: #11650, #11653, #11659
- Caller consumers (NOT migrated here): #9549 / #9554 / #11630 / #9567

## Scope boundary

In scope: this directory's `context.md`, `acceptance.md`, `checklist.md`.

Out of scope until the checklist Step 0 wake gate passes: all production code, the
executor domain types (#11647), the transaction (#11660), command registry
authority (#9550), state/capacity/process leaves (#11650/#11653/#11659),
`scripts/cargo-safe` changes (#11662), every caller migration
(#9549/#9554/#11630/#9567), route-plan/fan-in, tooling provisioning, and product
behavior.
