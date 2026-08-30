# CLAUDE.md

This file provides crate-local guidance for work in `crates/perl-dap`.

## Crate Overview

- **Tier**: application/runtime crate
- **Purpose**: native Debug Adapter Protocol server for Perl
- **Shipped product path**: `perl-dap` drives the local Perl interpreter through the native Rust adapter
- **Optional integration**: external debugger peers such as `Devel::ptkdb` may cooperate through the Perl Debugger Peer Protocol; they are not bundled or required
- **External conformance**: `Perl::LanguageServer` may be invoked only from repository test infrastructure; it is not a package feature, library backend, or runtime mode
- **Version**: workspace version from the root `Cargo.toml`

## Commands

```bash
cargo build -p perl-dap
cargo build -p perl-dap --release
cargo test -p perl-dap
cargo test -p perl-dap --features test-helpers --all-targets --locked
cargo clippy -p perl-dap --locked -- -D warnings -A missing_docs
cargo doc -p perl-dap --no-deps

# Prove the native package without default convenience features.
cargo test -p perl-dap --no-default-features --features dap-phase2,dap-phase3

./target/release/perl-dap --stdio
RUST_LOG=debug ./target/release/perl-dap --stdio
```

The shipped CLI must reject `--bridge`. Do not restore that flag, a PLS Cargo
feature, a `BridgeAdapter` public API, or PLS subprocess lifecycle code.

## Product Boundary

Preserve these invariants:

1. Native `perl-dap` is the default and first-mile debugger path.
2. A local Perl interpreter is the only external runtime requirement for native sessions.
3. Workspace parser, lexer, protocol, and adapter support crates are compiled into the binary.
4. External debugger peers are explicit optional integrations; `perl-dap` remains the DAP server.
5. PLS may appear only in repository-only conformance fixtures, commands, or historical documents.
6. Public guides, crate landing pages, CLI help, editor defaults, docs.rs, Cargo features, and release artifacts contain no PLS runtime path.

Canonical policy: `docs/reference/NATIVE_STACK_POLICY.md`.

## Architecture

### Core runtime

| Module | Key types | Purpose |
|---|---|---|
| `main.rs` | `Args` | shipped CLI, native and external-peer editor stdio (product); editor `--socket`/`--port` fail before bind |
| `server/` | `DapServer`, `DapConfig`, `DapMode` | native server lifecycle |
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

The external-peer path is not an alternate DAP server. It keeps `perl-dap` as
the DAP frontend while an explicitly selected debugger engine owns some or all
runtime control.

### PLS conformance

PLS comparison code belongs outside `crates/perl-dap/src/**`, for example under
an xtask or repository compatibility harness. A comparison run binds exact tool,
Perl, fixture, configuration, and native candidate identities. External behavior
is evidence for review, not automatically normative.

## Capability Advertising

A `supportsX` capability is a promise that the selected backend can honour the
request. Changes normally touch:

1. the feature catalog entry;
2. `backend/capabilities.rs::CatalogDapFlags::from_catalog`;
3. backend capability intersection/gating;
4. a positive handler or explicit refusal test.

Do not advertise a capability merely because a request type exists.

### Capabilities pinned false pending proof

Some capabilities are gated on a proof that does not exist yet. They are pinned
by a single named authority in `backend/capabilities.rs`, and no catalog row,
backend flag, or handler presence may widen them:

| Capability                    | Authority                       | Gate |
|---|---|---|
| `supportsEvaluateForHovers`   | `PURE_HOVER_INSPECTION_PROVEN`  | #9573 |

`supportsEvaluateForHovers` promises a *pure inspection of the selected frame*.
`handle_evaluate` runs a raw perl5db command against the debugger's **current**
frame and `allowSideEffects` can widen the screened subset, so hover stays
`false` and every mode refuses `evaluate` with `context: "hover"` up front —
before screening, frame lookup, reference allocation, or debugger I/O.

Admission is bound to the same authority: every mode refuses hover **exactly
when it does not advertise hover** (`refuse_hover_evaluation`). So flipping the
constant promotes advertisement and admission together — it cannot leave the
capability advertised true while handlers still reject the request. Peer modes
pass their own advertised value, so promoting the native gate does not silently
open an external-peer path that has no pure inspection of its own.

If you are about to change one of these back to a catalog flag: don't. Land the
named issue's re-enable gate first, then flip the one constant.

## Important Runtime Rules

- The output reader in `debug_adapter/process.rs` is the sole consumer of the debugger control stream.
- Reader-thread work must not block waiting on output that the same reader must produce.
- Other threads use framed debugger commands and bounded capture helpers for synchronous queries.
- Platform-specific process control stays behind `cfg(unix)` / `cfg(windows)`.
- Regex initialization must degrade safely rather than panic.
- Breakpoint and source decisions should consume the parser-backed oracle rather than duplicate line heuristics.
- External-peer capabilities must be negotiated and intersected honestly; unsupported control remains a visible refusal.
- Runtime values must be observed for the current session/stop; fabricated placeholders are forbidden.

## Validation

For native-runtime or public-surface work, start narrow and expand only as needed:

```bash
cargo fmt --check -p perl-dap -p perl-lsp-rs
cargo clippy -p perl-dap -p perl-lsp-rs --locked -- -D warnings -A missing_docs
cargo test -p perl-dap --bin perl-dap
cargo test -p perl-dap --test dap_dependency_tests --features dap-phase3
cargo test -p perl-dap --no-default-features --features dap-phase2,dap-phase3
cargo doc -p perl-dap --no-deps
cargo run -p xtask -- check-native-product-surface --strict
cargo package -p perl-dap --allow-dirty --list
```

For PLS comparison work, use the repository-only conformance owner under #7210.
Do not add a product Cargo feature or package source module to make a comparison
convenient.

For external-peer changes, add the relevant peer protocol, fake-peer conformance,
and session targets. A fake peer proves the repository protocol contract; it
does not by itself prove compatibility with a live external debugger build.

## Documentation Rules

- `crates/perl-dap/README.md` and `docs/tutorials/DAP_USER_GUIDE.md` are native-first product surfaces.
- Historical PLS material belongs in archive/conformance documentation only.
- `docs/reference/EXTERNAL_DEBUGGER_PEER_DECISIONS.md` owns the optional peer-seam boundary.
- Do not add PLS installation commands, `--bridge`, or bridge-first migration copy to first-mile docs or crate-level Rustdoc.
