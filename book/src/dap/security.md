# DAP Security

This page describes the current native `perl-dap` security boundary. It replaces
the 0.9 greenfield specification and does not assume a bundled Perl shim or a
PLS runtime backend.

## Trust boundary

`perl-dap` is a local development tool. It starts or attaches to a Perl program
selected by the user and communicates with the local debugger runtime. The
adapter must still distinguish:

- editor/client input;
- workspace configuration and source paths;
- adapter-owned state and protocol traffic;
- debugger-engine output;
- debuggee behavior and output;
- explicit optional external peers.

A debuggee exception, failed assertion, or non-zero exit is not an adapter
security failure.

## Current controls

### Paths and process launch

- Resolve and validate program, workspace, include, and source paths through the
  canonical platform/security helpers.
- Keep editor and peer listeners loopback-bound unless a separately reviewed
  transport contract says otherwise.
- Pass arguments as structured process arguments, not shell command strings.
- Keep DAP stdout protocol-clean; diagnostic logs use a separate channel.

### Resource bounds

- Bound socket connection, peer handshake, debugger query, and shutdown waits.
- Bound frame size, variable expansion depth/count, and retained output.
- Reap child processes and close reader/writer/socket state on all terminal
  paths.

### Runtime state

- Return only state observed for the current session and suspended generation.
- Invalidate stack, scope, variable, evaluate, and source references on resume or
  session replacement.
- Do not fabricate plausible runtime values when inspection is unavailable.

### Evaluate

The current default uses conservative expression screening and debugger-context
admission policy. It is **not** an interpreter sandbox. Side-effectful evaluation
is an explicit debugger action and must not be described as safe merely because
the request passed a regular-expression or syntax screen.

The stronger confinement decision is tracked in #1746. A future runtime helper
or isolated evaluator requires its own threat model and evidence.

### Optional external peers

- Native remains the default.
- Peer use requires explicit configuration.
- Handshakes validate protocol version and any per-session token.
- Capabilities are negotiated per session and must not be inferred from tool
  names or PATH presence.
- ptkdb live-peer claims remain experimental until #4786 proves a real partner
  build.

## Prohibited product paths

Release and package checks must reject:

```text
Perl::LanguageServer runtime bridge
Devel::TSPerlDAP / TSPerlDAP.pm
bundled perltidy or perlcritic
bundled ptkdb/Tk
```

Repository-only conformance tools may exist outside shipped package surfaces.

## Evidence owners

- #4979 — operational error origins and classification;
- #6684 — real-session behavior and bounded failures;
- #6691 — state lifetimes and cleanup;
- #2301 — inspection/evaluate/mutation semantics;
- #6694 — exact installed VSIX and packaged adapter;
- #7275 — removal of fabricated inspection values.

Security and maturity claims must follow those receipts rather than the presence
of a handler or test scaffold.
