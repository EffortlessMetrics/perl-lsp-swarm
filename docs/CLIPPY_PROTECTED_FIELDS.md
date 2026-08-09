# Protected fields for `clippy::disallowed_fields`

Status: design anchor for DF-1 in the Rust 1.95 rollout. This document does not
activate `clippy::disallowed_fields`, add a `clippy.toml` selector, or change Rust
code. It defines the seams that a later policy ledger and accessor work may select.

The lint is an architectural guard, not a ban on ordinary field access. A field is
eligible only when its owner has a stable invariant, the boundary is clear, and an
accessor can preserve that invariant without hiding ordinary data-model access.

## Protected classes

| Class | Invariant | Protected boundary | Today's surface | Failure if direct access spreads |
| --- | --- | --- | --- | --- |
| Redaction internals | Copyable explanations and trust reports must not expose raw workspace roots, launch paths, environment values, or secrets when a class, count, or hash is sufficient. | Serialization and copyable bug-report payload construction. | `crates/perl-lsp-rs-core/src/providers/provider_decision.rs`, the runtime workspace-trust report, and `PLSP-SPEC-0012` / `PLSP-SPEC-0016`. | A new consumer can bypass the redaction boundary and publish private host data. |
| Bundle and artifact paths | Paths used to build or describe bundles are implementation inputs, not portable user-facing identity. | Release, corpus, harness, and receipt metadata crossing into reports or uploaded artifacts. | Release-evidence and core-harness receipt surfaces carry path-bearing metadata; the concrete field selector remains an inventory task for DF-2. | Host-specific paths leak into artifacts or make receipts non-reproducible across runners. |
| Trust receipts | A receipt must preserve the decision, source, freshness, confidence, and fallback boundary while remaining safe to copy and replay. | Receipt construction and schema serialization. | Provider-decision receipts, runtime refactor receipts, and `crates/perl-core-harness-types`. | A consumer can emit an incomplete or unsafe receipt that looks like proof but loses its safety boundary. |
| Source opaque IDs | Fact identity is deterministic, host-path-free, and derived from repo-relative content rather than time, traversal order, or randomness. | Construction and projection of file, package, symbol, and content identities. | `crates/perl-workspace-core/src/id.rs` (`Digest`, `FileId`, `PackageId`, and `SymbolId`) and its linked ADR. | Consumers can couple identity to absolute paths or incidental traversal state, invalidating caches and cross-run comparisons. |
| Cache internals | Bounded cache capacity, eviction, TTL, synchronization, and statistics remain owned by the cache implementation. | Access from workspace consumers into `BoundedLruCache` storage and accounting. | `crates/perl-workspace/src/workspace/cache.rs` (`CacheConfig`, `CacheStats`, `CacheEntry`, and `BoundedLruCache`). | Callers couple to representation, bypass eviction/accounting rules, or make future cache changes unsafe. |
| Policy ledger metadata | Lint policy and debt metadata are parsed and validated by one policy owner; active, planned, and expiring entries must not drift silently. | Policy loading, validation, and policy-derived decisions. | `policy/clippy-lints.toml`, `policy/clippy-debt.toml`, and `xtask/src/tasks/check_lint_policy.rs`. | Ad hoc field access creates validators that disagree about levels, status, MSRV, ownership, or expiry. |

## Selection rules for the next slices

DF-2 should turn these classes into a machine-readable ledger. Each entry should
name one owning module, one concrete field selector, the invariant it protects, an
owner, and the proof that the selector is currently unused or intentionally migrated.
The ledger must not be treated as permission to activate a selector.

DF-3 should select the smallest concrete class, introduce an accessor at the owning
boundary, and prove the invariant with focused tests. It must not combine unrelated
field classes or use a lint suppression as a substitute for an accessor.

DF-4 may then add the selected field paths to `clippy.toml` and promote the matching
planned policy entry. It must activate one class at a time and require a clean
workspace Clippy run. The empty configuration remains valid before DF-4.

The following are deliberately out of scope:

- banning all `String`, `Path`, receipt, cache, or metadata fields;
- restricting ordinary private implementation details without a named invariant;
- changing public API shapes or serialization contracts in the preparation slice;
- activating the lint before a concrete owner and field selector have been proved;
- treating a planned ledger row, a passing policy parser, or a green Clippy run as
  proof that a protected seam is correctly designed.

## Proof obligations

The preparation slice is complete when the policy parser accepts the planned entry,
the internal links resolve, and the document remains consistent with the Rust 1.95
rollout. The later activation slice must additionally prove the selected accessor,
the absence of unintended direct access, and the full Clippy gate.

See also:

- [`docs/development/RUST_1_95_ROLLOUT.md`](development/RUST_1_95_ROLLOUT.md) for DF-1
  through DF-4 sequencing and acceptance contracts.
- [`docs/CLIPPY_POLICY.md`](CLIPPY_POLICY.md) for the active policy and suppression
  rules.
- [`policy/clippy-lints.toml`](../policy/clippy-lints.toml) for the planned lint row.
