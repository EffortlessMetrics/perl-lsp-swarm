# Issue #2977: static Mojo database-adapter method completion

## Problem

The completion provider has DBI-specific receiver catalogs and the semantic
analyzer synthesizes a `Mojo::Pg` framework symbol, but static calls such as
`Mojo::Pg->db` and `Mojo::mysql->strict_mode` do not have source-backed
adapter-specific method catalogs. The original issue also combines adapter
methods, result-object flow, promise/callback inference, and pub/sub behavior;
this first slice separates the static adapter contract from those deeper
inference problems.

## Evidence

- Completion seam: `crates/perl-lsp-rs-core/src/providers/completion/completion/methods.rs`.
- Static module detection and auto-import already use the `->` prefix seam.
- Framework symbol synthesis: `crates/perl-semantic-analyzer/src/analysis/symbol.rs`.
- Existing async framework tests: `crates/perl-semantic-analyzer/tests/frameworks_async.rs`.
- Upstream API references: [Mojo::Pg](https://metacpan.org/pod/Mojo::Pg) and
  [Mojo::mysql](https://metacpan.org/pod/Mojo::mysql).

The upstream references document these top-level adapter methods for the first
catalog:

| Adapter | Static method names |
| --- | --- |
| `Mojo::Pg` | `new`, `db`, `from_string`, `reset` |
| `Mojo::mysql` | `new`, `db`, `from_string`, `strict_mode`, `close_idle_connections` |

`query`, `hash`, `hashes`, `pubsub`, and migration methods belong to returned
database/result/publisher objects or chained-flow inference and are not part of
the first static catalog unless an existing provider seam proves they are
unambiguously adapter methods.

## Design

1. Add separate, named method catalogs for `Mojo::Pg` and `Mojo::mysql` in the
   existing method-completion module.
2. Gate adapter-specific catalog selection on an explicit imported framework
   symbol or equivalent source-backed module evidence. Unknown and unimported
   adapters must not receive adapter-specific methods.
3. Preserve DBI inference, generic object methods, prefix filtering, ordering,
   auto-import behavior, and existing symbol synthesis.
4. Keep result-object, promise/callback, pub/sub, migration DSL, dynamic module,
   and broad Mojolicious-helper inference out of scope.

## Claim boundary

This spec covers static adapter method completion for imported `Mojo::Pg` and
`Mojo::mysql` class receivers. It does not claim result-object typing, chained
return-type inference, hover/signature support unless the existing provider
seam can reuse the catalog without widening the slice, or runtime reflection.

