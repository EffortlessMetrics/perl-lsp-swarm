# CLAUDE.md (perl-source-identity)

## Role

Canonical `source_identity.v1` core types for the Perl toolchain transport
layer: durable, content-addressable identity for projects, workspace roots,
logical sources, exact content revisions, freshness, and origin. The smallest
transport-neutral substrate, so downstream consumers take real canonical types
instead of opaque strings, path hashes, or locally invented references.
Published to crates.io (`publish = true`); public API surface by design.

## Owns

- Durable ID newtypes: `ProjectId` (from an authority-defined canonical
  project name), `WorkspaceRootId` (project plus authority-defined root key),
  `LogicalSourceId` (root plus root-relative path; revision-independent).
- Content identity: `ContentDigest` (SHA-256 over exact bytes) and
  `ContentRevision` (logical source paired with its digest).
- Freshness vocabulary: `SourceGeneration` (`Known(label)` / `Unknown`;
  `Unknown` is the default and must never be read as current).
- Origin vocabulary: `SourceOrigin` and `PhysicalSourceRole`, both
  `#[non_exhaustive]`, with explicit `Unknown` variants instead of implicit
  fallbacks.
- `SourceIdentityEnvelope`: the schema-versioned top-level record answering
  ownership, logical source, content revision, generation, origin, and role,
  plus `SourceIdentitySchemaVersion` with fail-closed serde decoding.

## Does not own

Per the crate doc comment and PLSP-ADR-0006, this crate sits below the whole
product stack and must not depend on: any parser implementation (AST/HIR/PIR),
`perl-workspace` or the ProjectModel runtime, LSP/DAP/editor types, async
runtimes, or Git/release tooling. It also does not normalize paths (callers
pass canonical forward-slash root-relative paths), does not map origins to
physical ranges or redact locations, and does not own workspace-local
`FileId`/`PackageId`/`SymbolId` identity — those stay FNV-1a-minted inside
`perl-workspace-core` (`fnv64:` wire prefix), a deliberately different
concern from these `sha256:`-prefixed durable IDs.

## Invariants

- All durable IDs are domain-separated SHA-256: each type hashes under a
  unique domain tag via `DomainHasher`, so IDs of different kinds never
  collide even with identical material, and every field is length-prefixed so
  field boundaries cannot shift (`["a", "bc"]` and `["ab", "c"]` differ).
- One identity, one spelling: wire forms are `sha256:…`, `project:sha256:…`,
  `root:sha256:…`, `src:sha256:…` with exactly 64 lowercase hex digits;
  wrong prefixes and uppercase hex are rejected, not normalized, because
  equality and hashing are defined over the wire string.
- Fail-closed deserialization at the serde boundary: ill-formed IDs/digests
  and unsupported envelope schema versions are serde errors. Public
  constructors can still create values that require caller checks, including
  unsupported schema versions and semantically inconsistent envelope fields.
- No host path, URI, traversal-order counter, or process-local value ever
  becomes stable identity.

## Neighbors

- Upstream: `serde` and `sha2` only; the full transitive closure is pinned by
  an exact allowlist test (see below).
- Downstream: `perl-parser` (incremental snapshots use `ContentDigest`) and
  `perl-lsp-rs-core` (tooling result identity).

## Read first

- `src/lib.rs` — the crate-level contract: identity hierarchy, dependency
  bans, quick start.
- `src/digest.rs` — `DomainHasher`, length prefixing, wire validation.
- `tests/dependency_contract.rs` — the fail-closed `cargo tree` allowlist
  asserting the lower-crate boundary.
- `README.md` — wire formats and the FNV-1a boundary discussion.
- `docs/adr/PLSP-ADR-0006-perl-workspace-core-facts-substrate.md` — the
  recorded scope decision for why this crate owns durable identity.

## Focused validation

`cargo test -p perl-source-identity --all-targets --locked`. The dependency
contract shells out to `cargo tree` and fails closed when the instrument
cannot run. Unit tests cover determinism, cross-type and cross-domain
collision resistance, uppercase rejection, validating serde at every boundary,
and envelope schema-version rejection.

## Review hotspots

- Any new dependency fails `tests/dependency_contract.rs` until it is reviewed
  into the exact `PERMITTED` allowlist; a denylist alone would silently admit
  anything nobody thought to forbid.
- A new ID kind must take a unique domain tag and its own wire prefix;
  reusing a domain tag silently merges two identity namespaces.
- Envelope changes must preserve `schema_version` semantics: compatible
  additions without a bump; breaking changes require a bump so older builds
  reject.
- Envelope constructors make provenance fields explicit but do not validate
  their semantic relationships. Producers must ensure that project, root,
  logical-source, and optional revision values describe the same source before
  consumers treat the envelope as coherent.

## Claim boundary

This crate makes representation and wire-shape errors explicit at the type
level; it does not make provenance truthful by construction or decide when
envelopes are minted or how unknown generations surface to users. Freshness
honesty toward the editor, and semantic consistency among producer-supplied
fields, live with the producers and consumers above this crate.
