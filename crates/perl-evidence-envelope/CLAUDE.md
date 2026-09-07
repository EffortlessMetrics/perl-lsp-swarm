# CLAUDE.md (perl-evidence-envelope)

## Role

Canonical `evidence_envelope.v1` domain-neutral outer evidence record and
deterministic envelope identity for the Perl toolchain's evidence
infrastructure (see `README.md` for the controlling issue and epic). The
outer shell every domain-specific evidence payload (test receipts, coverage
receipts, gate receipts, and kinds not yet designed) is carried inside, so
those sibling slices can consume real canonical types instead of ad hoc
structs. `publish = false`: internal evidence infrastructure until a
production consumer exists — greenfield, no `EvidenceEnvelope`/`GateEnvelope`
types existed anywhere in the repository before this crate.

## Owns

- Schema version: `EVIDENCE_ENVELOPE_SCHEMA_VERSION_V1` +
  `EvidenceEnvelopeSchemaVersion` with fail-closed serde (mirrors
  `perl-source-identity`'s `SourceIdentitySchemaVersion` exactly).
- `ReceiptId` — durable identity for one receipt instance, domain-separated
  SHA-256 of a caller-supplied canonical key (`receipt:sha256:...`).
- `PayloadIdentity` — payload kind (opaque string), the payload's own schema
  version (independent of the envelope's schema version), and a
  `perl_source_identity::ContentDigest` over the exact payload bytes.
- `ProducerIdentity` — name, version, source SHA, build identity.
- `EvidenceSubject` — owning `perl_source_identity::ProjectId`, optional
  base/head/candidate/artifact reference strings, and `RunIdentity`
  (`RunSource::Workflow`/`Local` + run ID + attempt).
- `Completeness` — `Complete`/`Partial`/`NotApplicable`/
  `StructurallyUnavailable`/`Stale`/`Invalid`, `#[non_exhaustive]`, always a
  required field on `EvidenceEnvelope` (see Invariants).
- `RedactionClass`, `RetentionClass` — `#[non_exhaustive]` classification
  enums with no `Default` impl and no implicit fallback variant.
- `ClaimBoundary` (established/not-established statement lists) and
  `Limitation` (detail + optional affected claim).
- `InputReference` — lineage edge as data only: an upstream `ReceiptId` paired
  with the `ContentDigest` it had when consumed.
- `EnvelopeFingerprint` — deterministic, domain-separated SHA-256 over the
  full envelope's canonically-ordered fields (`envfp:sha256:...`).
- `EvidenceEnvelope` — the top-level record tying all of the above together.

## Does not own

Per the controlling issue's explicit non-goals, this crate does not own: a
receipt registry schema or loader, a validation library that enforces
domain-specific payload shapes, a freshness rule registry, a JSON Schema
projection of these types, CLI tooling, fixture migrations, or any production
receipt/CI gate change. It also does not perform **cycle detection** over
`InputReference` lineage edges — those are carried as data only; a sibling
issue owns lineage graph validation. It does not validate
`PayloadIdentity::kind` values or
`PayloadIdentity::schema_version` semantics — those belong to the
domain-specific payload's own owning crate, not this domain-neutral shell.

Like `perl-source-identity`, this crate must not depend on: any parser
implementation (AST/HIR/PIR), `perl-workspace` or the ProjectModel runtime,
LSP/DAP/editor types, async runtimes, or Git/release tooling. Asserted in
`tests/dependency_contract.rs`.

## Invariants

- Fail-closed deserialization: an `EvidenceEnvelope` with an unsupported
  `schema_version`, a malformed `ReceiptId`/`ContentDigest`/`ProjectId`
  anywhere inside it, an unrecognized enum variant name, or a **missing**
  `completeness` or `inputs` field is a serde error — never a value that
  flows onward. This is deliberate: a permissive decode that defaulted
  `completeness` to `Complete` when absent would let an envelope with zero
  recorded inputs be silently read as fully complete evidence.
- `EnvelopeFingerprint` is deterministic and canonicalizes every `Vec`-valued
  field (`claim_boundary.established`, `claim_boundary.not_established`,
  `limitations`, `inputs`) by sorting before hashing, so insertion order never
  affects the result. Two envelopes differing only in `Vec` element order
  produce the same fingerprint; two envelopes differing in any scalar field or
  in the actual multiset of collection elements do not.
- `EnvelopeFingerprint` uses its own domain tag
  (`perl-lsp:evidence-envelope-fingerprint:v1`), distinct from
  `perl-source-identity`'s `perl-lsp:content-digest:v1`. The same raw bytes
  hashed under the two domains never produce the same digest.
- `RedactionClass` and `RetentionClass` do not implement `Default` and carry
  no variant marked as an automatic fallback; a producer must state a value.
- `ReceiptId` is independent of `EnvelopeFingerprint`: re-minting a receipt
  for byte-identical evidence is valid and expected (e.g. a reproducible
  re-run), so the two identities intentionally answer different questions.

## Neighbors

- Upstream: `perl-source-identity` (path/workspace dependency, reused for
  `ContentDigest`/`ProjectId`), `serde`, `sha2`. The full transitive closure
  is pinned by an exact allowlist test — see below.
- Downstream: none yet. This is greenfield infrastructure; the receipt
  registry, validator, freshness-rule, JSON-schema-projection, and CLI slices
  of the parent issue (see `README.md`) are the intended future consumers.

## Read first

- `src/lib.rs` — the crate-level contract: what this crate owns/does not own,
  dependency bans, quick start.
- `src/fingerprint.rs` — `EnvelopeFingerprint`'s canonical field order,
  collection canonicalization, and domain separation from `ContentDigest`.
- `src/envelope.rs` — the top-level `EvidenceEnvelope` record and the
  fail-closed completeness/schema-version invariants.
- `tests/dependency_contract.rs` — the fail-closed `cargo tree` allowlist
  asserting the lower-crate boundary.
- `README.md` — wire formats and the completeness discipline.

## Focused validation

`cargo test -p perl-evidence-envelope --all-targets --locked`. The dependency
contract shells out to `cargo tree` and fails closed when the instrument
cannot run. Unit tests cover fingerprint determinism (with a negative
control), fingerprint independence from `Vec` insertion order, domain
separation from `ContentDigest`, fail-closed schema-version and
completeness/inputs decoding, and serde round-trips for every enum variant.

## Review hotspots

- Any new dependency fails `tests/dependency_contract.rs` until it is
  reviewed into the exact `PERMITTED` allowlist.
- A change to `EnvelopeFingerprint`'s field walk in `fingerprint.rs` changes
  every existing fingerprint's value — this is a breaking change to anything
  that has persisted a fingerprint, not merely a refactor.
- Adding a field to `EvidenceEnvelope`, `PayloadIdentity`, `ProducerIdentity`,
  `EvidenceSubject`, `ClaimBoundary`, `Limitation`, or `InputReference` must
  also add it to `EnvelopeFingerprint::of`'s field walk, or the new field
  becomes silently unfingerprinted.
- Enum-valued fields are fingerprinted by their `fingerprint_tag()` string,
  never by `self as u8`. The discriminant is declaration-order dependent, and
  these enums are `#[non_exhaustive]` specifically so variants can be inserted
  — so discriminant hashing would let a pure source reorder silently change
  the fingerprint of already-produced evidence whose JSON is byte-identical.
  A `fingerprint_tag` string is part of the durable identity contract:
  renaming one is a breaking change, not a rewording.
- `fingerprint_is_stable_for_a_known_envelope` pins the exact fingerprint of a
  fully-populated envelope. It is the only test that catches a change applied
  *uniformly* to the walk (reordering the walk, renaming a tag, inserting an
  enum variant, adding a fingerprinted field) — the determinism and
  order-independence tests all compare fingerprints only to each other and
  stay green through every one of those. If it fails, establish that the
  identity change is intended before updating the constant.
- `Completeness`/`inputs` stay required, never `#[serde(default)]`: relaxing
  either to a default reopens the exact silent-completeness failure mode this
  crate exists to close.

## Claim boundary

This crate makes the envelope's shape and identity fail-closed and
deterministic; it does not make any specific payload's content honest, does
not validate that `completeness`/`redaction_class`/`retention_class` values
are the *correct* classification for what actually happened (only that some
explicit value was stated), and does not detect cycles in the lineage graph
`InputReference` edges describe. Those responsibilities live with the
domain-specific producer above this crate and the sibling validation/registry
slices of the parent issue.
