# External Debugger Peer — Decisions and Claim Boundary

This document records the current architecture and evidence boundary for
external debugger peers, with ptkdb as the first intended partner.

## Decision summary

1. `perl-dap` remains the Debug Adapter Protocol server.
2. Peers speak the backend-neutral Perl Debugger Peer Protocol, not DAP.
3. Native `perl-dap` remains the default product path.
4. Peers are explicit, unbundled, and never selected from PATH/module presence.
5. Capabilities are negotiated per authenticated session.
6. Repository fake-peer tests establish host/protocol behavior only.
7. A real ptkdb compatibility claim requires the live partner receipt in #4786.
8. `.ptkdbrc` bootstrap and live peer are separate product surfaces.

## Architecture

```text
IDE / DAP client
        │
        ▼
perl-dap DAP frontend
        │ canonical debugger model
        ├─ native Perl debugger backend
        └─ external peer backend
                │ Content-Length JSON
                ▼
          explicit debugger peer
```

DAP-to-model translation belongs at the frontend. Peer-to-model translation
belongs in the peer backend. A peer does not inherit editor protocol complexity.

## Current implementation truth

### Native production path

The established `DebugAdapter` / `DapServer` path remains the v0.18 production
authority. The backend-neutral migration is incomplete and is tracked by #4783
and #4785. Architecture convergence should not replace the proven native path
without response/event parity.

### External peer host

The Rust host includes:

- connect and listen rendezvous modes;
- Content-Length framing;
- protocol-version validation;
- optional per-session token authentication;
- request/response correlation;
- output and stop event projection;
- bounded read, write, connect, handshake, and request waits;
- capability intersection;
- cleanup on close, timeout, or failure.

These properties are tested against repository fake/reference peers.

### Real ptkdb partner

No live stock ptkdb build has yet earned the partner claim. Until #4786 closes:

```text
ptkdb bootstrap                 best-effort compatibility helper
ptkdb live peer                 experimental / developer preview
stock ptkdb live compatibility  not proven
```

## Capability model

A session starts with `none`, not a tool-name-derived default. The authenticated
peer advertises each capability.

### Mirror minimum

```text
hello
output
stopped
terminated
```

The external UI owns execution control.

### Mirror inspection

Stack, scopes, variables, evaluate, and source facts require separate live proof.

### Cooperative control

Breakpoint and execution-control operations require explicit shared-ownership
semantics and individual capability proof.

### DAP-controlled

Editor-authoritative operation remains future work.

Helpers or examples that describe documented ptkdb behavior are upper-bound
research material, not session defaults.

## Transport decisions

- Use blocking sockets plus a dedicated reader thread for the synchronous backend
  contract.
- Reuse the shared Content-Length framing implementation.
- Give every connection and operation a finite timeout.
- Mark the connection closed on write failure and wake pending waiters.
- Reject protocol-version and token mismatches before the session becomes live.
- Keep sequence and request correlation independent of DAP sequence numbers.

## Source and breakpoint truth

The native parser-backed breakpoint oracle remains authoritative for source
facts and pre-session validation. Runtime installation/hits remain backend facts.
A peer cannot turn a static breakable line into a verified runtime breakpoint
without reporting the corresponding behavior.

## Bootstrap decision

The `.ptkdbrc` renderer is retained because it provides useful one-way setup
without a ptkdb patch. It:

- emits escaped Perl literals;
- wraps registrations so one unsupported call degrades locally;
- can carry session-plan breakpoints and watches.

It cannot claim installation success without ptkdb read-back. Documentation and
status must use “generated” or “requested,” not “installed,” for that surface.

## VS Code and CLI exposure

- Native is the default debugger configuration.
- External peer modes require explicit configuration.
- Unsupported mode combinations fail visibly; they are not coerced into another
  topology.
- Discoverable live-peer templates must say experimental until #4786.
- The Microsoft DAP implementor listing names `perl-dap`, not ptkdb.

## Naming

Reserve terms consistently:

```text
PLS bridge / BridgeAdapter   historical alternate-DAP proxy; remove from product
external peer backend        optional backend-neutral engine integration
DAP frontend/session driver  normal model-to-DAP translation
ptkdb bootstrap              one-way startup helper
ptkdb live peer              experimental partner protocol until proven
```

Do not use “bridge mode” as the generic name for both PLS proxying and normal
backend translation.

## Evidence and promotion

The host seam is useful engineering but does not by itself establish a supported
partner journey. Promotion requires:

- real ptkdb source/build and Perl/Tk identities;
- exact `perl-dap` and VSIX artifacts;
- authenticated handshake;
- negotiated capability set;
- real output/stop/termination results;
- any claimed inspection/control results;
- malformed, mismatched, stalled, crashed, and clean-shutdown cases.

The receipt owner is #4786; installed editor proof composes through #6694.

## Deferred work

- #4783 — one production backend-neutral dispatcher;
- #4785 — complete native backend delegation;
- #4786 — real ptkdb peer and editor proof;
- cooperative and DAP-controlled ownership;
- multi-root/generated-source identity;
- final terminology cleanup after dispatcher convergence.

None of these optional-peer tasks may weaken or delay honest native behavior.
