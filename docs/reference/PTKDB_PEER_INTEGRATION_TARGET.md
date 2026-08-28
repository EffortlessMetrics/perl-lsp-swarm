# ptkdb Peer Integration Target

This document defines the smallest `Devel::ptkdb` partner behavior needed to
cooperate with `perl-dap` without making ptkdb implement DAP.

**Current status**: the host protocol is implemented. The repository's Perl
reference fixture now also contains an authenticated, headless-tested
mirror plugin substrate pinned to `Devel::ptkdb 1.1091`. Real stock-ptkdb + Tk
partner behavior is still not proven; issue #4786 owns that live receipt.

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

A complete first partner needs only to:

- do nothing unless `PERL_DAP_PEER` is set and `PERL_DAP_PEER_MODE=mirror`;
- connect to the loopback `HOST:PORT` in `PERL_DAP_PEER`;
- send one authenticated `peer/hello` using the per-session token;
- advertise only capabilities actually implemented by that ptkdb build;
- emit `debugger/output` for debuggee output;
- emit `debugger/stopped` for every real stop;
- emit `debugger/terminated` when the debuggee/debugger ends;
- treat clean host EOF/disconnect as normal session completion;
- leave normal ptkdb behavior unchanged when the rendezvous variables are absent.

This level does **not** require ptkdb to accept editor control, install
breakpoints, list variables, or evaluate expressions. The ptkdb UI remains
authoritative and the editor mirrors state.

## Repository implementation substrate

[`minimal_ptkdb_peer.pl`](../../fixtures/debug-peer/perl/minimal_ptkdb_peer.pl)
now has two explicit modes:

- **executed directly**: the existing synthetic reference peer, useful for
  reading and exercising the wire contract without ptkdb;
- **loaded from `.ptkdbrc`**: an experimental mirror plugin for the inspected
  `matthewpersico/Devel-ptkdb@680b83bb0039ac04014a31f00dfe13d8ac589acd`
  / `Devel::ptkdb 1.1091` surface.

The loaded mode requires a 32-hex session token, accepts only an IPv4 loopback
rendezvous in `mirror` mode, sends an empty capability set, and uses deadline-guarded peer I/O (with
nonblocking sockets where the Perl platform exposes that mode). It wraps `Devel::ptkdb::set_file` while preserving the
original method and emits `debugger/stopped` only when that method is reached
from `DB::DB`. On the inspected source, this call occurs after ptkdb's
`no_stop_at_start` gate and immediately before the Tk pause loop, so the event
carries the actual file and line for a real ptkdb stop. An `END` hook emits one
best-effort, bounded `debugger/terminated` event.

A `.ptkdbrc` can load the plugin by absolute path:

```perl
my $perl_dap_peer = '/absolute/path/to/minimal_ptkdb_peer.pl';
do $perl_dap_peer or die $@ || $!;
```

When the rendezvous variables are absent, loaded mode is a silent no-op. A
version mismatch or malformed/authentication contract leaves ptkdb untouched
and reports one narrow diagnostic on stderr.

The current plugin emits debugger-console connection output, real stopped
locations, and termination. It does **not** yet tee debuggee stdout/stderr into
`debugger/output`, answer inspection requests, or accept control requests.
Those missing behaviors keep #7349 and #4786 open.

The Rust integration test exercises the plugin against the real authenticated
host backend with a pinned ptkdb-shaped harness. That proves the protocol,
version gate, wrapper preservation, real stop-location seam, and cleanup logic.
It is not a Tk session and therefore cannot promote stock ptkdb compatibility.

## Rendezvous environment

| Variable | Meaning |
|---|---|
| `PERL_DAP_PEER` | Loopback `HOST:PORT` to connect to. |
| `PERL_DAP_PEER_TOKEN` | Per-session bearer token echoed in `peer/hello`; required by the pinned plugin. |
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

The fixture remains reference/experimental integration code, not bundled stock
ptkdb code.

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

This is the only required first-partner level. The pinned plugin has earned the
handshake, debugger-console output, stopped, and terminated substrate; debuggee
stdout/stderr mirroring and the live Tk receipt remain open.

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
ptkdb's documented APIs. It is useful independently of a live peer, but it is
not a bidirectional integration and cannot prove that ptkdb accepted each
generated call.

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

Fake-peer conformance establishes host correctness only. The new headless
ptkdb-shaped plugin proof establishes the partner seam only. Neither can promote
a live stock-ptkdb compatibility claim without the Tk receipt.

## Non-goals

- No DAP implementation in ptkdb.
- No ptkdb bundle or automatic installation.
- No replacement of the ptkdb GUI.
- No cooperative or DAP-controlled claim in the first patch.
- No requirement that optional partner work block the native debugger release.
