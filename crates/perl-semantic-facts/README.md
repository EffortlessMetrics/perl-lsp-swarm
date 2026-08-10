# perl-semantic-facts

`perl-semantic-facts` defines a neutral, serializable semantic fact vocabulary shared
across parsing, semantic analysis, and workspace indexing layers.

This crate intentionally does **not**:
- parse Perl source,
- provide LSP behavior,
- own workspace storage/index persistence.

It provides stable typed identifiers, facts, confidence/provenance metadata, and enums
for entity/occurrence/edge classification.

## Canonical provider envelope

`SemanticFactEnvelope` is the transport contract between compiler/workspace producers
and provider adapters. It carries a stable fact identity, source anchor and generation,
scope/package and lifecycle context, producer/provenance/confidence metadata, freshness,
boundary linkage, invalidation dependencies, and a stable reason code.

Providers can call `SemanticFactEnvelope::status()` without inspecting AST, HIR, or PIR:

- `Exact` means all required identity and authority metadata is known, confidence is high,
  freshness is `Fresh`, the reason code is `ExactSource`, and no boundary is linked.
- `Degraded` means the fact is usable as advisory/shadow evidence but has reduced certainty.
- `Refused` means a linked boundary explicitly forbids promotion or the reason code is
  `UnsupportedEffect`, even when no boundary link is present.
- `Stale` means the fact or one of its dependency generations is no longer current,
  including when a dependency generation is unknown, empty, or otherwise cannot be
  verified.

Unknown values for generation, provenance, confidence, lifecycle, producer, or reason code are
explicit and can never classify as `Exact`; only `Known(High)` confidence qualifies for an exact
result. Only `SemanticFreshness::Fresh` can classify as `Exact`: `Unknown`, `NotApplicable`,
and any future non-fresh state degrade, while `Stale` remains a distinct status. Exact reason
codes are positively allowlisted; unrecognized serialized variants normalize to `Unknown`, so
future reason-code variants fail closed to `Degraded`. Dependency order is canonicalized by the
constructor and on deserialization so serialized receipts remain deterministic.
