# Native parser incremental authority

Issue: [#6701](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6701)
Controller: [#6698](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6698)

## Ruling

The canonical native incremental parser surface is:

```rust
perl_parser::incremental::{Edit, IncrementalState, apply_edits}
```

The top-level compatibility re-exports remain available while the facade and API migration are reviewed:

```rust
perl_parser::{Edit, IncrementalState, apply_edits}
```

Canonical means **one implementation target and one behavioral authority**. It does not mean that the current implementation is production-ready, faster than a fresh parse, or eligible for LSP document synchronization.

Production eligibility remains blocked on:

- #6704 — complete live lexer checkpoints and conservative resynchronization;
- #6710 — fresh-equivalent recovery and diagnostic output;
- #6714 — one exhaustive AST structural-invariant oracle;
- #2327 — full fresh/incremental differential equivalence.

The LSP scheduler may not promote the incremental path under #1374 until the supported envelope satisfies those contracts.

## Why this surface

This is the only incremental API already shared by all three intended boundaries:

1. the `perl-parser` facade exports it directly;
2. `perl-incremental-parsing` forwards it as a compatibility adapter;
3. the LSP adoption issue #1374 names `IncrementalState`, `Edit`, and `apply_edits` as the intended integration seam.

The repository also exports several generations, experiments, and one active lower-tier token-replay kernel. They may supply tests or implementation techniques, but none is a second behavioral authority.

## Current module disposition

The machine-readable source of truth is `crates/perl-parser/incremental_authority.json`.

| Module | Status | Current disposition |
| --- | --- | --- |
| `incremental_advanced_reuse` | internal | Keep only as analysis or privatize after #6707 defines truthful reuse. |
| `incremental_checkpoint` | experimental | #6734 decides real parser continuation versus retirement. |
| `incremental_document` | experimental | Migrate unique proof, then privatize or retire. |
| `incremental_edit` | internal | Fold useful edit semantics into the canonical contract or privatize. |
| `incremental_handler_v2` | retire | Remove after its documented compatibility boundary. |
| `incremental_integration` | experimental | Migrate unique proof, then privatize or retire. |
| `incremental_simple` | experimental | Migrate unique proof, then privatize or retire. |
| `incremental_v2` | experimental | Comparison-only until unique proof is migrated; it cannot establish production readiness. |

A module may remain public temporarily for compatibility. Public visibility does not make it canonical or production-eligible.

## Lower-tier token-replay kernel

`perl_parser_core::incremental` is an active public implementation, not an omitted detail. It exports `IncrementalEdit`, `IncrementalState`, and `IncrementalState::reparse`; `tree_sitter_perl_rs::Parser::parse_with_old_tree` currently consumes that kernel.

The authority ledger classifies it as a **lower-tier kernel**:

- it may provide checkpointed token replay behind a facade;
- it is not the native parser-incremental behavioral authority;
- it is not production-eligible independently of the canonical contract;
- #6707 must either route it behind the canonical surface with shared differential proof or retire it after its useful proof is migrated.

The tree-sitter facade remains an explicitly allowed consumer during that transition. Each allowed consumer records both its public symbol and its production Rust source path. The contract scans `crates/*/src/**/*.rs`, excluding the owner crate, and requires the discovered source set to equal the ledger exactly. Tests, examples, benches, and `archive/` are intentionally outside this production-consumer boundary. Adding another production consumer therefore requires an authority-ledger change in the same PR.

## Compatibility crate

`perl-incremental-parsing` is a compatibility adapter over `perl-parser`. It does not own implementation behavior, correctness expectations, benchmarks, or capability claims.

Its retained obligations are bounded:

- re-export the canonical types and functions;
- compile representative downstream compatibility imports;
- prove that both import paths resolve to the same implementation and result identity;
- carry a documented migration/removal boundary.

Unique behavioral cases currently housed in the compatibility crate should move to the canonical implementation before duplicate suites are removed.

## Enforcement

`incremental_authority_contract` checks that:

- there is one canonical surface;
- every publicly exported `perl-parser` incremental generation is classified exactly once;
- the active `perl-parser-core` token-replay kernel remains explicitly classified;
- every production Rust source that imports the lower-tier kernel is discovered and allowlisted by exact path;
- each allowed source still contains the declared consumer symbol and lower-tier call;
- no non-canonical surface is marked production-eligible;
- the compatibility crate forwards the canonical implementation instead of defining another one;
- retired, experimental, and lower-tier surfaces cannot disappear from the authority ledger silently.

The parser-integration shard executes this contract. Adding another `pub mod incremental_*`, lower-tier implementation, facade re-export, or production consumer requires an explicit authority disposition in the same change.

## Claims this does not establish

This ruling does not establish:

- subtree reuse;
- partial parsing;
- sub-millisecond updates;
- fresh-equivalent tokens, spans, diagnostics, or recovery;
- correct parser continuation checkpoints;
- LSP scheduling readiness.

Those claims require their own implementation and proof issues. The purpose of this ruling is to make sure they are proved against one exact parser surface.
