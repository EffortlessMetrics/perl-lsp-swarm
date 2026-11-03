# Incremental Parsing Guide

## Current State

The LSP server advertises `TextDocumentSyncKind::Full` (value 1) to clients for
the v0.18 envelope (#8129). That refers to **how document text is transferred**,
not whether parsing itself is incremental.

### What "Full" means here

When a client sends `textDocument/didChange`, each admitted member is a complete
document replacement (`range` omitted). The server commits the last replacement
atomically. Ranged incremental edits are protocol violations: they do not mutate
last-good text, and current parse/provider facts become unavailable until an
accepted full replacement, close/reopen, or restart.

Wire positions are UTF-16 only. A nonempty well-formed `general.positionEncodings`
list that omits `utf-16` still accepts Full+UTF-16 via an explicit mandatory
fallback reason rather than silently advertising another encoding. Malformed
`positionEncodings` shapes fail initialize.

### What happens after each admitted edit

After committing the replacement Rope, the LSP server does a **full reparse** of
the document. There is no AST subtree reuse — `Parser::new(source).parse()` runs
on the complete source text every time. Full transfer and full parse are
independent facts.

See [`crates/perl-lsp-rs/src/runtime/text_sync.rs`](../../crates/perl-lsp-rs/src/runtime/text_sync.rs)
and [`crates/perl-lsp-rs/src/runtime/v0_18_text_sync_envelope.rs`](../../crates/perl-lsp-rs/src/runtime/v0_18_text_sync_envelope.rs).

## The `perl-incremental-parsing` Crate

The `perl-incremental-parsing` crate exists and contains substantial incremental parsing infrastructure:

- **`IncrementalState`** — Rope-backed document state with lexer checkpoints and token cache reuse
- **`IncrementalDocument`** — experimental #7292 generation; retained edits fail closed to a full fresh parse and rebuild the current-generation cache (#13378). Not a production incremental engine.
- **`IncrementalParserV2`** — Parser with AST subtree reuse metrics; not the v0.18 text-sync envelope
- **`DocumentParser`** — Enum wrapper (`Full` | `Incremental`) gated on `PERL_LSP_INCREMENTAL` env var

This crate is a feature-gated optional dependency of `perl-parser` (behind the `incremental` feature flag). It is **not** the v0.18 text-sync envelope and is **not wired into the `perl-lsp` server** — `perl-lsp` does not depend on this crate and does not invoke it on `didChange` events. Dormant incremental fields, Rope mutation helpers, and parser-incrementality epics (#1690/#7409/#7417) remain open long-term work.

## Performance Characteristics (Current)

The full-reparse approach is fast enough for typical Perl files in practice:

- The v3 recursive-descent parser is written in Rust and generally handles small-to-medium files in under a millisecond on modern hardware
- The live server does not retain or reuse an AST-only cache: `didOpen`, `didChange`, and the asynchronous parse-worker route run the full parser for every current document parse, including repeated identical text. This preserves the complete parser outcome, including recovery diagnostics; a complete parse-artifact store is future work.
- Files larger than the configured size limit (`PERL_LSP_MAX_FILE_SIZE_KB`, default 512 KB) are skipped entirely with no AST

No 65µs or 99.7% node-reuse benchmarks apply to the current LSP path — those numbers were measured against the `perl-incremental-parsing` crate's internal test suite, which is not connected to the server.

## Rope Integration

The server uses a Rope for text storage and UTF-16 position conversion on
outgoing ranges. Production `didChange` no longer applies ranged
`apply_changes` under the supported envelope.

## Testing Commands

```bash
# Envelope unit tests
cargo test -p perl-lsp-rs --lib --locked -- v0_18_text_sync_envelope

# Text-sync production path
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs --lib --locked -- text_sync -- --test-threads=2
```
