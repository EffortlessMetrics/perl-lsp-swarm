# perl-evidence-envelope

Canonical `evidence_envelope.v1` domain-neutral outer evidence record and
deterministic envelope identity (issue #15057, parent #4843, epic #4837).

**Internal evidence infrastructure — `publish = false`.** This crate has no
production consumers yet. It exists so the sibling slices of #4843 (a receipt
registry, a validation library, a freshness rule registry, a JSON Schema
projection, and CLI tooling) can consume real canonical types instead of
locally invented structs, without reopening this contract per sibling.

## What this crate owns

- **[`EvidenceEnvelopeSchemaVersion`]** — fail-closed schema version for the
  outer envelope shape.
- **[`ReceiptId`]** — durable identity for one receipt instance.
- **[`PayloadIdentity`]** — the domain-specific payload's kind, its own schema
  version, and a reused `ContentDigest` over its exact bytes.
- **[`ProducerIdentity`]** — name, version, source SHA, build identity.
- **[`EvidenceSubject`]** / **[`RunIdentity`]** / **[`RunSource`]** — what (and
  which run) this evidence is about.
- **[`Completeness`]** — explicit, never-inferred completeness classification.
- **[`RedactionClass`]**, **[`RetentionClass`]** — explicit classification
  enums with no implicit fallback.
- **[`ClaimBoundary`]**, **[`Limitation`]** — what the evidence establishes,
  and what it explicitly does not.
- **[`InputReference`]** — lineage edges to upstream receipts, as data only.
- **[`EnvelopeFingerprint`]** — deterministic identity over the whole
  envelope.
- **[`EvidenceEnvelope`]** — the top-level record.

## Reuse, not reinvention

This crate depends on `perl-source-identity` for `ContentDigest` and
`ProjectId` rather than re-deriving domain-separated content/project
identity. The only new hashing here is `EnvelopeFingerprint`, which genuinely
needs a domain tag distinct from `ContentDigest`'s so the two can never
collide (see [Domain separation](#domain-separation) below).

## Completeness is explicit

`EvidenceEnvelope::completeness` and `EvidenceEnvelope::inputs` are both
required fields with no `serde` default. A zero-length `inputs` array is
never, by itself, evidence of completeness: an envelope can be `Complete`
with zero inputs (nothing was ever applicable — use
`Completeness::NotApplicable` when that's the actual reason) or `Partial`
with a hundred inputs (more were expected). An envelope that omits either
field fails to deserialize rather than silently reading as complete.

## Deterministic fingerprint

`EnvelopeFingerprint::of(&envelope)` (or `envelope.fingerprint()`) is:

- **Deterministic** — identical envelope contents always produce the same
  fingerprint, computed fresh each time.
- **Independent of `Vec` insertion order** — `claim_boundary.established`,
  `claim_boundary.not_established`, `limitations`, and `inputs` are each
  canonically sorted before hashing, so reordering elements within any of
  these lists does not change the fingerprint.
- **Sensitive to every scalar field and to the actual multiset of collection
  elements** — changing any other field, or the actual set of inputs/
  limitations/claims (not merely their order), changes the fingerprint.

### Domain separation

`EnvelopeFingerprint` uses the domain tag
`perl-lsp:evidence-envelope-fingerprint:v1`, distinct from
`perl-source-identity`'s `perl-lsp:content-digest:v1`. Hashing the same raw
bytes under the two domains produces different digests — an
`EnvelopeFingerprint` can never be mistaken for, or collide with, a
`ContentDigest`.

## Dependency contract

This crate **must not** depend on (and does not depend on):

- any parser implementation (AST/HIR/PIR);
- `perl-workspace` or the ProjectModel runtime;
- LSP/DAP/MCP/editor types;
- `tokio` or other async runtimes;
- Git, release workflows, repository receipts, or VS Code.

This is asserted by `tests/dependency_contract.rs`.

## Non-goals (deferred to sibling issues of #4843)

- Receipt registry schema and loader.
- A validation library that enforces domain-specific payload shapes.
- Freshness rule registry.
- JSON Schema projection of these types.
- CLI tools.
- Fixture migrations or registry changes.
- Production receipt or CI gate modifications.
- **Cycle detection** over `InputReference` lineage edges — carried as data
  only in this crate.

## Wire formats

| Type | Wire format |
|---|---|
| `ReceiptId` | `receipt:sha256:<64 lowercase hex>` |
| `EnvelopeFingerprint` | `envfp:sha256:<64 lowercase hex>` |
| `ContentDigest` (reused) | `sha256:<64 lowercase hex>` |
| `ProjectId` (reused) | `project:sha256:<64 lowercase hex>` |

Every wire form is **validated on deserialization**, matching
`perl-source-identity`'s discipline: uppercase hex and wrong prefixes are
rejected rather than normalized.

## Quick start

```rust
use perl_evidence_envelope::{
    ClaimBoundary, Completeness, EvidenceEnvelope, EvidenceEnvelopeSchemaVersion,
    EvidenceSubject, PayloadIdentity, ProducerIdentity, ReceiptId, RedactionClass,
    RetentionClass, RunIdentity, RunSource,
};
use perl_source_identity::{ContentDigest, ProjectId};

let envelope = EvidenceEnvelope {
    schema_version: EvidenceEnvelopeSchemaVersion::V1,
    receipt_id: ReceiptId::from_canonical_key("run-42"),
    payload: PayloadIdentity::new("test-receipt", 1, ContentDigest::of_bytes(b"payload bytes")),
    producer: ProducerIdentity::new("perl-lsp-test-runner", "0.17.0", "abc123", "ci-42"),
    subject: EvidenceSubject::new(
        ProjectId::from_canonical_name("acme/widget"),
        Some("base-sha".to_string()),
        Some("head-sha".to_string()),
        None,
        None,
        RunIdentity::new(RunSource::Workflow, "run-1", 1),
    ),
    completeness: Completeness::Complete,
    redaction_class: RedactionClass::Public,
    retention_class: RetentionClass::Standard,
    claim_boundary: ClaimBoundary::empty(),
    limitations: Vec::new(),
    inputs: Vec::new(),
};

assert!(envelope.is_schema_supported());
let fingerprint = envelope.fingerprint();
assert_eq!(fingerprint, envelope.fingerprint());
```

## License

Licensed under either of Apache License, Version 2.0 or MIT License at your
option.
