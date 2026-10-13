# perl-workspace-core

The **LSP-free project-facts substrate** for Perl. One deterministic project
model — files, packages, symbols, dynamic boundaries — with stable IDs, source
ranges, provenance, confidence, and honest limitations, consumed by every
product surface: the LSP server, the DAP server, native critic/tidy, the RIPR
exporter, Kwalitee scoring, and (later) a tree-sitter-compatible adapter.

See [PLSP-ADR-0006](../../docs/adr/PLSP-ADR-0006-perl-workspace-core-facts-substrate.md)
and [NATIVE_STACK_POLICY.md](../../docs/reference/NATIVE_STACK_POLICY.md).

## Dependency contract

This crate sits **below** the editor/LSP runtime and **above** the raw parser.
It must never depend — directly or transitively — on:

```
perl-lsp-rs · perl-lsp-rs-core · perllsp · perl-dap
lsp-types · tokio · tower-lsp
perl-workspace   (transitively pulls lsp-types via perl-position-tracking lsp-compat)
```

Allowed dependencies: `perl-parser-core`, `perl-symbol`, `serde` (and
`serde_json` for receipts in tests). The contract is enforced by
`tests/dependency_contract.rs`.

## Model at a glance

```rust
use perl_workspace_core::{build_project_model, FactClasses, ProjectModelRequest};

let model = build_project_model(&ProjectModelRequest {
    root: "lib",
    fact_classes: FactClasses::FILES | FactClasses::SYMBOLS,
})?;

for pkg in &model.packages {
    println!("{} @ {}:{}", pkg.name, pkg.file_id, pkg.declaration_range.start_line);
}
# Ok::<(), perl_workspace_core::WorkspaceCoreError>(())
```

- **Identity** — `FileId`/`PackageId`/`SymbolId` are deterministic FNV-1a
  digests over repo-relative content (no host paths, no timestamps, no
  traversal order, no UUIDs). Re-running on unchanged source yields identical
  IDs.
- **Ranges** — `SourceRange` stores byte offsets + 0-based **UTF-8**
  line/column. UTF-16 LSP positions are computed only at the LSP boundary,
  never stored here.
- **Provenance + confidence** — every fact records its `EvidenceSource` and
  `Confidence`.
- **Dynamic boundaries** — where static analysis stops (`eval`, runtime
  `require`, typeglob assignment, generated methods, XS), the model says so
  explicitly rather than silently.
- **Fact classes** — `FactClasses` gates the work: a request that omits symbols
  never pays to parse.

## Incremental ingestion

Schema v2 adds `ProjectFactShard`, a versioned LSP-free envelope for all facts
owned by one file, and generation-aware `ProjectModel::insert_or_replace` and
`ProjectModel::remove_file` operations. Replacement ownership is keyed by the
normalized relative path because the existing `FileId` intentionally includes
the content digest and therefore changes after an edit.

Serialized schema-v1 `ProjectModel` values remain readable: the new
`shard_states` field defaults to an empty map when absent. A model loaded that
way has no generation history until shards are adopted through the ingestion
API. Producers must not infer freshness from an empty map.

## Status

**All 11 fact classes are implemented** — a request for any class has a real
producer, and no class reports itself unimplemented:

| Class | Produces |
|-------|----------|
| `FILES` | file role, digest, parse status |
| `SYNTAX` | parse success/recovery status |
| `SYMBOLS` | packages, subs, methods, variables, … |
| `IMPORTS` | `use`/`no`/`require` + effects |
| `EXPORTS` | `@EXPORT`/`@EXPORT_OK` symbol lists |
| `COMPILE_EFFECTS` | strict/warnings/features/version (via `perl-pragma`) |
| `DIST` | `META.json`/`cpanfile` facts plus bounded `Makefile.PL`/`Build.PL`/`dist.ini` authoring facts |
| `TESTS` | test framework + assertion counts |
| `POD` | module doc + documented methods + `=head`/`=item` sections |
| `RELATIONS` | inherits / uses / tests edges |
| `DYNAMIC_BOUNDARIES` | string-eval / runtime-require / typeglob / source-filter |

Consumer built on the substrate in this PR: `perl-tree-sitter-compat`
(editor-ecosystem adapter). Documented follow-ups: RIPR migration onto the
substrate (PLSP-ADR-0006 PR 5), critic/tidy/DAP wiring (PR 8), a
substrate-consuming CPAN-`Kwalitee` distribution scorer (the existing
`perl-kwalitee` crate is a *separate* repo-readiness evaluator, not a substrate
consumer — see PLSP-ADR-0006), and richer edges (caller→callee) / external
metadata formats.
