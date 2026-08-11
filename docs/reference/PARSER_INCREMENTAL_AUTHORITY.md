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

This is the only incremental API already shared by all three relevant boundaries:

1. the `perl-parser` facade exports it directly;
2. `perl-incremental-parsing` forwards it as a compatibility adapter;
3. the LSP adoption issue #1374 names `IncrementalState`, `Edit`, and `apply_edits` as the intended integration seam.

The repository also exports several generations and experiments. Some are useful sources of tests or algorithms, but none is a second production authority.

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
- every publicly exported incremental generation is classified exactly once;
- no non-canonical generation is marked production-eligible;
- the compatibility crate forwards the canonical implementation instead of defining another one;
- retired and experimental surfaces cannot disappear from the authority ledger silently.

The parser-integration shard executes this contract. Adding another `pub mod incremental_*` or facade re-export requires an explicit authority disposition in the same change.

## Claims this does not establish

This ruling does not establish:

- subtree reuse;
- partial parsing;
- sub-millisecond updates;
- fresh-equivalent tokens, spans, diagnostics, or recovery;
- correct parser continuation checkpoints;
- LSP scheduling readiness.

Those claims require their own implementation and proof issues. The purpose of this ruling is to make sure they are proved against one exact parser surface.
