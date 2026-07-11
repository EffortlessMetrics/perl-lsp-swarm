# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Name**: `perllsp`
- **Version**: workspace (inherits)
- **Tier**: 1 (thin installable facade)
- **Purpose**: Installable binary facade — the only crate published under the `perllsp` name on crates.io. Contains no logic; delegates entirely to `perl-lsp-rs`.

## Commands

```bash
cargo build -p perllsp            # Build the binary
cargo install --path crates/perllsp  # Install locally
cargo clippy -p perllsp           # Lint
```

## Architecture

This crate is intentionally minimal — a two-file shell:

| File | Purpose |
|------|---------|
| `src/lib.rs` | Re-exports everything from `perl_lsp` via `pub use perl_lsp::*` (3 lines) |
| `src/main.rs` | Binary entry point: calls `perllsp::run_cli(std::env::args())` (3 lines) |

**Sole dependency**: `perl-lsp-rs` (workspace) — all implementation lives there.

## Does NOT own

- Any LSP logic, protocol handling, or provider implementation (→ `perl-lsp-rs`)
- Configuration or feature flags (→ `perl-lsp-rs-core`)
- Transport or dispatch (→ `perl-lsp-rs`, `perl-lsp-rs-core`)

## Neighbors

| Crate | Relationship |
|-------|-------------|
| `perl-lsp-rs` | The only direct dependency; provides `run_cli()` and all re-exported types |

## Claim boundary

Any bug or feature request for LSP behavior belongs in `perl-lsp-rs` or `perl-lsp-rs-core`, not here. Changes to this crate are only needed for:
- Binary metadata changes (version, binary name, `cargo-binstall` config)
- Publish/release mechanics

## Important Notes

- `#![deny(unsafe_code)]`
- Cargo-binstall metadata present for pre-built `.tgz` distribution
- Do NOT add logic here — this crate exists to give the binary a clean installable name separate from the implementation
