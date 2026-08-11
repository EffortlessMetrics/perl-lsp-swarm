# DAP Protocol Authority and Compatibility

**Status:** Compatibility guide; support is row-specific and evidence-backed, not globally complete.  
**Owner:** [#6737](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6737)  
**Machine-readable authority:** `.ci/dap/protocol-authority.json`

## Authority

The standard wire authority is the upstream Microsoft Debug Adapter Protocol schema pinned by exact content identity:

| Field | Value |
|---|---|
| Repository | `microsoft/debug-adapter-protocol` |
| Commit | `bf8a5d27e8040044b84b863f90916e08925ee811` |
| Schema path | `debugAdapterProtocol.json` |
| Git blob SHA-1 | `814e3b9597b4e734707b61eb196ecbefa121927e` |
| Independent SHA-256 | Recorded in `.ci/dap/protocol-authority.json` after exact-candidate observation |

The Git blob identity binds the schema bytes to the declared upstream commit. The independent SHA-256 is a second content receipt, not a replacement for the commit and blob relationship. The update command must validate both before accepting a new authority.

This repository does not reproduce the entire upstream schema in prose. The upstream artifact governs standard DAP field names, required and optional fields, integer formats, event and request shapes, and extensibility behavior. Project code, tests, capabilities, and documentation must point to that authority rather than maintaining a second hand-written protocol.

## Base protocol

DAP uses its own base protocol over **Content-Length framed JSON**. It is **not JSON-RPC**.

The standard envelope families are:

| Family | Required upstream fields after inheritance |
|---|---|
| `ProtocolMessage` | `seq`, `type` |
| `Request` | `seq`, `type`, `command` |
| `Response` | `seq`, `type`, `request_seq`, `success`, `command` |
| `Event` | `seq`, `type`, `event` |

`Request`, `Response`, and `Event` inherit `seq` and `type` from `ProtocolMessage`. The authority gate resolves those references rather than validating only inline `required` arrays, so an upstream pin that drops the inheritance cannot pass by accident.

The adapter currently uses the shared `ContentLengthFramer` for incremental reads and serializes `DapMessage` envelopes for responses and events. That implementation remains subject to protocol and transport proof; the existence of a Rust type or dispatch arm is not a compatibility verdict.

## What counts as standard DAP

A message or field counts as standard DAP only when all of these are true:

1. It has an upstream definition in the pinned schema.
2. The Rust wire type uses the upstream spelling, casing, requiredness, and range.
3. Production dispatch returns the standard response and required events.
4. The selected backend can perform the behavior.
5. The capability is advertised only when that behavior is available.
6. Positive and negative public-transport proof exists for the current candidate.

A protocol type without runtime behavior is `partial` or `unsupported`, not implemented. A handler without a capability row is not automatically user-visible. A green unit test does not establish installed-editor behavior.

The machine-readable request and event matrix will classify each production surface as one of:

- `compatible` — standard wire shape and behavior are proven;
- `partial` — some standard shape or behavior exists, but the advertised contract is narrower;
- `unsupported` — recognized or representable but not offered by the selected backend;
- `extension` — project-specific wire surface outside standard DAP;
- `not_proven` — implementation may exist, but current evidence is insufficient.

<a id="4-breakpoint-requests"></a>
## Breakpoint requests

This compatibility anchor is retained for existing Rustdoc links. Standard breakpoint request and response shapes are owned by the pinned upstream schema. Repository behavior, backend support, identity lifetimes, and verification status remain row-specific; this document does not reproduce a second hand-written breakpoint schema.

## Standard versus project extension

The repository has project-specific behavior that must not receive standard-conformance credit. The authority gate compares the exact production request/event inventory with these rows and rejects both unclassified production names and stale manifest entries.

| Wire name | Kind | Classification | Version | Owner |
|---|---|---|---|---|
| `inlineValues` | `request` | `extension` | `unversioned-current` | `#2374` |

### `inlineValues`

`inlineValues` is a **project extension**, not a standard DAP request. Its current owner is #2374. It requires its own collision-resistant identity, version, negotiation rule, source or stopped-frame semantics, cancellation and timeout behavior, and unnegotiated-client response. Until that contract is complete, it remains `unversioned-current` and must not appear in standard DAP capability counts.

The current implementation mixes source-derived variable discovery with optional runtime lookup. #2374 owns the decision between an explicitly static source-hint contract and a stopped-frame runtime-value contract. Either can be useful; silently blending them is not an honest protocol.

### Adapter configuration

| Surface | Classification | Owner |
|---|---|---|
| `launch/attach arguments` | `adapter-configuration` | `#4754` |

Keys such as Perl executable selection, include paths, environment, workspace roots, external-peer mode, and adapter-specific timeouts are adapter configuration carried in DAP launch or attach arguments. They are not additions to the standard DAP schema merely because they cross the request boundary.

### External debugger peer protocol

The ptkdb peer protocol is a backend integration protocol behind the DAP frontend. It does not become standard DAP and does not require ptkdb to implement DAP. Its capabilities are intersected with frontend support before the adapter advertises behavior to a DAP client.

## Capability truth

The runtime `initialize` response is governed by #6688:

```text
frontend wire support
∩ selected backend implementation
∩ selected backend mode
∩ validated runtime/configuration prerequisites
∩ behavior-backed proof
= advertised capability set
```

`features_sot.toml`, roadmap entries, Rust structs, handler names, and documentation are inputs and planning surfaces. None is sufficient on its own to set a capability true.

A capability row must point to:

- its upstream field and request/response definitions;
- the frontend translation owner;
- each backend and mode verdict;
- required configuration and runtime facts;
- one positive behavior test;
- one negative or unsupported test;
- the current receipt identity;
- the user-visible limitation owner.

## Lifecycle authority

Lifecycle and event ordering are governed by the pinned schema and #2321. One important upstream distinction is preserved explicitly: a debug adapter is not expected to emit a `continued` event merely to restate a successful client request that already implies resumed execution. `continued` is required for execution that resumes without such a preceding request.

Initialization, configuration, running and stopped state, thread identity, breakpoint identity, process exit, cancellation, disconnect, termination, and late-event rejection must all pass the same typed lifecycle model. Static golden transcripts are useful fixtures but do not replace live-session proof.

## Reference lifetimes

Frames, scopes, variables, evaluated values, source references, and asynchronous producers are tied to session and suspended-generation lifetimes under #6691. A numerically reused reference cannot make an object from an older generation valid again.

The standard schema defines wire fields; it does not prove that the adapter invalidates retained state correctly. That behavior must be tested through the public transport with real debugger sessions.

## Verification

The authority checker has two modes.

### Observe an exact upstream artifact

```bash
python3 scripts/ci/dap_protocol_authority.py observe \
  --root . \
  --manifest .ci/dap/protocol-authority.json \
  --receipt target/receipts/dap-protocol-authority.json
```

`observe` always requires the exact commit URL and Git blob identity. It prints and records the independently calculated SHA-256 so the first candidate can pin it reviewably.

### Enforce the complete pin

```bash
python3 scripts/ci/dap_protocol_authority.py check \
  --root . \
  --manifest .ci/dap/protocol-authority.json \
  --receipt target/receipts/dap-protocol-authority.json
```

`check` additionally requires the manifest SHA-256 and fails when the downloaded bytes, schema identity, inherited base definitions, production wire inventory, documentation, or extension classification disagree.

The receipt records the canonical manifest SHA-256, complete extension and adapter-configuration rows, upstream content identity, standard request/event inventory, and the exact production command/event inventory. A receipt from before an authority metadata change is therefore distinguishable from one produced after it.

Offline or hermetic verification can pass an already-fetched exact schema:

```bash
python3 scripts/ci/dap_protocol_authority.py check \
  --root . \
  --manifest .ci/dap/protocol-authority.json \
  --schema /path/to/debugAdapterProtocol.json \
  --receipt target/receipts/dap-protocol-authority.json
```

Focused falsifiers:

```bash
python3 scripts/tests/test_dap_protocol_authority.py
```

## Update procedure

An upstream update is intentional work, not an automatic dependency bump:

1. Select an exact upstream commit.
2. Resolve `debugAdapterProtocol.json` at that commit and record its Git blob SHA.
3. Fetch the exact commit URL, calculate SHA-256, and validate the schema.
4. Review the upstream schema and changelog delta.
5. Update the project wire matrix for added, removed, or changed definitions.
6. Reconcile the production `SUPPORTED_COMMANDS` and emitted-event inventory with standard and extension rows.
7. Update Rust types, dispatch, capabilities, fixtures, docs, and explicit unsupported rows as needed.
8. Run schema-derived serde fixtures, adversarial framing tests, the real-session matrix, and installed-adapter proof where affected.
9. Commit the manifest, generated projection or fixtures, documentation, and evidence changes together.

A new upstream revision does not make newly added requests supported. It changes the authority against which support and explicit non-support are classified.

## Proof consumers

This authority is consumed by:

- #6600 — focused response, event, and lifecycle conformance packet;
- #6688 — runtime capability truth;
- #6684 — real-session core matrix;
- #2321 — lifecycle event sequencing;
- #2374 — custom `inlineValues` and data-breakpoint boundary;
- #6694 — packaged stdio adapter and installed VSIX proof.

## Claim boundary

This document identifies the authority and the rules for earning compatibility. It does not claim complete optional DAP support, all backend parity, real ptkdb integration, installed-editor correctness, or GA stability. Those verdicts remain row-specific and receipt-backed.
