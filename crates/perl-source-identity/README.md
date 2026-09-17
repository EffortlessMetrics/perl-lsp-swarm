# perl-source-identity

Canonical `source_identity.v1` core types for the Perl toolchain transport layer.

This crate provides the smallest transport-neutral implementation substrate for
`source_identity.v1` so downstream crates can consume real canonical types
instead of opaque strings, path hashes, or locally invented references.

## What this crate owns

- **[`ProjectId`]** — stable project identity across machines, roots, and sessions.
- **[`WorkspaceRootId`]** — identity for one checkout of a project at a specific root.
- **[`LogicalSourceId`]** — stable, revision-independent identity for one logical
  source file within a root.
- **[`ContentDigest`]** — collision-resistant SHA-256 digest of exact byte content.
- **[`ContentRevision`]** — a logical source paired with its exact content digest.
- **[`SourceGeneration`]** — explicit `Known`/`Unknown` freshness cursor.
- **[`SourceOrigin`]** — where the source came from (workspace, virtual, generated,
  staged, upstream, runtime-derived, or unknown).
- **[`PhysicalSourceRole`]** — the physical/functional role of the file.
- **[`SourceIdentityEnvelope`]** — the complete `source_identity.v1` envelope that
  answers all ownership, identity, content, generation, and origin questions.

## Identity guarantees

All durable IDs use **SHA-256** with domain separation:

- Fixed inputs always produce byte-identical IDs across machines and builds.
- No host path, URI, traversal-order counter, or process-local value becomes
  stable identity — for the *logical path*, `RootRelativeLogicalPath` now
  enforces this (see below). The project name and root key remain
  authority-defined caller material, and the low-level
  `LogicalSourceId::from_root_and_path` still hashes whatever it is given, so
  this is a guarantee about the governed constructors rather than about every
  public entry point.
- IDs of different kinds never collide even when their material inputs match
  (each type uses a unique domain prefix).
- Fields are length-prefixed so `["a", "bc"]` and `["ab", "c"]` hash
  differently.

## Dependency contract

This crate **must not** depend on (and does not depend on):

- any parser implementation (AST/HIR/PIR);
- `perl-workspace` or the ProjectModel runtime;
- LSP/DAP/MCP/editor types;
- `tokio` or other async runtimes;
- Git, release workflows, repository receipts, or VS Code.

This is asserted by `tests/dependency_contract.rs`.

## Non-goals (deferred to child issues)

- The remaining authority-bound constructor matrix — open buffer, staged,
  upstream, harness, generated, DAP/runtime source roles (issue #7655).
  The canonical **logical-path** invariant is enforced here: see
  `RootRelativeLogicalPath` below.
- Path *normalization* of any kind. Logical paths are validated, never rewritten:
  no separator folding, no traversal resolution, no case folding, no Unicode
  normalization, and no I/O. Physical-path mechanics belong to issues #7621,
  #8185 and #8198.
- Origin/range mappings or redacted location projection (issue #7659).
- ProjectFactShard/TestItem/DAP/RIPR consumer migration.
- Provider or file-lifecycle behavior change.
- Global content-addressed filesystem.

## Wire formats

| Type | Wire format |
|---|---|
| `ContentDigest` | `sha256:<64 lowercase hex>` |
| `ProjectId` | `project:sha256:<64 lowercase hex>` |
| `WorkspaceRootId` | `root:sha256:<64 lowercase hex>` |
| `LogicalSourceId` | `src:sha256:<64 lowercase hex>` |

Every wire form is **validated on deserialization**. Each type accepts exactly
one spelling of its value: the type prefix is mandatory, the digest body must be
exactly 64 hex digits, and uppercase hex is rejected rather than normalized —
equality and hashing are defined over the wire string, so admitting two
spellings would give one identity two values. A wire string minted for one ID
kind does not parse as another. `SourceIdentityEnvelope` likewise rejects a
`schema_version` this build does not support, instead of decoding it as though
it were v1.

Together this means a value of any of these types, however it was obtained, is
well-formed. `from_wire` returns `Option`; `serde` returns an error.

## `RootRelativeLogicalPath`: validation, not normalization

`LogicalSourceId` is *root* plus *root-relative path*. That second half used to be
a promise in a doc comment, so a caller could hand it a host absolute path, a `..`
traversal or an empty string and get back a perfectly well-formed durable ID for
the wrong source — and two callers normalizing the same file differently both
produced valid-looking, different IDs.

`RootRelativeLogicalPath::parse` makes the promise checkable. Its only constructor
is fallible, so holding the type *is* the proof, and
`LogicalSourceId::from_root_and_logical_path` cannot be handed anything else.

| Refused | Why not repaired |
|---|---|
| empty | names nothing |
| leading `/` | stripping it turns a host path into a plausible relative path |
| `\` anywhere | an unconverted separator and a literal POSIX backslash are indistinguishable without platform authority; folding would alias `we\ird.pm` onto `we/ird.pm` |
| `C:` prefix | drive-qualified paths are not root-relative |
| `.` / `..` segment | resolving lexically can escape the root; resolving truthfully needs the filesystem |
| `//` run, trailing `/` | collapsing lets several spellings validate to one file while hashing differently |
| control / NUL | not representable, and usually a truncated or injected value |

Two properties worth stating explicitly:

- **Nothing is folded.** Case is preserved (`lib/App.pm` ≠ `lib/app.pm`) and bytes
  are preserved (NFC ≠ NFD), so identity reflects what the producer actually named
  rather than this crate's Unicode tables. Case-insensitive filesystem contexts
  need declared authority and remain #7655's business.
- **Errors carry no path.** `LogicalPathError` holds only a reason, and its
  `Display`/`Debug` never echo the rejected material — which is exactly the
  material most likely to be a private host path.

`from_root_and_path` remains available as the low-level constructor over material
already proven canonical. It is the one digest implementation; the governed
constructor delegates to it, so validation cannot move published wire vectors.

## Ownership and the FNV-1a boundary

This crate owns `source_identity.v1` — durable source/project/content identity
that must survive across machines, checkouts, and time. That is a different
concern from `perl-workspace-core`'s workspace-local `FileId`/`PackageId`/
`SymbolId`, which PLSP-ADR-0006 mints with FNV-1a within a single extraction
run. FNV-1a offers no collision resistance and so cannot carry durable
cross-repository identity; the two coexist unambiguously because both wire forms
are explicitly prefixed (`fnv64:` vs `sha256:`).

See the "Scope boundary: `source_identity.v1`" section of
[PLSP-ADR-0006](../../docs/adr/PLSP-ADR-0006-perl-workspace-core-facts-substrate.md)
for the recorded decision. `LogicalSourceId` is **not** an alias for
`perl-workspace-core`'s `FileId`.

## Quick start

```rust
use perl_source_identity::{
    ContentDigest, ContentRevision, LogicalSourceId, ProjectId,
    RootRelativeLogicalPath, SourceGeneration, SourceIdentityEnvelope,
    WorkspaceRootId,
};

let project = ProjectId::from_canonical_name("https://github.com/acme/widget");
let root = WorkspaceRootId::from_project_and_root_key(&project, "abc123");

// Validated: a host absolute path, a `..` traversal or an empty string cannot
// become a durable logical source identity through this route.
let path = RootRelativeLogicalPath::parse("lib/Widget.pm")?;
let src = LogicalSourceId::from_root_and_logical_path(&root, &path);

let digest = ContentDigest::of_bytes(b"package Widget;\n1;\n");
let revision = ContentRevision::new(src.clone(), digest);

let envelope = SourceIdentityEnvelope::for_workspace_file(
    project,
    root,
    src,
    Some(revision),
    SourceGeneration::known("1"),
);

assert!(envelope.is_schema_supported());
assert!(envelope.has_known_generation());
```

## License

Licensed under either of Apache License, Version 2.0 or MIT License at your
option.
