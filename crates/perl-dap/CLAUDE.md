# CLAUDE.md

This file provides crate-local guidance for work in `crates/perl-dap`.

## Crate Overview

- **Tier**: application/runtime crate
- **Purpose**: native Debug Adapter Protocol server for Perl
- **Shipped product path**: `perl-dap` drives the local Perl interpreter through the native Rust adapter
- **Optional integration**: external debugger peers such as `Devel::ptkdb` may cooperate through the Perl Debugger Peer Protocol; they are not bundled or required
- **Legacy reference**: the `Perl::LanguageServer` bridge is deprecated library-only compatibility/conformance code and is not exposed by the shipped CLI
- **Version**: workspace version from the root `Cargo.toml`

## Commands

```bash
cargo build -p perl-dap
cargo build -p perl-dap --release
cargo test -p perl-dap
cargo test -p perl-dap --features test-helpers --all-targets --locked
cargo clippy -p perl-dap --locked -- -D warnings -A missing_docs
cargo doc -p perl-dap --no-deps

./target/release/perl-dap --stdio
./target/release/perl-dap --socket --port 13603
RUST_LOG=debug ./target/release/perl-dap --stdio
```

The shipped CLI must reject `--bridge`. Do not restore that flag or describe
`Perl::LanguageServer` as a normal runtime dependency.

## Product Boundary

Preserve these invariants:

1. Native `perl-dap` is the default and first-mile debugger path.
2. A local Perl interpreter is the only external runtime requirement for native sessions.
3. Workspace parser, lexer, protocol, and adapter support crates are compiled into the binary.
4. External debugger peers are explicit optional integrations; `perl-dap` remains the DAP server.
5. Legacy PLS bridge code may be used only for compatibility, migration, or conformance comparisons.
6. Public guides, crate landing pages, CLI help, and editor defaults must not teach the legacy bridge as product setup.

Canonical policy: `docs/reference/NATIVE_STACK_POLICY.md`.

## Architecture

### Core runtime

| Module | Key types | Purpose |
|---|---|---|
| `main.rs` | `Args` | shipped CLI, native stdio/TCP and explicit external-peer options |
| `server/` | `DapServer`, `DapConfig`, `DapMode` | native server lifecycle; deprecated bridge branch retained for library compatibility |
| `debug_adapter/` | `DebugAdapter`, `DapMessage` | native request routing, process lifecycle, stepping, frames, variables, evaluate |
| `protocol.rs` | DAP request/response/event types | DAP wire contracts |
| `breakpoints.rs` | `BreakpointStore`, `BreakpointRecord` | breakpoint replacement, hit counting, and logpoints |
| `breakpoint/` | `AstBreakpointValidator`, `BreakpointValidator` | parser-backed breakpoint truth |
| `platform/` | Perl/path/environment helpers | cross-platform process setup |
| `security/` | validation types and functions | path, expression, and timeout boundaries |

### Backend-neutral and external-peer seam

| Module | Purpose |
|---|---|
| `model/` | canonical backend-neutral debugger facts |
| `backend/` | `DebugBackend`, native backend, external-peer backend, DAP/model translation |
| `peer_protocol/` | Perl Debugger Peer Protocol framing and messages |
| `session_plan/` | stable external handoff packet |
| `ptkdb_bootstrap/` | `.ptkdbrc` bootstrap/fallback rendering |

The external-peer path is not the deprecated PLS bridge. It keeps `perl-dap` as
the DAP frontend while an optional debugger engine owns some or all runtime
control.

### Legacy compatibility

`bridge_adapter.rs` and `DapMode::Bridge` exist only to preserve older library
integrations and comparison tests. Keep all setup instructions in
`docs/reference/DAP_LEGACY_BRIDGE_COMPAT.md`. New production code must not select
this path.

## Capability Advertising

A `supportsX` capability is a promise that the selected backend can honour the
request. Changes normally touch:

1. the feature catalog entry;
2. `backend/capabilities.rs::CatalogDapFlags::from_catalog`;
3. backend capability intersection/gating;
4. a positive handler or explicit refusal test.

Do not advertise a capability merely because a request type exists.

## Important Runtime Rules

- The output reader in `debug_adapter/process.rs` is the sole consumer of the debugger control stream.
- Reader-thread work must not block waiting on output that the same reader must produce.
- Other threads use framed debugger commands and bounded capture helpers for synchronous queries.
- Platform-specific process control stays behind `cfg(unix)` / `cfg(windows)`.
- Regex initialization must degrade safely rather than panic.
- Breakpoint and source decisions should consume the parser-backed oracle rather than duplicate line heuristics.
- External-peer capabilities must be negotiated and intersected honestly; unsupported control remains a visible refusal.

## Validation

For native-runtime or public-surface work, start narrow and expand only as needed:

```bash
cargo fmt --check -p perl-dap
cargo clippy -p perl-dap --locked -- -D warnings -A missing_docs
cargo test -p perl-dap --bin perl-dap
cargo test -p perl-dap --test dap_dependency_tests --features dap-phase3
cargo test -p perl-dap
cargo run -p xtask -- check-native-product-surface --strict
```

For external-peer changes, add the relevant peer protocol, fake-peer conformance,
and bridge-session targets. A fake peer proves the repository protocol contract;
it does not by itself prove compatibility with a live external debugger build.

## Documentation Rules

- `crates/perl-dap/README.md` and `docs/tutorials/DAP_USER_GUIDE.md` are native-first product surfaces.
- `docs/reference/DAP_LEGACY_BRIDGE_COMPAT.md` owns historical PLS bridge details.
- `docs/reference/EXTERNAL_DEBUGGER_PEER_DECISIONS.md` owns the optional peer-seam boundary.
- Do not add PLS installation commands, `--bridge`, or bridge-first migration copy to first-mile docs or crate-level Rustdoc.
