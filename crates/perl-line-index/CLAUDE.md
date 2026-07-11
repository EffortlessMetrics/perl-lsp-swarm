# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Name**: `perl-line-index`
- **Version**: workspace (inherits)
- **Tier**: 3 (zero-dependency primitive)
- **Purpose**: Byte-oriented line/column index for Perl source text — maps between raw byte offsets and line/column positions, with explicit UTF-16 support for LSP protocol conversion at the boundary.

## Commands

```bash
cargo build -p perl-line-index           # Build
cargo test -p perl-line-index            # Run tests
cargo clippy -p perl-line-index          # Lint
cargo doc -p perl-line-index --open      # View documentation
```

## Architecture

The entire implementation is a single exported type:

| Type | Purpose |
|------|---------|
| `LineIndex` | Precomputed line-start byte offsets for a source text |

### `LineIndex` API

| Method | Signature | Purpose |
|--------|-----------|---------|
| `new` | `fn(text: &str) -> LineIndex` | Build the index from source text |
| `byte_to_position` | `fn(&self, byte: usize) -> (u32, u32)` | Byte offset → `(line, col)` in UTF-8 units |
| `position_to_byte` | `fn(&self, line: u32, col: u32) -> usize` | `(line, col)` → byte offset |
| `position_to_byte_utf16` | `fn(&self, text: &str, line: u32, col: u32) -> usize` | UTF-16 `(line, col)` → byte offset |
| `position_to_byte_checked` | `fn(&self, line: u32, col: u32) -> Option<usize>` | Bounds-checked variant; returns `None` on out-of-range |

**Zero runtime dependencies.** No `serde`, no allocator tricks — a `Vec<usize>` of line starts.

## Position model contract

| Layer | Unit |
|-------|------|
| Storage / internal | Raw byte offsets (UTF-8) |
| `byte_to_position` / `position_to_byte` | UTF-8 column units |
| `position_to_byte_utf16` | UTF-16 column units (LSP protocol) |

LSP positions use UTF-16 column offsets. Convert at the LSP boundary using `position_to_byte_utf16`; everywhere else in the stack use raw byte offsets. **Never store UTF-16 positions internally.**

## Does NOT own

- UTF-8 validation (callers must pass valid UTF-8)
- Unicode normalization
- LSP `Position` / `Range` types (→ `lsp-types`)

## Neighbors

| Crate | Relationship |
|-------|-------------|
| `perl-lsp-rs-core` | Consumer: converts LSP UTF-16 positions to byte offsets for all providers |
| `perl-workspace-core` | Uses byte offsets throughout; line index created at LSP boundary only |
| `perl-ast-v2` | Uses `Range` (byte-based) aligned with `LineIndex` output |

## Important Notes

- `position_to_byte_checked` is the safe default inside providers — never panic on malformed positions
- The UTF-16 path requires the original source text string (to count UTF-16 code units per character); cache the `LineIndex`, not the text
- Line numbers and column numbers are **zero-indexed** in all methods
