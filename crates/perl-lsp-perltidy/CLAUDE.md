# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Name**: `perl-lsp-perltidy`
- **Version**: workspace (inherits)
- **Tier**: 3 (formatting integration layer)
- **Purpose**: Formatting integration for the Perl LSP server — provides both a native (parse-gated, zero-subprocess) formatter and a `Perl::Tidy` subprocess formatter, unified behind a common `PerlFormatter` trait.

## Commands

```bash
cargo build -p perl-lsp-perltidy           # Build
cargo test -p perl-lsp-perltidy            # Run tests
cargo clippy -p perl-lsp-perltidy          # Lint
cargo doc -p perl-lsp-perltidy --open      # View documentation
```

## Architecture

### Public API

| Type / Trait | Purpose |
|-------------|---------|
| `PerlFormatter` | Common trait implemented by both formatters |
| `FormatConfig` | Unified configuration passed to either formatter |
| `FormatterMode` | Selects `Native` or `PerlTidy` mode |
| `FormatResult` | Output: formatted text + `Vec<TextEdit>` |
| `TextEdit` | Range-based text replacement (LSP-compatible) |
| `NativeFormatter` | Built-in formatter (parse-gated, default path; no subprocess) |
| `BuiltInFormatter` | Low-level native formatting engine |
| `PerlTidyFormatter` | Subprocess-based `Perl::Tidy` integration |
| `PerlTidyConfig` | `Perl::Tidy`-specific knobs; presets: `.pbp()`, `.gnu()`, `.default()` |
| `BracePlacement` | Enum: brace style options |
| `ElsePlacement` | Enum: `else`/`elsif` placement options |
| `TrailingComma` | Enum: trailing comma policy |
| `FinalNewline` | Enum: newline termination policy |

### Formatter modes

**`NativeFormatter`** is the default LSP formatting path. It is:
- Parse-gated: only runs when the source parses cleanly
- WASM-safe: no subprocess, no filesystem I/O
- Preferred for editor-on-save formatting (low latency)

**`PerlTidyFormatter`** delegates to the external `perltidy` binary via subprocess. It is:
- Richer style control (respects `.perltidyrc`)
- Falls back to identity on subprocess failure; never panics

### Dependencies

| Crate | Role |
|-------|------|
| `perl-subprocess-runtime` | Subprocess execution for `Perl::Tidy` |
| `perl-parser-core` | Parse gating for the native formatter |
| `serde` | `FormatConfig` and `PerlTidyConfig` serialization |

## Does NOT own

- LSP formatting request/response dispatch (→ `perl-lsp-rs-core` providers)
- `Perl::Tidy` binary distribution or installation
- Source-range tracking or line index (→ `perl-line-index`)

## Neighbors

| Crate | Relationship |
|-------|-------------|
| `perl-lsp-rs-core` | Primary consumer — drives the LSP `textDocument/formatting` provider |
| `perl-subprocess-runtime` | Subprocess execution abstraction |
| `perl-parser-core` | Parse-gate for `NativeFormatter` |

## Important Notes

- WASM-safe: the native formatter path has no subprocess or filesystem dependency
- `PerlTidyConfig` presets — `.pbp()` (Perl Best Practices), `.gnu()`, `.default()` — let callers pick a well-known style without constructing configs from scratch
- `FormatResult` always includes both the formatted text and LSP-ready `TextEdit`s so providers can choose which representation to use
- Never panic in formatters — subprocess failures and parse failures return a graceful `FormatResult` (identity or error variant)
