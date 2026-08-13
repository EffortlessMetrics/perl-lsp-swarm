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
  stable identity.
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

- Path normalization or authority-bound constructor matrix (issue #7655).
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

## Quick start

```rust
use perl_source_identity::{
    ContentDigest, ContentRevision, LogicalSourceId, ProjectId,
    SourceGeneration, SourceIdentityEnvelope, WorkspaceRootId,
};

let project = ProjectId::from_canonical_name("https://github.com/acme/widget");
let root = WorkspaceRootId::from_project_and_root_key(&project, "abc123");
let src = LogicalSourceId::from_root_and_path(&root, "lib/Widget.pm");

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
