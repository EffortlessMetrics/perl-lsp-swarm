# ADR-0047: Stdio-Only Editor Transport for perl-dap

- **Status**: Accepted
- **Date**: 2026-08-25
- **Decides**: #10564 (freeze a stdio-only editor transport boundary and socket-retirement inventory)
- **Constrains**: #10565 (native `--socket` removal), #10566 (external-peer editor socket wrappers), #10567 (zero ambient editor-listener proof)
- **Parent**: #7486 (DAP security controller)
- **Related**: [ADR-0019](0019-security-first-dap.md), [ADR-0027](0027-dap-bridge-native.md), #6949 (debugger-peer authentication), #9415 (canonical DAP train), #9532 (protocol/wire inventory)

## Context

When this ADR was accepted, `perl-dap` offered two editor-facing TCP listeners:

1. native `--socket` / `--port` → `run_socket` / `TcpListener::bind(127.0.0.1)` in
   `crates/perl-dap/src/debug_adapter/transport.rs` (removed by #10565);
2. optional `--socket` wrappers around `--external-peer` and
   `--external-peer-listen` via `bind_editor_listener` in
   `crates/perl-dap/src/main.rs` (owned by #10566).

Those listeners are unauthenticated ambient DAP endpoints. An authenticated
editor socket would require a new out-of-band credential handoff and
pre-initialize wire convention across every client. Current first-party and
planned integration contracts already use parent-owned stdio:

```text
editor / DAP client
→ launches exact perl-dap child
→ inherited stdin/stdout
→ DAP Content-Length framing
```

The distinct debugger-peer boundary stays TCP where required, and is
authenticated separately by #6949:

```text
perl-dap --stdio  ← editor
      │
      └── authenticated TCP peer protocol → external debugger / ptkdb
```

Repository search found no checked current command using `perl-dap --socket`
as an accepted client route. VS Code launches `DebugAdapterExecutable`.
Sublime uses `StdioTransport` with `perl-dap --stdio`. Zed's DAP binary uses
empty arguments and `connection: None`. nvim-dap and Vimspector fixtures are
stdio. Helix and Kubernetes declare stdio and remain `not_proven` as installed
proof. A generic DAP client that *might* speak TCP is not a product
requirement.

This ADR froze architecture truth before code removal. Native editor
`run_socket` was later removed by #10565. #10566 then removed
`bind_editor_listener` and the remaining external-peer editor-socket wrapper.
`--socket`/`--port` still parse via shared `TransportArgs` and fail before bind
on every `perl-dap` path, including `--external-peer` / `--external-peer-listen`.
Debugger-peer TCP is not in scope. Editor-socket authentication is rejected as a
design for this train.

## Decision Drivers

- The deciding criterion is actual current supported consumption, not
  theoretical TCP compatibility.
- DAP `attach` is a protocol request to a stdio-launched adapter. It is not a
  reason for the editor transport itself to be a TCP listener.
- Debugger-peer credentials must never become editor credentials or vice versa.
- Test-only loopback fixtures cannot satisfy a product client row or a support
  claim.
- A current supported/installed client that **requires** editor TCP and cannot
  launch stdio must stop the train and amend #7486 with that exact
  client/receipt. Do not silently break it and do not invent authentication.

## Decision

**Stdio is the sole production editor-facing transport for `perl-dap`.**

Classify every DAP transport and claimed consumer before any code removal:

| Surface | Disposition |
|---|---|
| native editor stdio | retain / product |
| native editor TCP `--socket`/`--port` | retire (#10565) |
| external-peer editor stdio | retain / product |
| external-peer editor TCP wrapper | retire (#10566) |
| debugger-peer connect/listen TCP | retain / authenticated by #6949 |
| test-only loopback transport fixtures | retain only under explicit test authority |
| DAP attach to debuggee/process/peer | independent protocol/backend behavior |
| legacy PLS DAP-to-DAP TCP proxy (`tcp_attach`) | not_product / historical |

### Frozen invariants

1. stdio is the sole production editor-facing transport;
2. no product CLI path binds an ambient editor DAP listener as a supported run mode;
3. external-peer TCP is a debugger-backend transport, not an editor transport;
4. debugger-peer credentials from #6949 never become editor credentials or vice versa;
5. DAP `attach`, external-peer connect/listen, and editor transport remain separate propositions;
6. no supported editor needs a DAP-to-DAP proxy or socket relay;
7. test-only TCP fixtures cannot enter package help/docs/capability/support claims;
8. removing editor TCP does not change DAP request schemas, native launch, PID-attach honesty, or peer protocol semantics;
9. a future editor-socket requirement must return through a new evidence-backed architecture decision, preferably OS-authenticated local IPC rather than reopening an unauthenticated listener.

### Machine check

The inventory at `.ci/dap/editor-transport-inventory.v1.json` and
`scripts/ci/dap_editor_transport_inventory.py` own recurrence. They consume
#9532's protocol-authority boundary without duplicating protocol-route
authority. The check fails closed on unlabeled production `TcpListener::bind`
sites, a second production bind in an already-inventoried file without a
matching `bind_sites` row, public `--socket`/`--port` flags classified as
supported, first-mile docs that recommend an editor socket run mode, VS Code
`DebugAdapterServer` launch, a product DAP-to-DAP relay, test-only evidence
used as a product client row, product-client required markers satisfied only
by a listed test fixture, a debugger-peer listener mislabeled as editor
transport, a supported-client row whose fixtures disagree, or a current
supported client that requires editor TCP.

## Consequences

- First-mile docs, crate landing rustdoc, and the book pointer must not teach
  `perl-dap --socket` as a product run mode. Historical ADRs may retain the
  command as history.
- Native `#10565` removed production `run_socket` / the native editor
  `TcpListener`. Shared `TransportArgs` `--socket`/`--port` remain parsed and
  inventoried as `retire`; native use fails before bind with a `--stdio`
  migration.
- #10566 removed the remaining external-peer editor-socket wrapper. The same
  flags now fail before bind on `--external-peer` / `--external-peer-listen`
  with a stdio migration that preserves the selected peer backend. They are not
  silently ignored.
- #10567 proves stdio-only editor authority and zero ambient DAP listeners
  across every process mode. The composed proof lives in
  `crates/perl-dap/tests/dap_editor_transport_security.rs` and
  `scripts/ci/dap_editor_transport_security.py`. Missing socket observation is
  `not_proven` / `instrument_failure`, never a zero-listener pass. Do not
  implement #7486 or editor-socket authentication in the PR that lands a later
  child.
- If later evidence proves a current supported client requires editor TCP,
  `ruling_status` cannot stay `accepted` and #7486 must be amended before
  further removal.

## Follow-up obligations

- #10565 — native editor `--socket` / `run_socket` production admission removed.
- #10566 — external-peer editor socket wrappers removed; peer-only TCP retained.
- #10567 — composed stdio-only editor-authority / zero ambient listener proof
  (`dap_editor_transport_security.v1`).
- Do not implement #7486 or editor-socket authentication in the PR that lands
  a later child.
