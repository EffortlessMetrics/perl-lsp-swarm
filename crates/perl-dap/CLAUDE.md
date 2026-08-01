# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Tier**: 6 (application/executable crate)
- **Purpose**: Debug Adapter Protocol server for Perl. Provides a native adapter that drives `perl -d` directly, and a `BridgeAdapter` library that proxies DAP messages to Perl::LanguageServer.
- **Version**: workspace (see root `Cargo.toml` `[workspace.package]` version)

## Commands

```bash
cargo build -p perl-dap               # Build
cargo build -p perl-dap --release     # Build optimized
cargo test -p perl-dap                # Run tests
cargo test -p perl-dap --features test-helpers   # Include seed-helper test targets
cargo clippy -p perl-dap              # Lint
cargo doc -p perl-dap --open          # View docs
./target/release/perl-dap --stdio     # Run native adapter (stdio)
./target/release/perl-dap --socket --port 13603  # Run native adapter (TCP)
./target/release/perl-dap --bridge    # Run bridge adapter
RUST_LOG=debug ./target/release/perl-dap  # Run with debug logging
```

## Architecture

### Dependencies

**Internal crates**: `perl-parser` / `perl-parser-core` / `perl-ast` (AST for breakpoint
validation), `perl-lexer` (completion keywords), `perl-lsp-rs-core` (transport framing,
platform helpers, feature catalog), `perl-module` (module path resolution).

The former `perl-dap-breakpoint`, `perl-dap-eval`, `perl-dap-stack`, and
`perl-dap-variables` satellites were absorbed into this crate (Wave H); they are now the
`breakpoint`, `eval`, `stack`, and `variables` modules and are re-exported from `lib.rs`.

**External crates**: `tokio` (async runtime), `serde`/`serde_json` (protocol
serialization), `anyhow`/`thiserror` (errors), `clap` (CLI), `tracing` (logging), `regex`
(debugger output parsing), `ropey` (position mapping), `nix` (Unix signals), `winapi`
(Windows process checks).

### Key Modules

Several of these are directories with a `mod.rs`, not single files — check `src/` before
assuming a path.

| Module | Key types | Purpose |
|--------|-----------|---------|
| `lib.rs` | re-exports | Public surface; see `pub use` block at the bottom |
| `server/` | `DapServer`, `DapConfig`, `DapMode` | Server entry point; dispatches to Native or Bridge mode |
| `main.rs` | `Args` (clap) | CLI binary; parses `--stdio`, `--socket`, `--bridge`, `--port`, `--log-level` |
| `debug_adapter/` | `DebugAdapter`, `DapMessage` | Native adapter, split by concern: `process` (lifecycle + output reader), `execution` (stepping), `breakpoints`, `variables`, `evaluation`, `frames`, `logpoint`, `transport`, `dispatch` |
| `backend/` | `DebugBackend`, `NativePerlDbBackend`, peer bridge/launch | Backend abstraction and the external-peer (ptkdb) path |
| `peer_protocol/` | framing, message, payload types | Wire protocol for external debugger peers |
| `bridge_adapter.rs` | `BridgeAdapter` | Spawns Perl::LanguageServer in DAP mode, proxies messages via stdio |
| `protocol.rs` | `Request`, `Response`, `Event`, `Capabilities`, `SourceBreakpoint`, ... | DAP protocol type definitions (serde-annotated) |
| `breakpoints.rs` | `BreakpointStore`, `BreakpointRecord`, `BreakpointHitOutcome`, `interpolate_logpoint_message` | Breakpoint storage with REPLACE semantics, hit counting, logpoint templating |
| `breakpoint/` | `AstBreakpointValidator`, `BreakpointValidator` | AST-based breakpoint line validation and suggestions |
| `eval/` | `SafeEvaluator` | Expression admission control for `evaluate`/`setExpression` |
| `stack/` | `PerlStackParser` | Stack trace extraction and frame classification |
| `variables/` | `VariableParser`, `PerlVariableRenderer` | Debugger variable parsing and DAP rendering |
| `configuration.rs` | `LaunchConfiguration`, `AttachConfiguration`, `create_launch_json_snippet()` | Launch/attach config structs with validation |
| `platform/` | `resolve_perl_path()`, `normalize_path()`, `setup_environment()` | Cross-platform path resolution and env setup |
| `security/` | `SecurityError`, path/expression validation | Path traversal prevention, expression sanitization, timeout caps |
| `tcp_attach/` | `TcpAttachConfig`, `TcpAttachSession`, `DapEvent` | TCP socket attachment to running Perl debuggers |
| `inline_values/` | `collect_inline_values()` | Inline value extraction for scalar variables |
| `feature_catalog.rs` | `has_feature()`, `advertised_features()` | Generated from `features.toml` at build time by `build.rs` |

### Capability advertising

`initialize` capabilities are **gated on the feature catalog**, not hardcoded. A
`supportsX` flag is a promise that the request can succeed, so:

- adding a capability means adding/advertising its `features.toml` entry, and
- a request whose handler always returns `success: false` (currently `restartFrame` and
  `terminateThreads` — perl5db has no primitive for either) stays `advertised = false`
  with `maturity = "planned"`.

`test_initialize_capabilities_mirror_feature_catalog` (in `debug_adapter/mod.rs`) and
`tests/dap_capability_advertising_tests.rs` enforce both directions.

### Feature Flags

| Feature | Purpose |
|---------|---------|
| `dap-phase1` | Phase 1: bridge to Perl::LanguageServer (AC1-AC4) |
| `dap-phase2` | Phase 2: native adapter features (AC5-AC16) |
| `dap-phase3` | Phase 3: production hardening (AC17-AC19) |
| `test-helpers` | Exposes `*_for_test` seeding helpers to integration tests; excluded from production builds |

## Usage Examples

```rust
// Native mode (default)
use perl_dap::{DapConfig, DapMode, DapServer};
let config = DapConfig { log_level: "info".into(), mode: DapMode::Native, workspace_root: None };
let mut server = DapServer::new(config)?;
server.run()?; // stdio transport

// Bridge mode
use perl_dap::BridgeAdapter;
let mut adapter = BridgeAdapter::new();
adapter.spawn_pls_dap().await?;
adapter.proxy_messages().await?;
adapter.shutdown().await?;

// Configuration generation
use perl_dap::{create_launch_json_snippet, create_attach_json_snippet};
println!("{}", create_launch_json_snippet());
```

## Important Notes

- Use `DebugAdapter` directly to route DAP requests and manage protocol state
- The output reader thread in `debug_adapter/process.rs` is the **sole** consumer of the
  debugger control stream. It must never block on a request/response round trip through
  `recent_output` — it is that buffer's producer. Work that needs debugger values from
  inside the reader (e.g. logpoint interpolation) queues framed commands and folds the
  replies in as they stream past; see `debug_adapter/logpoint.rs`
- Request handlers running on other threads use `send_framed_debugger_commands` +
  `capture_framed_debugger_output` for synchronous queries
- Platform-specific code gated with `cfg(unix)` / `cfg(windows)` for signal handling
- Security module enforces workspace-boundary path checks and expression sanitization
- All regex patterns use `OnceLock<Result<Regex, regex::Error>>` or `Lazy<Option<Regex>>` for graceful degradation
- A handful of tests fail in sandboxed/local environments for environment reasons (fake
  PID attach, path canonicalization, renderer drift) — see issue #1435 before assuming a
  regression
