# Protected fields for `clippy::disallowed_fields`

Status: selector-design anchor. `clippy::disallowed_fields` is already active at
workspace deny. `clippy.toml` remains deliberately empty, and the active ledger
row records `configuration_state = "empty-by-design"`. The current configured
selector denominator and protected-seam denominator are both zero. This document
selects no production field and changes no Rust code.

The lint is an architectural guard, not a ban on ordinary field access. A field is
eligible only when its owner has a stable invariant, the boundary is clear, and an
accessor can preserve that invariant without hiding ordinary data-model access.

## Protected classes

| Class | Invariant | Protected boundary | Today's surface | Failure if direct access spreads |
| --- | --- | --- | --- | --- |
| Redaction internals | Copyable explanations and trust reports must not expose raw workspace roots, launch paths, environment values, or secrets when a class, count, or hash is sufficient. | Serialization and copyable bug-report payload construction. | `crates/perl-lsp-rs-core/src/providers/provider_decision.rs`, the runtime workspace-trust report, and `PLSP-SPEC-0012` / `PLSP-SPEC-0016`. | A new consumer can bypass the redaction boundary and publish private host data. |
| Bundle and artifact paths | Paths used to build or describe bundles are implementation inputs, not portable user-facing identity. | Release, corpus, harness, and receipt metadata crossing into reports or uploaded artifacts. | Release-evidence and core-harness receipt surfaces carry path-bearing metadata; the concrete field selector remains an inventory task for #11252. | Host-specific paths leak into artifacts or make receipts non-reproducible across runners. |
| Trust receipts | A receipt must preserve the decision, source, freshness, confidence, and fallback boundary while remaining safe to copy and replay. | Receipt construction and schema serialization. | Provider-decision receipts, runtime refactor receipts, and `crates/perl-core-harness-types`. | A consumer can emit an incomplete or unsafe receipt that looks like proof but loses its safety boundary. |
| Source opaque IDs | Fact identity is deterministic, host-path-free, and derived from repo-relative content rather than time, traversal order, or randomness. | Construction and projection of file, package, symbol, and content identities. | `crates/perl-workspace-core/src/id.rs` (`Digest`, `FileId`, `PackageId`, and `SymbolId`) and its linked ADR. | Consumers can couple identity to absolute paths or incidental traversal state, invalidating caches and cross-run comparisons. |
| Cache internals | Bounded cache capacity, eviction, TTL, synchronization, and statistics remain owned by the cache implementation. | Access from workspace consumers into `BoundedLruCache` storage and accounting. | `crates/perl-workspace/src/workspace/cache.rs` (`CacheConfig`, `CacheStats`, `CacheEntry`, and `BoundedLruCache`). | Callers couple to representation, bypass eviction/accounting rules, or make future cache changes unsafe. |
| Policy ledger metadata | Lint policy and debt metadata are parsed and validated by one policy owner; active, planned, and expiring entries must not drift silently. | Policy loading, validation, and policy-derived decisions. | `policy/clippy-lints.toml`, `policy/clippy-debt.toml`, and `xtask/src/tasks/check_lint_policy.rs`. | Ad hoc field access creates validators that disagree about levels, status, MSRV, ownership, or expiry. |

## Successor train

Phase 1 stops at a validated empty configuration. The first protected seam remains
a separate, ordered train:

1. [#11252](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11252)
   selects one concrete authority-bearing field from the candidate classes above.
2. [#11254](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11254)
   introduces the canonical owner API and migrates every non-owner direct consumer.
3. [#11255](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11255)
   adds exactly one reasoned selector and a direct-bypass falsifier, then removes
   the now-stale `empty-by-design` marker.

Selection requires one stable owner, one fully qualified field, one bypassed
invariant, one canonical replacement, a complete current consumer denominator,
and behaviorally discriminating proof. A getter that merely returns mutable
internals is not an architecture seam.

The following are deliberately out of scope:

- banning all `String`, `Path`, receipt, cache, or metadata fields;
- restricting ordinary private implementation details without a named invariant;
- changing public API shapes or serialization contracts in the preparation slice;
- adding a production selector before its owner, invariant, replacement, consumer
  denominator, and bypass proof have been established;
- treating a planned ledger row, a passing policy parser, or a green Clippy run as
  proof that a protected seam is correctly designed.

## Proof obligations

Phase 1 is complete only when the policy checker requires the `clippy.toml` hook,
requires the explicit empty-state marker, and a synthetic Clippy fixture proves
that a configured field is rejected. A later selector slice must additionally
prove the owner API, complete consumer migration, exact selector resolution, and
the direct-bypass failure under the full Clippy gate.

See also:

- DF-1 through DF-4 sequencing remains issue-owned by
  [#9850](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9850).
- [`docs/CLIPPY_POLICY.md`](CLIPPY_POLICY.md) for the active policy and suppression
  rules.
- [`policy/clippy-lints.d/00-active.toml`](../policy/clippy-lints.d/00-active.toml)
  for the active lint row and explicit empty-state marker.
