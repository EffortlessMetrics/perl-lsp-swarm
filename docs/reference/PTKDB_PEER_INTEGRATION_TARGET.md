# ptkdb Peer Integration Target

This document defines the smallest `Devel::ptkdb` partner behavior needed to
cooperate with `perl-dap` without making ptkdb implement DAP.

**Current status**: the host protocol is implemented. The repository's Perl
reference fixture now also contains an authenticated, headless-tested mirror
adapter for an explicitly marked, ptkdb-shaped `Devel::ptkdb 1.1091` reference
harness. The adapter pins the CPAN distribution by SHA-256
`889bfc25d107f46718963023cc9662d3d779896a48d729d0327beec0502c226e` and
verifies a loaded `ptkdb.pm` against SHA-256
`2da4a792a732c134f8f4fa3b6b482da9e5df8dec8cd7ae424ad3b6e06c0bceab`.
The headless harness carries the same digest as an explicit test contract.
This is not stock-ptkdb + Tk support; issue #4786 owns that live receipt.

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
- **loaded from `.ptkdbrc`**: an experimental mirror adapter for an explicitly
  marked reference harness exposing the `Devel::ptkdb 1.1091`-shaped surface.

The loaded mode requires a 32-hex session token, accepts only an IPv4 loopback
rendezvous in `mirror` mode, sends an empty capability set, and uses deadline-guarded peer I/O (with
  nonblocking sockets where the Perl platform exposes that mode). It wraps the
  explicitly marked reference harness's `Devel::ptkdb::set_file` while preserving
  the original method and emits `debugger/stopped` only when that method is
  reached from `DB::DB`. An `END` hook emits one best-effort, bounded
  `debugger/terminated` event.

A `.ptkdbrc` can load the reference adapter by absolute path. The path is
still user-selected; the adapter is not an installer or package manager:

```perl
my $perl_dap_peer = '/absolute/path/to/minimal_ptkdb_peer.pl';
do $perl_dap_peer or die $@ || $!;
```

The reference harness must expose both `$Devel::ptkdb::VERSION = '1.1091'` and
`$Devel::ptkdb::PERL_DAP_MIRROR_SOURCE =
'CPAN:AEPAGE/Devel-ptkdb-1.1091'` plus
`$Devel::ptkdb::PERL_DAP_MIRROR_SHA256` equal to the pinned module digest. A
The adapter does not activate when `%INC` reports a loaded `Devel/ptkdb.pm`:
Perl does not expose the bytes already executed, so hashing that mutable path
would not bind provenance to the loaded artifact. The adapter therefore accepts
only the explicit no-`%INC` headless harness contract; loaded stock modules fail
closed before any connection or method wrap. When the rendezvous variables
are absent, loaded mode is a silent no-op. A version/source mismatch or
malformed/authentication contract leaves the harness untouched and reports one
narrow diagnostic on stderr.

The current adapter emits debugger-console connection output, harness stop
locations, and termination. It does **not** yet tee debuggee stdout/stderr into
`debugger/output`, answer inspection requests, or accept control requests.
Those missing behaviors keep #7349 and #4786 open.

The Rust integration test exercises the adapter against the real authenticated
host backend with a marked ptkdb-shaped harness. That proves the protocol,
version/source gates for the headless harness, wrapper preservation, harness stop
seam, and cleanup logic. It does not prove loaded-module provenance. This is not a stock ptkdb or Tk session and cannot promote compatibility.

## Rendezvous environment

| Variable | Meaning |
|---|---|
| `PERL_DAP_PEER` | Loopback `HOST:PORT` to connect to. |
| `PERL_DAP_PEER_TOKEN` | Per-session bearer token echoed in `peer/hello`; required by the reference adapter. |
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
ptkdb code. The absolute `.ptkdbrc` path and source marker do not establish a
trusted distribution; do not present this adapter as safely distributable.

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

This is the only required first-partner level. The marked reference adapter has
earned the handshake, debugger-console output, stopped, and terminated substrate; debuggee
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
