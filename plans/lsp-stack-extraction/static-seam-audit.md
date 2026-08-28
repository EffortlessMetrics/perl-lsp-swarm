# lsp-stack Static Seam Audit

Status: candidate
Owner: perl-lsp maintainers
Issue: [#13059](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13059)
Linked ADR: [PLSP-ADR-0004](../../docs/adr/PLSP-ADR-0004-lsp-stack-extraction.md)
Linked spec: [PLSP-SPEC-0028](../../docs/specs/PLSP-SPEC-0028-lsp-stack-extraction.md)
Linked plan: [implementation-plan.md](implementation-plan.md)
Audited revision: `a9664af790888333efbe50a042fa060f3cc2d171`

## Decision

The first extraction unit should be `JsonRpcId`, not the whole `protocol`
module, the whole `jsonrpc.rs` file, or `perl-lsp-rs-core`.

`JsonRpcId` is the smallest useful language-neutral primitive with downstream
leverage:

- its contract is only JSON-RPC integer-or-string identity;
- its source needs only `std`, `serde`, and `serde_json`;
- focused tests already prove strict wire behavior;
- current request, response, dispatch, cancellation, and serving paths consume
  it through the public protocol re-export.

The current `jsonrpc.rs` file is mixed. Its request, response, and error models
are language-neutral, but `JsonRpcError` directly implements
`perl_parser_core::ErrorClass`. Moving the file would import the Perl parser
error taxonomy into the future crate.

The next PR should split `JsonRpcId` into an in-place file-bounded module while
preserving `perl_lsp_rs_core::protocol::JsonRpcId`. After that unit compiles and
tests without a Perl dependency, a scaffold PR may create `crates/lsp-stack`.
The first mechanical move can then relocate the exact unit and keep the current
path as a compatibility re-export.

No production code is moved by this audit.

## Classification Rule

- **Language-neutral**: owned behavior and dependencies require no Perl source,
  facts, providers, process policy, product identity, or release state.
- **Mixed**: a reusable primitive shares a file or API with product-owned
  behavior and must be split before extraction.
- **Perl/product-specific**: the surface owns current application semantics,
  feature policy, process configuration, or product identity.
- **Not extractable in this lane**: the surface crosses an explicit non-goal
  such as providers, DAP, editor integration, packaging, or release work.

Classification follows owned behavior, not directory placement.

## Protocol Audit

### `protocol::jsonrpc` — mixed; first primitive lives here

Reusable:

- `JsonRpcId::{Integer, String}`;
- strict untagged serialization and deserialization;
- `from_value`, `try_from_value`, `to_value`, and display;
- later, the request, response, and error wire models.

Blocker:

- `JsonRpcError` directly implements `perl_parser_core::ErrorClass`.

Current proof:

- embedded ID, request, response, and error tests;
- app dispatch, serving, cancellation, and raw-RPC tests.

Disposition:

- split and audit `JsonRpcId` first;
- defer the remaining wire model until error classification is product-owned.

Moving `JsonRpcError` first and relocating its `ErrorClass` implementation back
to `perl-lsp-rs-core` would not work. Rust's orphan rules would then prohibit
implementing an external trait for an external type. Product classification
must remain behind a local function or local wrapper.

`perl-lsp-rs::runtime::dispatch::response::classify_jsonrpc_error` already
demonstrates that ownership shape.

### `protocol::document_version` — language-neutral

The typed decoder owns only LSP integer-domain validation. It distinguishes
absence, explicit null, wrong JSON types, and out-of-range integers without
choosing stale-version or lifecycle policy. Its source needs `serde_json` and
`std`; project issue references exist only in its docs.

Current proof is embedded coverage of absence, null, type, and range
boundaries. This is the second protocol candidate after the compatibility
re-export pattern is proven.

### `protocol::errors` — mixed

Reusable:

- standard JSON-RPC and LSP error codes;
- `ErrorCode`;
- generic error constructors;
- generic required URI, position, and range extraction.

Product-owned:

- `enhanced_error` emits `server_info.name = "perl-lsp"`;
- cancellation builders add provider metadata;
- document-not-found, transport, and connection shapes are current policy.

Embedded tests cover constants, builders, response shape, metadata, and
parameter extraction. Split standard vocabulary from product adapters before
moving anything.

### `protocol::methods` — mixed

Standard LSP method constants are neutral. The same module owns
`$/test/slowOperation`, `experimental/testDiscovery`, and crate-specific docs.

Embedded tests cover lifecycle, document, workspace, hierarchy, window,
notification, refresh, special-method, and uniqueness expectations. Move only
a reviewed standard subset in a later PR.

### `protocol::resolve_envelope` — mixed; defer

Authenticated bounded-envelope mechanics may be reusable. The current contract
also owns:

- the `perl-lsp.resolve.v1:` prefix;
- closed provider method and family enums;
- currentness and effective-profile identities;
- replay and session-auth policy.

Use embedded envelope/session-auth tests plus owning provider resolve tests if
this surface is reconsidered. Do not generalize it by renaming; first separate
mechanism from the current provider registry and prove an external need.

### `protocol::schema` — mixed; defer

Bounded JSON-RPC envelope validation and traversal may be reusable. The current
module combines:

- the project-pinned upstream source manifest;
- the current method and payload registry;
- `perllsp` direction labels;
- `PerlLspExtension`;
- project-extension fallback policy.

Current proof is the schema test suite and exact-process schema receipts.
Separate generic envelope validation from project registry and source authority
before moving it.

### `protocol::capabilities` — Perl/product-specific

This module owns the current feature catalog, Perl trigger characters and
commands, build flags, capability gaps, and inline-completion policy. Current
capability snapshots and registration tests stay with the product.

Generic capability helpers, if needed, should be designed independently rather
than extracted with Perl feature policy.

### `protocol::binary_identity` — Perl/product-specific

This module owns canonical server, DAP, VSIX, repository, publisher, package,
artifact, and compatibility identity. It remains with the product.

### Protocol governance — Perl/product-specific

`error_disposition`, `error_inventory`, and `final_surface_inventory` own
parser-derived categories, workspace-wide inventory, and
capability/registration/mutation governance.

The inventory also says `JsonRpcError` is unclassified while `jsonrpc.rs`
contains a direct implementation. That stale relation reinforces that the
manual inventory must not define the reusable wire boundary.

## Transport Audit

### `transport::framing` — mixed; later candidate

Reusable:

- `ContentLengthFramer`;
- bounded header and frame parsing;
- `FramingError`;
- `frame`;
- split-frame and multi-frame state handling.

Product-owned or blocking:

- `FramingError: perl_parser_core::ErrorClass`;
- decode and encode through current JSON-RPC types;
- conversion of client responses to `$/perl-lsp/clientResponse`;
- current logging, lossy UTF-8, and malformed-frame recovery policy.

Current proof includes embedded framing tests,
`wave_final_absorption_tests.rs`, `tests/support/message_framing.rs`, and raw
RPC/lean-editor receipts. Split byte framing from decode and routing before
moving it.

## Runtime Audit

### `runtime::cancellation` — mixed; later candidate

Atomic token and registry mechanics are reusable. Current names, provider
cleanup context, raw JSON parameters, metrics/cache policy, and the direct
`JsonRpcId` dependency are product-owned.

Use embedded model/property/concurrency tests and app cancellation-dispatch
tests. Revisit after JSON-RPC identity and model ownership are stable.

### `runtime::limits` — mixed

`MemoryBudget`, `MemoryPressure`, and `MemoryMonitor` are neutral.
`LspLimits` owns provider result caps, AST/index/parser bounds, and
operation-specific deadlines.

Use embedded memory tests and affected provider-limit tests. Consider only the
memory primitives later.

### `runtime::input_validation` — mixed

Structural method and JSON bounds may be neutral. Perl file extensions,
workspace paths, supported editor URI schemes, parser line limits, and current
sync-sink policy are product-owned.

Use embedded boundary tests. Keep policy at its sinks; do not export current
allowlists as a generic security API.

### `runtime::launcher` and `runtime::tuning` — Perl/product-specific

These modules own the `perl-lsp` CLI, feature profiles, catalog reports,
`PERL_LSP_*` environment variables, diagnostics, indexing, watchers, logging,
transport defaults, and startup. They remain with the product.

### `runtime::text_utils` — Perl/provider-specific

This module performs Perl statement, subroutine, pragma, and import insertion
analysis for code-action edits. Its placement under `runtime` does not make it
an LSP runtime primitive.

### Server-originated request path — mixed; later candidate

Request-ID allocation and pending-response correlation may be reusable.
Current code is embedded in `LspServer`, initialization state, client
capabilities, workspace-edit metadata, outbound delivery, and feature refresh
methods.

Use embedded pre-initialization, frame-shape, wrap, and registry-currentness
tests. Revisit after protocol and transport boundaries.

### `perl-lsp-rs::runtime::scheduler` — mixed; defer

Method classes, bounded queues, priority ordering, and cooperative shutdown may
be reusable. Current feature priorities, document generations and instances,
deduplication, provider dispatch, response logging, and stale-read policy are
product-owned.

Use embedded classification, ordering, deduplication, freshness, concurrency,
and shutdown tests plus raw-RPC and lean-editor receipts. Do not extract the
current scheduler wholesale.

## Explicit Non-Candidates

This campaign does not extract:

- providers or provider-facing text-edit logic;
- parser, lexer, AST, semantic, symbol, module, pragma, diagnostics, workspace,
  or source-identity behavior;
- DAP, debugger, or peer-bridge behavior;
- inline-completion provider or AI-provider behavior;
- editor extension, installer, package, signing, marketplace, release, or
  publishing surfaces;
- current feature-catalog policy or support/receipt claims.

Those surfaces may consume the future stack. They do not become part of it.

## Initial External Consumer Contract

The first public unit should eventually be
`lsp_stack::jsonrpc::JsonRpcId` with this contract:

- IDs are signed 64-bit integers or strings;
- serde uses the untagged JSON representation;
- deserialization rejects null, booleans, arrays, objects, fractional numbers,
  and integers outside the supported signed domain;
- `from_value` returns `None` for unsupported values;
- `to_value` preserves every accepted ID;
- display returns the contained integer or string spelling;
- the enum remains `#[non_exhaustive]`;
- no API mentions Perl, providers, parser categories, feature catalogs,
  process environment, tracing policy, DAP, packaging, or release state.

During migration,
`perl_lsp_rs_core::protocol::JsonRpcId` remains a compatibility re-export. The
wire behavior above is the compatibility contract; internal file layout is not.

## Next PR: `JsonRpcId` Dependency Boundary

The next PR prepares the unit but does not create `crates/lsp-stack`.

Allowed:

- split `JsonRpcId` and focused tests into an in-place protocol submodule;
- preserve the current public re-export and serialized form;
- add a focused dependency assertion or compile probe;
- update audit and plan status.

Forbidden:

- no `crates/lsp-stack`;
- no cross-crate production move;
- no request, response, error, transport, cancellation, provider, capability,
  DAP, editor, release, or package behavior change;
- no weakening of strict ID decoding;
- no candidate dependency on any `perl-*` crate, `lsp-types`, feature catalog,
  tracing, runtime, or product identity.

Allowed candidate dependencies:

- `std`;
- `serde`;
- `serde_json`.

Acceptance:

- the candidate unit has no `crate::` or workspace-crate import;
- all callers compile through the unchanged public path;
- integer and string IDs retain round-trip behavior;
- null, fractional, and out-of-domain numeric IDs remain rejected;
- no runtime or JSON shape changes;
- the move and compatibility-re-export shape is documented.

Proof:

```bash
git diff --check
./scripts/cargo-safe test -p perl-lsp-rs-core protocol::jsonrpc::tests --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs-core --all-targets --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs --all-targets --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked
```

Raw-RPC receipts are not required for the in-place split unless wire
construction or framing changes.

## Later Order

1. Prove the in-place `JsonRpcId` boundary.
2. Scaffold `crates/lsp-stack` with no moved behavior.
3. Move `JsonRpcId` and retain the compatibility re-export.
4. Make product error classification local, then move the remaining neutral
   JSON-RPC wire types.
5. Split and move low-level Content-Length framing.
6. Evaluate standard method constants and policy-neutral document-version
   decoding.
7. Evaluate cancellation and server-request registry primitives.
8. Leave capability policy, providers, launcher/tuning, DAP, editor, and
   release surfaces in the product.

No later candidate is authorized merely because this audit finds a reusable
nucleus.

## Change Declaration

This audit changes only documentation: it adds this audit and links it from the
implementation plan.

It does not change production code location, runtime behavior, JSON-RPC wire
shape, capability shape, dynamic registration, editor integration,
dependencies, release, publishing, signing, marketplace, installer, or package
surfaces.

## Rollback

Revert this audit and the implementation-plan link. Keep current modules,
imports, tests, and behavior unchanged. A replacement candidate must repeat the
classification and dependency proof.
