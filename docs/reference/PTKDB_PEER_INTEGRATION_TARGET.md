# ptkdb Peer Integration Target

This document defines the smallest upstreamable `Devel::ptkdb` change needed to
cooperate with `perl-dap` without making ptkdb implement DAP.

**Current status**: host protocol implemented and fake/reference-peer tested;
real stock-pткdb partner behavior is not yet proven. Issue #4786 owns that proof.

## Product boundary

```text
IDE / DAP client
        │ DAP
        ▼
perl-dap                    ← remains the DAP server
        │ Perl Debugger Peer Protocol
        ▼
Devel::ptkdb                ← optional, explicit, unbundled peer
        │
        ▼
debuggee
```

The native `perl-dap` path remains the default. ptkdb is not bundled,
auto-installed, auto-selected, or required.

## Minimum upstream ptkdb patch: `mirror_minimum`

A complete first patch needs only to:

- do nothing unless `PERL_DAP_PEER` is set and `PERL_DAP_PEER_MODE=mirror`;
- connect to the loopback `HOST:PORT` in `PERL_DAP_PEER`;
- send one authenticated `peer/hello` using the session token when supplied;
- advertise only capabilities actually implemented by that ptkdb build;
- emit `debugger/output` for debuggee output;
- emit `debugger/stopped` for every real stop;
- emit `debugger/terminated` when the debuggee/debugger ends;
- treat clean host EOF/disconnect as normal session completion;
- leave normal ptkdb behavior unchanged when the rendezvous variables are absent.

This level does **not** require ptkdb to accept editor control, install
breakpoints, list variables, or evaluate expressions. The ptkdb UI remains
authoritative and the editor mirrors state.

## Rendezvous environment

| Variable | Meaning |
|---|---|
| `PERL_DAP_PEER` | Loopback `HOST:PORT` to connect to. |
| `PERL_DAP_PEER_TOKEN` | Per-session bearer token to echo in `peer/hello` when present. |
| `PERL_DAP_PEER_MODE` | `mirror` for the initial integration. |

The host rejects a mismatched protocol version or token. The peer must not log
the token.

## Protocol

Messages use Content-Length-framed JSON:

```text
Content-Length: <bytes>\r\n
\r\n
<JSON payload>
```

The canonical schemas and examples live in:

- [`EXTERNAL_DEBUGGER_PEER_PROTOCOL.md`](EXTERNAL_DEBUGGER_PEER_PROTOCOL.md)
- [`fixtures/debug-peer/`](../../fixtures/debug-peer/)
- [`minimal_ptkdb_peer.pl`](../../fixtures/debug-peer/perl/minimal_ptkdb_peer.pl)

The fixture is a teaching/reference peer, not production ptkdb code.

## Capability promotion levels

A real session begins with no assumed peer capabilities. The exact authenticated
`peer/hello` determines the session contract.

### Level 1 — `mirror_minimum`

```text
hello
output
stopped
terminated
```

This is the only required first-partner level.

### Level 2 — `mirror_inspection`

Add individually proven support for:

```text
stackTrace
scopes
variables
evaluate
source facts
```

Each operation needs exact ptkdb-version proof and honest failure behavior. No
inspection capability is enabled from documentation alone.

### Level 3 — `cooperative_control`

Add separately negotiated ownership for:

```text
setBreakpoints
setFunctionBreakpoints
continue
next
stepIn
stepOut
pause
```

The implementation must define what happens when the Tk UI and editor issue
conflicting commands. Capability fields cannot substitute for that ownership
contract.

### Level 4 — `dap_controlled`

Editor-authoritative control is future work and not part of the initial ptkdb
integration target.

## `.ptkdbrc` bootstrap is separate

`perl-dap --ptkdb-bootstrap-rc PROGRAM` generates a one-way startup file using
ptkdb's documented APIs. It is useful before a live peer exists, but it is not a
bidirectional integration and cannot prove that ptkdb accepted each generated
call.

The bootstrap and live peer therefore have separate user-facing names:

```text
ptkdb bootstrap                  best-effort setup helper
ptkdb live peer experimental     authenticated bidirectional protocol
```

## Required live receipt

Before the repository calls the live peer supported, #4786 must bind:

- exact `perl-dap` artifact hash/version/SHA;
- exact ptkdb source or distribution build;
- Perl and Tk versions;
- peer protocol version and negotiated capabilities;
- real output/stopped/terminated outcomes;
- any proven inspection/control outcomes;
- token/version mismatch, malformed frame, peer crash, GUI close, debuggee exit,
  timeout, and cleanup results;
- the installed VSIX configuration used for the session.

Fake-peer conformance establishes host correctness only. It cannot promote a
partner compatibility claim.

## Non-goals

- No DAP implementation in ptkdb.
- No ptkdb bundle or automatic installation.
- No replacement of the ptkdb GUI.
- No cooperative or DAP-controlled claim in the first patch.
- No requirement that optional partner work block the native debugger release.
