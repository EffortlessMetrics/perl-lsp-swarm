# CLAUDE.md (perl-sync-convergence)

## Role

Canonical `perl_lsp.convergence_transaction.v1` persistence for continuous
source synchronization (#11282). Owns durable transaction identity,
content-addressed immutable generation receipts, writer leases and takeover,
the invalidation graph, closed lifecycle/action vocabularies, and the
append-only journal whose replay reconstructs current state and next legal
actions in a fresh process. Published to crates.io; public API by design.

## Owns

- Identity: `TransactionId`, `GenerationId` (`gen:sha256:…` wire form,
  domain-separated SHA-256 over length-prefixed exact inputs).
- Receipts: `ConvergenceGeneration` + versioned `GenerationReceiptFile`
  wrapper; identity is re-derived and compared on every validation.
- Vocabularies: `Direction`, `ReleaseContextMode`, `TransitionState` (14
  states), `PermittedAction`, `InvalidationCause`, `StaleDisposition`.
- Leases: `Lease`, `Takeover`, `TimestampMs`; expiry/heartbeat math with an
  injected clock.
- Invalidation: `InvalidationRecord`, `StaleDescendant`.
- Journal/replay: `ConvergenceEvent`, `replay`, `is_legal_transition`,
  `permitted_writer_actions`, `ReplayError(Kind)`.
- Store: `ConvergenceStore` file layout, fail-closed loads, atomic writes,
  receipt immutability, transaction index.

## Does not own

Per the #11003 decomposition: projection semantics (#10996), event intake and
coalescing (#11284), candidate publication and review maps (#11285), source
admission, merging, live protection/settings mutation, sync-health reporting
(#11289), reverse convergence. This crate never shells out to Git or GitHub;
callers supply exact SHAs/trees and timestamps. The v0.18 release R/S/J/M
transaction stays governed elsewhere; release-specific mode here only keeps
the vocabulary distinct.

## Invariants

- Generation identity is derived from exact inputs: moved inputs change the
  ID, forcing a successor generation; receipts are never edited (identical
  canonical rewrite is idempotent, different bytes are refused).
- Closed enums everywhere; unknown spellings and unsupported schema versions
  are serde/load errors, never defaults. No state can fold into pass from
  `not_proven`, `instrument_failure`, or `rejected`.
- Replay is total and deterministic over the journal alone; concurrent active
  generations for one direction+source parent without supersession are
  refused at append time.
- Takeover requires a complete reconciliation record against an expired
  displaced lease; live leases cannot be taken over and grant no merge or
  ref-mutation authority.
- Persisted artifacts contain no secrets, credentials, raw host paths, or
  unbounded logs; expiring URLs never stand in for digests.

## Neighbors

- Upstream deps: `serde`, `sha2` only; exact allowlist asserted by
  `tests/dependency_contract.rs` (fails closed when `cargo tree` cannot run).
- Downstream consumers: the #11003 controller family — #11284 (intake),
  #11285 (publication), #11289 (health) — plus future reverse-convergence.

## Read first

- Issue #11282 (schema contract), controller #11003 (decomposition).
- `src/event.rs` — transition legality and replay rules.
- `src/store.rs` — on-disk layout and fail-closed loading.
- `crates/perl-source-identity` — the Tier-1 leaf precedent this crate
  follows for domain-separated IDs and dependency contracts.

## Focused validation

```bash
cargo test -p perl-sync-convergence --all-targets --locked
cargo fmt -p perl-sync-convergence -- --check
cargo clippy -p perl-sync-convergence --all-targets --locked -- -D warnings
```

Fixtures in `tests/convergence_persistence.rs` cover crash recovery, lease
expiry/takeover, duplicate writers, input movement, release-mode confusion,
rejection immutability, unresolved-state honesty, and malformed/
version-mismatched persisted state.

## Review hotspots

- Any new dependency fails `tests/dependency_contract.rs` until reviewed into
  `PERMITTED`.
- New events must extend replay's closed rules; an unhandled variant would be
  a compile error (match exhaustiveness), but a new *transition* needs an
  explicit legality decision in `is_legal_transition`.
- Schema evolution bumps `*_SCHEMA_VERSION` constants so older readers reject
  rather than misread.
