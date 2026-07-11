# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Name**: `perl-lsp-rs-core`
- **Version**: workspace (inherits)
- **Tier**: 2 (monolithic LSP implementation core)
- **Purpose**: The monolithic implementation core of the Perl LSP server. Provides all 25+ LSP providers, feature-flag system, transport, runtime lifecycle, config, tooling integration, and performance infrastructure.

## Commands

```bash
cargo build -p perl-lsp-rs-core                        # Build
cargo test -p perl-lsp-rs-core -- --test-threads=2    # Run tests (respect thread limit)
cargo clippy -p perl-lsp-rs-core --tests               # Lint
cargo doc -p perl-lsp-rs-core --open                   # View documentation
```

> **Threading**: use `--test-threads=2` for all test runs — some tests mutate `PATH` and require `PATH_ENV_LOCK` serialization.

## Architecture

### Top-level modules

| Module | Purpose |
|--------|---------|
| `capability_map` | Translates `features_sot.toml` entries to client capability checks |
| `config` | Runtime config loading, `perl_oracle_env`, toolchain profile, native build hints, metadata deps |
| `critic_parser` | Parses `Perl::Critic` output from external lint runs |
| `feature_catalog` | Generated feature catalog (from `build.rs` + `features_sot.toml`) |
| `features` | Feature model: IDs, flags, contracts, profiles, policy, grid (~7 absorbed microcrates) |
| `governance` | Feature-profile rollout controls and policy APIs |
| `hashing` | Workspace hashing helpers |
| `performance` | Sync caches and allocation strategies via `moka` |
| `platform` | Cross-platform interpreter/toolchain detection |
| `protocol` | JSON-RPC + LSP protocol types: capabilities, errors, methods |
| `providers` | 25+ LSP request handlers (see below) |
| `runtime` | Request lifecycle, cancellation, input validation, launcher, limits, text utils |
| `tooling` | External tool integrations: `perltidy` native compat, `Perl::Critic` |
| `transport` | Content-length framing (absorbed from `perl-content-length-framing`) |
| `uri` | URI parsing helpers |

### Key providers (under `providers/`)

completion, diagnostics, formatting, navigation (goto-def/refs/decl/impl), rename, semantic-tokens, code-actions, hover, inlay-hints, document-symbols, workspace-symbols, type-hierarchy, call-hierarchy, document-links, folding, selection-range, on-type-formatting, color-provider, import-management, AI-features, testing (code lens), and more.

### Key dependencies

| Crate | Role |
|-------|------|
| `perl-parser-core`, `perl-ast`, `perl-lexer` | Perl parsing stack |
| `perl-semantic-analyzer` | Symbol extraction, type inference, scope analysis |
| `perl-module` | Module resolution |
| `perl-workspace` | Cross-file workspace index |
| `lsp-types` | LSP protocol types |
| `moka` | Sync LRU caches (Wave G3 performance) |
| `perl-lsp-perltidy` | Formatting integration |
| `perl-subprocess-runtime` | Subprocess execution |
| `tracing` + `tracing-subscriber` + `tracing-appender` | Structured logging |

## Feature flags

- `lsp-ga-lock` — feature flag for GA-locked features
- `lsp-compat` — backward-compatibility shim

## Build-time generation

- `build.rs` reads `features_sot.toml` and generates `feature_catalog.rs`
- `features_sot.toml` is the single source of truth for the feature catalog; never edit `feature_catalog.rs` directly

## Does NOT own

- Binary entry point (→ `perl-lsp-rs`)
- Installable crate name (→ `perllsp`)
- DAP server logic (→ `perl-dap`)

## Neighbors

| Crate | Relationship |
|-------|-------------|
| `perl-lsp-rs` | Consumer: wraps this crate's server in the binary runtime |
| `perl-semantic-analyzer` | Analysis layer driving most providers |
| `perl-workspace` | Workspace index used by navigation providers |

## Important Notes

- `test_support::PATH_ENV_LOCK` is a process-global mutex serializing all tests that mutate `PATH`
- Documentation is auto-included from `README.md` via `#![doc = include_str!("../README.md")]`
- Wave consolidations (G1a, G1b, G2, G3, Final PR B) absorbed 25+ formerly-separate provider crates; the module layout reflects their origins
- Never panic in providers — use `Result`/`Option` and degrade gracefully
