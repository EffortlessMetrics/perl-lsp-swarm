# Perl Debugger Peer Protocol — golden message fixtures

Canonical example messages for `perl-debug-peer-v1` (see
[`docs/reference/EXTERNAL_DEBUGGER_PEER_PROTOCOL.md`](../../docs/reference/EXTERNAL_DEBUGGER_PEER_PROTOCOL.md)).

Each file is one JSON message body (unframed; on the wire it is prefixed with a
`Content-Length` header). They double as documentation and as drift guards: the
`perl-dap` test `tests/peer_protocol_fixtures.rs` deserializes each into the
`perl_dap::peer_protocol` types, so a breaking change to the wire types fails a
test.

| File                            | Message                                   |
| ------------------------------- | ----------------------------------------- |
| `hello_request.json`            | peer → host `peer/hello`                   |
| `hello_response.json`           | host → peer `peer/hello` response          |
| `stopped_event.json`            | peer → host `debugger/stopped`             |
| `set_breakpoints_request.json`  | host → peer `debugger/setBreakpoints`      |
| `set_breakpoints_response.json` | peer → host `debugger/setBreakpoints` resp |
| `perl/minimal_ptkdb_peer.pl`    | dual-use core-Perl fixture: executable synthetic reference peer, or explicitly marked experimental `.ptkdbrc` mirror adapter — see [the integration target](../../docs/reference/PTKDB_PEER_INTEGRATION_TARGET.md#repository-implementation-substrate) |

`minimal_ptkdb_peer.pl` keeps both roles explicit. Executing it directly does
not require ptkdb and emits the synthetic reference events. Loading it with
`do` from `.ptkdbrc` is a silent no-op unless the authenticated mirror
rendezvous is present; for the explicitly marked reference `Devel::ptkdb 1.1091`
shaped seam it preserves `set_file`, reports harness stop locations, and emits
bounded termination. If `%INC` reports a loaded `Devel/ptkdb.pm`, the adapter
fails closed because its already-executed bytes cannot be bound to the marker;
the source marker is an adapter-contract guard, not a cryptographic provenance
check. This headless conformance is partner-seam evidence, not a live Tk
compatibility receipt or stock ptkdb support claim.

Note: `hello_request.json` predates the optional `token` field added on
`peer/hello` in [#3404](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3404)
(`PERL_DAP_PEER_TOKEN` handshake auth) — a host that minted a session token
requires `arguments.token` to match. The direct reference mode sends it when
set; the loaded ptkdb plugin requires a valid 32-hex token.
