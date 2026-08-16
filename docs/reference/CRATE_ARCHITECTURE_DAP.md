# DAP Crate Architecture
<!-- Labels: architecture:dap, crate:perl-dap, integration:native -->

- **Status**: Current product architecture
- **Release line**: v0.18 candidate
- **Updated**: 2026-08-12

This document replaces the 0.9-era greenfield design that proposed a bundled
`Devel::TSPerlDAP` runtime shim and a bridge-first migration path. That design is
superseded. Its historical intent is summarized in
[`docs/archive/DAP_0_9_SHIM_DESIGN.md`](../archive/DAP_0_9_SHIM_DESIGN.md) and
remains available through Git history.

## Product boundary

`perl-lsp` ships its own debugger implementation:

- `perl-dap` owns the Debug Adapter Protocol frontend and session lifecycle;
- the native parser stack supplies source, position, and breakpoint truth;
- the local Perl interpreter and `perl -d` runtime execute the debuggee;
- stack, scope, variable, evaluate, breakpoint, and control behavior are adapted
  into typed Rust state and DAP responses;
- optional external debugger peers are explicit, unbundled integrations where
  `perl-dap` remains the DAP server.

The product does **not** ship or require:

- `Perl::LanguageServer` as a runtime backend;
- `Devel::TSPerlDAP` or another bundled Perl shim;
- `perlcritic` or `perltidy` for debugger operation;
- an alternate DAP server hidden behind a fallback mode.

A local Perl interpreter remains a runtime requirement because the product
executes and debugs Perl programs. Internal Rust crates are compiled into the
released `perl-dap` binary.

## Runtime shape

```text
IDE / DAP client
        │ DAP over stdio or TCP
        ▼
perl-dap
  ├─ DAP framing, dispatch, lifecycle, events
  ├─ parser-backed source and breakpoint truth
  ├─ stack / scope / variable / evaluate projection
  ├─ path, process, timeout, and security policy
  └─ backend-neutral debugger model
        │
        ├─ native Perl debugger path (default)
        │       ▼
        │    local perl -d runtime
        │       ▼
        │     debuggee
        │
        └─ explicit external peer path (optional)
                ▼
          debugger peer such as ptkdb
```

## Main crate seams

| Surface | Ownership |
|---|---|
| `main.rs` | Shipped CLI and transport selection. Native remains the default. |
| `debug_adapter/` | Production DAP request routing, process lifecycle, events, inspection, and execution control. |
| `breakpoint/`, `breakpoint_oracle/` | Parser-backed breakpoint validation and source facts. |
| `stack/`, `variables/`, `eval/` | Typed projection of observed debugger state. Fabricated runtime values are not permitted. |
| `backend/`, `model/` | Backend-neutral debugger contract and canonical model. |
| `peer_protocol/` | Explicit external-debugger peer protocol; not DAP and not a fallback server. |
| `platform/`, `security/`, `shell/` | Cross-platform process, path, command, and admission boundaries. |
| `session_plan/`, `ptkdb_bootstrap/` | Explicit session handoff/bootstrap helpers for optional peers. |

## Native and external-peer paths

The current native production path is authoritative for v0.18. The
backend-neutral dispatch migration is tracked separately so architecture cleanup
does not outrun behavior parity:

- #4783 — move production request dispatch onto `DebugBackend`;
- #4785 — complete native backend delegation and parity;
- #6684 — real native stdio session matrix;
- #6691 — session and suspended-generation lifetime proof;
- #2301 — scopes, variables, evaluate, and mutation semantics.

The optional ptkdb path is separate from the deleted PLS proxy:

- `.ptkdbrc` generation is a best-effort bootstrap helper;
- the live peer protocol is experimental until #4786 proves a real ptkdb build;
- capability negotiation must reflect the authenticated peer's actual behavior;
- no external peer is bundled, auto-selected, or required for native debugging.

## Packaging invariants

The released package and archives contain workspace-owned product runtime only.
Checks must reject payloads or product features for:

```text
Perl::LanguageServer
BridgeAdapter runtime proxy code
Devel::TSPerlDAP
TSPerlDAP.pm
perlcritic
perltidy
bundled ptkdb/Tk
```

Repository-only conformance harnesses may install or invoke external tools in
bounded test lanes. Those harnesses are evidence infrastructure, not product
runtime.

## Security and claim boundary

- Paths and executable selection remain workspace/trust bounded.
- Debugger queries and process waits are time-bounded.
- DAP stdout remains protocol-clean; logs use separate channels.
- Runtime state is reported only when observed for the current session and stop.
- Evaluation uses the documented screening policy; it is not described as an
  interpreter sandbox unless a stronger boundary is independently proven.
- DAP remains preview until installed-artifact evidence earns a stronger claim.

## Historical note

The former 0.9 design was useful as an early decomposition exercise, but its
bridge-first and bundled-shim assumptions no longer describe the product. A
future first-party structured runtime helper requires fresh evidence and a new
ADR under #7295; it must not inherit the old module name, bundle layout, or
installation path by default.
