# External Debugger Peer — Experimental Quickstart

`perl-dap` can host an explicitly selected external Perl debugger engine while
remaining the Debug Adapter Protocol server seen by the editor.

The native `perl-dap` runtime is the default and does not require an external
peer. This page covers optional interoperability only.

## Current support boundary

| Surface | Current status | User meaning |
|---|---|---|
| Native `perl-dap` | Preview product default | Use the normal Perl launch/attach configurations. |
| Debug-session plan JSON | Available helper | Inspect source facts and planned breakpoints without starting another debugger. |
| ptkdb `.ptkdbrc` bootstrap | Best-effort compatibility helper | Generates escaped startup calls; it does not prove ptkdb accepted every call. |
| Live peer host protocol | **Experimental / developer preview** | Proven against repository fake/reference peers. |
| Marked ptkdb-shaped `Devel::ptkdb 1.1091` reference adapter | **Implementation substrate available** | Authenticated headless proof covers the harness stop seam and cleanup; no source provenance or live Tk receipt. |
| Stock `Devel::ptkdb` live peer | **Not yet proven** | Requires an explicit plugin load plus the real ptkdb/Tk/VSIX receipt owned by #4786. |

No external peer is bundled, installed, detected into use, or selected from PATH.
The editor configuration must choose it explicitly.

## Choose the right surface

### Native debugging

For normal debugging, use the [DAP User Guide](../tutorials/DAP_USER_GUIDE.md).
Do not configure an external peer merely because ptkdb is installed.

### Inspect a session plan

```bash
perl-dap --debug-session-plan path/to/script.pl
```

This emits a stable `perl-lsp-debug-session-v1` document containing the program,
source facts, and any configured breakpoint/watch information available to the
builder. It does not launch ptkdb.

### Generate a ptkdb bootstrap

```bash
perl-dap --ptkdb-bootstrap-rc path/to/script.pl > .ptkdbrc
perl -d:ptkdb path/to/script.pl
```

The generated file:

- escapes interpolated values as single-quoted Perl literals;
- wraps registrations in `eval { ... }` so one unavailable ptkdb call does not
  abort the whole startup file;
- may seed line/subroutine breakpoints and watch expressions present in the
  session packet.

This is one-way setup. Without read-back from ptkdb, the product can claim only
that it generated the calls—not that every breakpoint or watch was installed.

### Load the experimental reference mirror adapter

The repository's
[`minimal_ptkdb_peer.pl`](../../fixtures/debug-peer/perl/minimal_ptkdb_peer.pl)
can also be loaded from `.ptkdbrc` as an experimental adapter for the marked
reference harness:

```perl
my $perl_dap_peer = '/absolute/path/to/minimal_ptkdb_peer.pl';
do $perl_dap_peer or die $@ || $!;
```

This mode is intentionally narrow and is not a pinned or distributable stock
ptkdb plugin:

- it activates only for a ptkdb-shaped headless harness that exposes
  `Devel::ptkdb 1.1091` and the exact pinned contract SHA-256 digests, when a trusted host launcher has
  supplied `PERL_DAP_PEER`, a 32-hex `PERL_DAP_PEER_TOKEN`, and
  `PERL_DAP_PEER_MODE=mirror`;
- it advertises no inspection or control capabilities;
- it preserves the harness's original `set_file` method and reports the supplied
  file and line when `DB::DB` reaches the marked stop path;
- it emits debugger-console connection output and one bounded termination event;
- it is a silent no-op when the rendezvous environment is absent.

The adapter refuses activation when `%INC` reports that a `Devel/ptkdb.pm` file
was loaded. Perl does not provide the bytes already executed, and hashing the
mutable `%INC` path later would not prove the loaded artifact. This keeps the
contract limited to the explicit headless harness until a load-time artifact
binding is available.

It does not yet mirror debuggee stdout/stderr, answer stack/scopes/variables, or
accept stepping and breakpoint commands. The repository test uses a
  ptkdb-shaped headless harness, not stock ptkdb or Tk. This is implementation
  evidence for the next live test, not a compatibility verdict or a ready-made default
editor configuration.

### Exercise the experimental live peer host

For a peer implementation that already speaks the Perl Debugger Peer Protocol:

```bash
perl-dap --external-peer 127.0.0.1:5000
```

The editor speaks DAP over stdio. `perl-dap` connects to the peer and translates
between DAP and the backend-neutral debugger model.

Listen mode is also available for development/protocol work:

```bash
perl-dap --stdio --external-peer-listen 127.0.0.1
```

These commands prove the host implementation, not stock ptkdb compatibility.
Use them only with a peer build whose exact protocol version and capabilities
are known. The listen authority mints the token; a peer launcher must pass the
complete environment contract to the ptkdb process without logging the token.

## Capability levels

A peer session starts with no assumed capabilities. The authenticated
`peer/hello` message determines what that exact peer can do.

### `mirror_minimum`

The first useful partner level is:

```text
peer/hello
debugger/output
debugger/stopped
debugger/terminated
```

The external UI owns execution control; the editor mirrors state. No stepping,
breakpoint mutation, stack, variables, or evaluate capability is implied.

### `mirror_inspection`

Stack, scopes, variables, source, or evaluate may be enabled only when the live
peer advertises each operation and a real partner build proves it.

### `cooperative_control`

Continue, step, pause, and breakpoint synchronization require separate ownership
rules and per-capability proof. They are not part of the initial ptkdb claim.

### `dap_controlled`

Editor-authoritative control is future work and must not be inferred from the
presence of modeled protocol fields.

## Failure behavior

The peer path is bounded by:

- protocol-version validation and token validation whenever the host mints one;
- a required authenticated token for the reference adapter;
- connection, read, write, request, and handshake timeouts;
- explicit capability intersection;
- clean socket/thread/process teardown;
- visible errors for unsupported configuration shapes.

A peer failure does not silently fall back to another external implementation.
Users can return to the native debugger by choosing the normal native launch
configuration.

## References

- [Peer protocol](../reference/EXTERNAL_DEBUGGER_PEER_PROTOCOL.md)
- [Current design decisions](../reference/EXTERNAL_DEBUGGER_PEER_DECISIONS.md)
- [Minimum ptkdb-side target](../reference/PTKDB_PEER_INTEGRATION_TARGET.md)
- [Backend author guide](DEBUGGER_BACKEND_AUTHORS.md)
- #4786 — real ptkdb partner implementation and proof
- #7349 — authenticated `mirror_minimum` partner slice
- #7276 — pre-proof product/UX claim boundary
