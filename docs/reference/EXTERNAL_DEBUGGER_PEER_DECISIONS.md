# External Debugger Peer Seam — Design Decisions & Scope

**Branch:** `claude/perl-debugger-peer-protocol-hlfa6q`
**Goal:** Make `perl-dap` a good *host* for external Perl debugger frontends/backends, with `Devel::ptkdb` as the first real partner — by exposing a small, stable, well-tested seam so a future ptkdb-side patch can be thin and not forced to swallow all of DAP.

This document records the decisions taken while building the seam, so reviewers
and future maintainers can see *why* the shape is what it is, and what was
deliberately deferred.

## Architecture landed

```
IDE / editor
   │  DAP  (crate::protocol, unchanged production path)
   ▼
perl-dap DAP frontend (DebugAdapter — unchanged)
   │
   │  canonical model  (crate::model)         ← backend-neutral vocabulary
   ▼
crate::backend::DebugBackend  (trait — the authored seam)
   ├── NativePerlDbBackend        (wraps existing DebugAdapter; catalog caps + AST breakpoints)
   ├── (LegacyBridgeBackend)      (existing BridgeAdapter path — not re-homed here)
   └── ExternalDebuggerPeerBackend  (crate::backend::external_peer)
             │
             │  Perl Debugger Peer Protocol v1  (crate::peer_protocol)
             │  Content-Length framed JSON, reusing perl_lsp_rs_core framing
             ▼
      Devel::ptkdb future PR (thin: hello + events + optional control)
```

## Decisions

### D1 — The backend contract is **model-typed**, not DAP-typed.
The whole point is that ptkdb should not implement DAP. So `DebugBackend`
speaks the canonical [`crate::model`] vocabulary (`DebugSource`,
`DebugBreakpoint`, `ResolvedBreakpoint`, `DebugStackFrame`, `DebugVariable`,
`DebugEvent`, `StopReason`, …). DAP↔model translation happens only at the DAP
frontend; peer↔model translation happens only in the external peer backend.
Reasoning from the diff alone: the existing dispatch uses
`fn(seq, request_seq, args: Option<Value>) -> DapMessage`; adopting *that* as
the backend contract would leak DAP into every backend and defeat the goal.

### D2 — Everything lands **inside `crates/perl-dap/src/`**, no new workspace crate.
The design offered `crates/perl-debug-model/` *or* inline modules. A new
workspace member touches root `Cargo.toml`, CI membership, semver tracking, and
the 39-member count. The seam is cohesive with `perl-dap` and has no other
consumer yet, so the lower-churn inline option was taken. Promotion to its own
crate is a mechanical follow-up if a second consumer appears.

### D3 — The peer transport uses **blocking `std::net` sockets + a reader thread**, not tokio.
`perl_lsp_rs_core::transport::framing` (`ContentLengthFramer`, `frame`) is a
*synchronous* byte-level API. The `DebugBackend` trait methods are synchronous.
Using blocking sockets with a dedicated reader thread and `std::sync::mpsc`
correlation keeps the peer backend fully decoupled from any ambient tokio
runtime, avoids blocking a tokio worker, and adds **zero new dependencies**. The
reader thread parses frames, routes responses to per-request oneshots and events
to a queue drained by `drain_events()`.

### D4 — Reuse the existing framing, not a new one.
Peer messages are framed with the *same* `Content-Length` family DAP uses, via
`perl_lsp_rs_core::transport::{frame, ContentLengthFramer}`. This is why the
future ptkdb side can reuse ordinary DAP-style framing libraries.

### D5 — `BreakpointOracle` is a **blanket impl over the existing `BreakpointValidator`**.
The AST validator (`AstBreakpointValidator`) is already the breakpoint truth
layer. `BreakpointOracle` is defined as a small superset trait and blanket-impl'd
so both the native path and the peer/session-packet path share one truth layer
rather than re-deriving breakable-line logic.

### D6 — Capabilities are **negotiated and intersected**, honestly.
DAP capabilities advertised to the editor are the intersection of what the
feature catalog supports and what the selected backend supports
(`intersect_dap_capabilities`). For a ptkdb peer in `mirror` mode we do **not**
claim control commands the peer did not offer. The dead `protocol::Capabilities`
struct is untouched (the live payload is the `json!` in `process.rs`); the
translation layer is additive and tested in isolation.

## Deliberately deferred (inventory, not product)

These are called out so the "done" claim is scoped honestly (closure discipline).

- **DF1 — Live dispatch migration.** Rehoming the production `dispatch_request`
  funnel onto `Box<dyn DebugBackend>` is a large, high-regression-risk refactor
  (spec "PR 2"). The seam is instead *proven by tests* (mock backend + fake
  ptkdb peer conformance harness). The existing native DAP path is untouched, so
  no current behavior regresses. Migration is its own follow-up.
- **DF2 — End-to-end editor↔ptkdb live session.** Wiring `ExternalDebuggerPeerBackend`
  into `DapServer::run` behind a launch flag and shipping the VS Code launch mode
  as a working round-trip is deferred; this PR delivers the backend + protocol +
  conformance harness + CLI/session-packet/bootstrap surfaces and the VS Code
  *schema*, so the remaining work is wiring, not design.
- **DF3 — `NativePerlDbBackend` full delegation.** The native backend implements
  the model-typed contract for the surface that does not require a live `perl -d`
  process (capabilities from the catalog, AST-backed `set_breakpoints`), and
  documents the delegation path for process-dependent methods. A full live
  delegate is gated on the DF1 migration.
- **DF4 — `cooperative`/`dapControlled` control modes.** Only `mirror` is fully
  exercised end-to-end (the friendliest first integration). The other modes are
  modeled in types and negotiated, but their bidirectional control paths are
  future work.

## Closure receipt

- repo: `effortlessmetrics/perl-lsp-swarm`
- production_entrypoint: additive modules under `crates/perl-dap/src/`; the live
  DAP production path (`DebugAdapter`/`DapServer`) is **unchanged**.
- independent_expected_behavior: peer protocol + backend proven against an
  in-repo fake ptkdb peer conformance harness (handshake, capability
  negotiation, breakpoints, stopped/output events, stack/scopes/variables/
  evaluate, timeout, mirror-mode capability honesty, peer crash).
- user_visible_effect: **none yet** for end users (DF2) — this is the host-side
  seam. The user-visible partner integration is the future ptkdb PR.
- fallback_remaining: `.ptkdbrc` bootstrap renderer + session-packet emitter are
  the working, shippable surfaces today.
- uncertainty: real `Devel::ptkdb` conformance is validated against the protocol
  spec + a faithful fake peer, not against a live ptkdb build.
