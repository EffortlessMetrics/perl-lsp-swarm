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

Wire positions are UTF-16 only. A nonempty `general.positionEncodings` list that
omits `utf-16` fails initialize instead of silently advertising another encoding.

### What happens after each admitted edit

After committing the replacement Rope, the LSP server does a **full reparse** of
the document. There is no AST subtree reuse — `Parser::new(source).parse()` runs
on the complete source text every time. Full transfer and full parse are
independent facts.

See [`crates/perl-lsp-rs/src/runtime/text_sync.rs`](../../crates/perl-lsp-rs/src/runtime/text_sync.rs)
and [`crates/perl-lsp-rs/src/runtime/v0_18_text_sync_envelope.rs`](../../crates/perl-lsp-rs/src/runtime/v0_18_text_sync_envelope.rs).

## The `perl-incremental-parsing` Crate

The `perl-incremental-parsing` crate exists and contains substantial incremental
parsing infrastructure. It is a feature-gated optional dependency of
`perl-parser` and is **not** the v0.18 text-sync envelope. Dormant incremental
fields, Rope mutation helpers, and parser-incrementality epics (#1690/#7409/#7417)
remain open long-term work.

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
