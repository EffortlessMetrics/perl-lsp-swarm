# Static Seam Audit for `lsp-stack` Extraction

- Status: reviewed candidate audit
- Date: 2026-08-27
- Owner: perl-lsp maintainers
- Controlling issue: [#13054](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13054)
- Audited main commit: `a9664af790888333efbe50a042fa060f3cc2d171`
- Governing ADR: [PLSP-ADR-0004](../../docs/adr/PLSP-ADR-0004-lsp-stack-extraction.md)
- Governing spec: [PLSP-SPEC-0028](../../docs/specs/PLSP-SPEC-0028-lsp-stack-extraction.md)
- Sequence authority: [implementation plan](implementation-plan.md), PR 2

## Conclusion

The repository does not yet contain a reusable LSP stack boundary. It contains
reusable-looking protocol, framing, cancellation, validation, and limit
primitives, but most live beside Perl product policy or depend directly on
`perl-*` crates. Directory names such as `protocol`, `transport`, and `runtime`
therefore do not identify safe move units.

One file is already language-neutral as written:
`protocol/document_version.rs`. It is not the recommended first move because
the accepted sequence starts with the JSON-RPC spine that transport and runtime
will consume.

The recommended first extraction candidate remains the JSON-RPC ID and envelope
types in `protocol/jsonrpc.rs`, after a dependency-preparation PR removes the
`perl_parser_core::ErrorClass` implementation from the wire type and reconciles
the repository's duplicate JSON-RPC error classification paths. The eventual
candidate dependency closure is `std`, `serde`, and `serde_json`.

Low-level `Content-Length` framing should follow only after three boundaries are
made explicit:

1. operational error classification stays in the Perl application;
2. byte framing is separated from JSON-RPC message decoding;
3. the `$/perl-lsp/clientResponse` pseudo-notification remains an application
   compatibility mechanism rather than entering the reusable stack.

No code should move from a whole current directory. Every first movement should
take one reviewed primitive plus its discriminating tests.

## Audit Method

A candidate is classified by what its code knows, not by its path.

| Classification | Meaning |
| --- | --- |
| **Language-neutral** | The implementation and public contract require no Perl facts, provider policy, product identity, release state, or direct `perl-*` dependency. |
| **Mixed** | A reusable kernel and Perl/application policy share one file or module. The current unit must be split before movement. |
| **Perl-specific** | The contract intentionally encodes Perl providers, feature policy, editor behavior, workspace semantics, or product configuration. It stays in the application. |
| **Not extractable** | The surface belongs to DAP, packaging, release, product identity, compatibility, or cross-workspace governance rather than a reusable LSP stack. |

The audit examined the candidate infrastructure named by PLSP-ADR-0004:
JSON-RPC, protocol schemas, capabilities, transport/framing, server-originated
requests, cancellation, limits, validation, tuning, and lifecycle support. It
does not treat parser, provider, diagnostics, workspace-index, DAP, editor, or
release files as candidates merely because they participate in the shipping
server.

## Current Ownership Shape

`perl-lsp-rs-core` is not a reusable stack crate. Its public root combines:

- protocol and transport infrastructure;
- Perl feature flags and capability policy;
- providers and formatting;
- parser- and workspace-facing runtime support;
- launcher and product configuration;
- operational error classification.

`perl-lsp-rs` remains the shipping application. It re-exports the core protocol
and transport modules for compatibility while also owning `LspServer`,
dispatch, provider routing, diagnostics, workspace state, editor integration,
and server-to-client request policy.

That split is useful for the product, but neither crate is a valid dependency
for an independent language server.

## Protocol Audit

| Current surface | Classification | Evidence and blocker | Current proof |
| --- | --- | --- | --- |
| `protocol/jsonrpc.rs` | **Mixed** | Preferred first candidate after preparation. `JsonRpcId`, request, response, and error envelopes use only `serde`/`serde_json`. The same file implements `perl_parser_core::ErrorClass` for `JsonRpcError`, so the wire type directly knows the Perl workspace error taxonomy. | Inline unit tests cover integer/string IDs, response ID echo, static JSON-RPC version serialization, null IDs, and fractional IDs. Future movement must also keep `lsp_registration_tests` and the app compile checks green. |
| `protocol/errors.rs` | **Mixed** | Standard JSON-RPC/LSP codes are reusable. Product builders add provider wording, timestamps, document-specific payloads, and `server_info.name = "perl-lsp"`. Split codes and neutral constructors from application diagnostics before movement. | Core unit tests plus app dispatch and registration tests. |
| `protocol/methods.rs` | **Mixed** | Mechanically separable: standard LSP method constants are reusable. Test and project-extension constants (`$/test/slowOperation`, `experimental/testDiscovery`) and crate-specific examples belong outside the neutral registry. | Inline constant and uniqueness tests; app method-direction and dispatch tests. |
| `protocol/document_version.rs` | **Language-neutral** | Already neutral as written. The typed LSP `textDocument/version` decoder depends only on `serde_json` and bounded standard-library types. It contains no Perl feature or provider policy. | Inline decoder tests. Keep current document-lifecycle tests green when a consumer path changes. |
| `protocol/schema/**` | **Mixed** | The bounded JSON validator mechanics are reusable, but the registry includes `PerlLspExtension`, project-extension fallback, current product method coverage, and a checked-in manifest owned by this application. The generic envelope/limit validator needs an injected method registry before movement. | Inline schema tests and exact-process protocol-schema coverage. |
| `protocol/resolve_envelope.rs` and `resolve_envelope/**` | **Mixed** | Reusable mechanism and product policy are co-located. The bounded/authenticated envelope machinery is generic in shape. The wire prefix is `perl-lsp.resolve.v1`, and the closed method/family/profile/currentness model encodes the current provider product. Do not move it as a stack primitive until product identity and family registration are injected. | Inline issue/decode/auth/currentness tests and provider resolve tests. |
| `protocol/capabilities.rs` and `capabilities/**` | **Perl-specific** | The module consumes Perl feature flags, Perl completion trigger characters, current feature catalog policy, and `perl.*` commands. A later PR may extract small JSON-shape helpers, not this capability authority. | `lsp_cap_snap`, feature alignment tests, registration tests, and capability-specific unit tests. |
| `protocol/binary_identity.rs` | **Not extractable** | Canonical server, DAP, VSIX, package, and release identity belongs to the product and release surface. | Product-identity and packaging proof. |
| `protocol/error_disposition.rs` | **Perl-specific** | Its action policy is keyed by `perl_parser_core::ErrorCategory`. It is not an LSP protocol primitive. | Inline disposition tests. |
| `protocol/error_inventory.rs` | **Not extractable** | The manual inventory spans parser, LSP, and DAP error types. It is workspace governance, not reusable protocol. | Inline inventory tests. |
| `protocol/final_surface_inventory.rs` and `final_surface_inventory/**` | **Not extractable** | The ledger records this product's capability, registration, and mutation surfaces. It must remain available to detect extraction regressions but must not move into the generic crate. | Inline final-surface inventory tests. |
| `protocol/mod.rs` | **Mixed** | It aggregates every classification above and exposes an application-specific `lsp_error` helper. | Core compile and public-path tests. |

### JSON-RPC Classification Inconsistency

The first dependency-preparation PR has a concrete reconciliation target:

- `protocol/jsonrpc.rs` currently implements
  `perl_parser_core::ErrorClass` for `JsonRpcError`;
- `perl-lsp-rs/src/runtime/dispatch/response.rs` still carries a separate
  provisional `classify_jsonrpc_error` function and says direct
  implementation is future work;
- `protocol/error_inventory.rs` still records `JsonRpcError` as not
  implementing `ErrorClass`.

These three authorities cannot all describe the current code correctly. More
importantly, the foreign `ErrorClass` implementation cannot follow
`JsonRpcError` into a neutral crate without preserving a forbidden
`perl_parser_core` dependency. Because of Rust's orphan rules, the application
cannot later implement a foreign trait for a type owned by another crate.

A pre-scaffold dependency-preparation PR after PR 3 should establish one
application-owned classification seam, such as a local classifier or local
wrapper, remove the parser taxonomy from the wire type, update the inventory,
and prove the observed category mapping has not changed. It should not
introduce a generic error-classification framework into `lsp-stack`.

## Transport Audit

| Current surface | Classification | Evidence and blocker | Current proof |
| --- | --- | --- | --- |
| `transport/framing.rs`: `ContentLengthFramer`, `frame`, frame limits, header parsing | **Mixed** | Later low-risk candidate after preparation. The byte-framing kernel is language-neutral. `FramingError` directly implements `perl_parser_core::ErrorClass`, so even the low-level unit carries a Perl dependency. | Extensive inline framing tests cover split frames, malformed headers, bounds, resynchronization, and round trips. |
| `transport/framing.rs`: request reader/writer and JSON codec | **Mixed** | The high-level layer imports current JSON-RPC types, tracing policy, lossy body logging, and application recovery behavior. It should move only after the envelope types and low-level framer have independent boundaries. | Inline reader/writer tests plus raw-RPC and registration receipts. |
| `transport/framing.rs`: client response conversion | **Perl-specific** | Server-originated responses are converted into the pseudo-notification `$/perl-lsp/clientResponse`. A reusable stack should route responses as responses through an explicit registry/channel, not encode the Perl compatibility method. | `read_next_converts_client_response_to_internal_notification` and server-request tests. |
| `perl-lsp-rs/src/transport/mod.rs` | **Not extractable** | It re-exports `perl-lsp-rs-core::transport` for current callers. Keep it until post-move parity and import migration are complete. | App compile and transport consumers. |

The first framing movement should include only the byte framer, frame builder,
limits, and byte-level tests. JSON parsing, request routing, logging policy, and
the client-response shim are separate claims.

## Runtime Audit

| Current surface | Classification | Evidence and blocker | Current proof |
| --- | --- | --- | --- |
| `perl-lsp-rs/src/runtime/scheduler.rs` | **Mixed** | Request classification, priority ordering, bounded queues, deduplication, and sequence barriers are reusable in shape. The current `Scheduler` owns an `Arc<LspServer>`, calls application handlers, reads document generation and instance state, installs diagnostics/file-watcher/parse workers, emits application timing, and writes through the current outbound channel. Split algorithms from server integration; do not move the file wholesale. | Inline classification, priority, heap, dedup, generation/instance freshness, panic-response, queue/shutdown, and `rapid_typing_stale_reads_cancel_before_worker_permit_receipt` tests; raw-RPC and lean startup-trace receipts. |
| `perl-lsp-rs/src/runtime/lifecycle/mod.rs`, `capabilities.rs`, `watchers.rs` | **Mixed** | Initialize-result shapes, client capability negotiation, and dynamic-registration mechanics coexist with Perl globs and language IDs, `perl.*` commands, `perl-lsp/index-ready`, workspace-index state, client-specific overrides, and runtime tuning. Extract only policy-injected helpers after their protocol shapes are independently proven. | `lsp_3_17_lifecycle_tests`, `lsp_registration_tests`, `lsp_inline_completion_registration_tests`, `lsp_cap_snap`, lifecycle unit tests, and LSP smoke E2E. |
| `perl-lsp-rs/src/runtime/lifecycle/module_resolution.rs`, `inc_context/**`, `workspace.rs`, `tools.rs` | **Perl-specific** | These modules own `@INC`, Perl module resolution, perltidy/perlcritic integration, workspace roots/scanning, and index coordination. They remain application behavior. | Module-resolution, workspace-folder/index, tool, and integration tests. |
| `perl-lsp-rs/src/runtime/lifecycle/final_surface_census.rs` | **Not extractable** | This is a current-product capability and lifecycle proof census, not reusable runtime infrastructure. | Inline final-surface census tests. |
| `runtime/cancellation/mod.rs` | **Mixed** | Atomic token/registry mechanics are reusable, but the public token is named `PerlLspCancellationToken` and carries provider strings, provider cleanup callbacks, JSON values, metrics, and current JSON-RPC ownership. Split mechanism from provider cleanup policy after the JSON-RPC boundary exists. | Inline model/property, registry, cleanup, and latency-oriented tests; app cancellation tests. |
| `runtime/limits/mod.rs` | **Mixed** | `MemoryBudget`, `MemoryMonitor`, and deadline primitives are generic. `LspLimits` combines them with AST cache, parser, index, provider result, diagnostics, and workspace defaults. Do not move the module wholesale. | Inline memory/limit tests and affected provider/index tests. |
| `runtime/input_validation/**` | **Mixed** | Structural JSON-RPC admission and bounded strings are potentially reusable. File extensions, Perl source examples, supported URI schemes, current product size limits, and `$/perl-lsp/clientResponse` policy are application-owned. | Inline validation boundary tests and sync-sink tests. |
| `runtime/tuning.rs` | **Perl-specific** | `PERL_LSP_*` environment variables, syntax-only diagnostics, eager indexing, watcher behavior, and launcher precedence describe this application and its editor harnesses. | Inline env/default/CLI overlay tests plus registration and lean/e2e receipts. |
| `runtime/launcher/mod.rs` | **Not extractable** | The module owns Perl LSP CLI, feature profiles, feature grid, logging environment, socket defaults, product startup reporting, and binary behavior. `launcher/timing.rs` may be audited separately later; the launcher is not a stack unit. | Launcher unit tests, CLI/BDD proof, and binary smoke tests. |
| `runtime/text_utils/**` | **Perl-specific** | The helper encodes Perl statement (`;`), subroutine (`sub `), pragma (`#!`), and import (`use`/`require`) placement semantics. It is provider-adjacent application code, not generic text editing. | Provider and text-edit tests. |
| `runtime/mod.rs` | **Mixed** | It aggregates unrelated classifications and still documents an old transport dependency-cycle deferral even though transport now lives in the same crate. The stale history must not be used as extraction authority. | Core compile. |

## Shipping-App Integration Audit

| Current surface | Classification | Evidence and blocker | Current proof |
| --- | --- | --- | --- |
| `perl-lsp-rs/src/protocol/mod.rs` | **Not extractable** | Re-exports core protocol and product identity so the shipping facade remains stable. Keep until consumer imports are migrated after parity. | App compile and public API tests. |
| `perl-lsp-rs/src/runtime/client_requests.rs` | **Mixed** | Request-ID allocation, initialized-state rejection, and sink emission are potentially reusable. The implementation is an inherent `LspServer` API with current capability checks and app-owned outbound sink. The accepted tranche does not authorize generic handler traits or a server rewrite. | Server-request unit tests, registration tests, and raw-RPC receipts. |
| `perl-lsp-rs/src/runtime/dispatch/lifecycle.rs` | **Mixed** | Standard initialize/initialized/shutdown/exit and trace-state transitions coexist with the inherent `LspServer`, compatibility auto-initialization, startup registrations, workspace indexing, the Perl index-ready notification, resolve-session teardown, logging policy, and process exit. Extract lifecycle state mechanics only after application side effects are explicit ports. | Inline lifecycle dispatch tests, `lsp_3_17_lifecycle_tests`, registration tests, and LSP smoke E2E. |
| `perl-lsp-rs/src/protocol/method_direction.rs` | **Mixed** | The descriptor shape and standard rows are reusable, but the registry includes project extensions and is mechanically synchronized with the current dispatch and outbound inventories. Revisit only after a neutral standard-method registry exists. | Method-direction tests. |
| Remaining `perl-lsp-rs/src/runtime/dispatch/**`, diagnostics, document/workspace state, providers | **Perl-specific** | After separating the lifecycle row above, these surfaces consume parser facts, documents, workspace generations, feature policy, provider behavior, test endpoints, and current product receipts. They are integration consumers of a future stack, not first extraction candidates. | Current app unit, integration, raw-RPC, lean/e2e, and provider receipts. |

## Recommended Sequence from This Audit

### PR 3: Record the dependency blocker

PR 3 remains the audit-only step defined by the implementation plan. It should:

- record the direct `perl_parser_core::ErrorClass` dependency in
  `protocol/jsonrpc.rs`;
- record the contradictory classification authorities as a blocker rather than
  declaring the file dependency-clean;
- record the intended `std` + `serde` + `serde_json` closure;
- move no code and create no `lsp-stack` crate.

A dependency audit that proves the preferred candidate is still mixed is a
valid result. Do not widen PR 3 to repair production code.

### Pre-scaffold dependency-preparation PR

After PR 3 lands, one bounded dependency-boundary PR should:

- reconcile the three JSON-RPC classification authorities;
- remove the direct `perl_parser_core::ErrorClass` dependency from
  `protocol/jsonrpc.rs`;
- keep category mapping in an application-owned seam where current consumers
  need it;
- update affected inventory and tests;
- move no files and create no `lsp-stack` crate.

The equivalent `FramingError` cleanup should be separate unless the proof shows
that combining the two removes one shared authority rather than joining two
independent claims.

### Pre-scaffold dependency closure re-check

After the preparation PR, prove the first candidate set has this closure:

```text
protocol/jsonrpc.rs
└── std
    serde
    serde_json
```

The audit does not authorize adding dependencies to make that proof pass.
`thiserror`, `lsp-types`, tracing, Tokio, parser crates, provider crates, and
product configuration are not required by the first JSON-RPC move.

Record the exact source set and dependency closure in the dependency-audit
artifact or a bounded follow-up receipt. Re-run the source scan after the
preparation PR; do not treat this audit snapshot as build proof.

### PR 4: Re-prove the current-app test baseline

After PR 3 and any required dependency preparation, run the current application
baseline before creating a crate boundary. The baseline must cover protocol,
capability/registration, lifecycle, scheduler/stale-read, raw-RPC, and synthetic
Neovim-shaped lean behavior at one exact commit. Test or receipt hardening is
valid; product behavior changes are separate PRs.

The implementation plan owns the full command list. The required runtime
receipts are also repeated below so this audit is executable without inferring
them from the spec.

### PR 5: Scaffold only the neutral crate

Create the minimal crate only after PR 4 records a green current-app baseline
and the required dependency-preparation and closure proof establish the
boundary. The scaffold should contain no copied production behavior and no Perl
dependency. Its package metadata must avoid claims of release readiness.

### PR 6: Move JSON-RPC IDs and envelopes

Move `JsonRpcId`, `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcError`, and
`JSONRPC_VERSION` with their existing parse/serialize tests. Preserve the
current application import path through a temporary re-export, then prove the
shipping server still compiles and its registration tests remain green.

### Later bounded moves

Recommended order after the first movement proves the pattern:

1. standard LSP method constants, excluding product/test extensions;
2. typed document-version decoding;
3. low-level `Content-Length` framing;
4. neutral error-code constants and constructors;
5. generic schema envelope/limit mechanics after method-registry injection;
6. cancellation mechanism after provider cleanup policy is split;
7. server-originated request primitives after response routing is explicit.

Capability policy, tuning, launcher, provider resolve policy, product identity,
DAP, editor integration, and release surfaces remain in the application.

## Proof Map for Future PRs

The cheapest discriminating proof should run before broader app proof.

### JSON-RPC dependency preparation and movement

```bash
./scripts/cargo-safe test -p perl-lsp-rs-core protocol::jsonrpc --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs-core --all-targets --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs --all-targets --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked
```

Add a dependency-boundary check that fails when any first-candidate source
imports a `perl-*` crate or application module. The exact command belongs to PR
3 after the checked mechanism exists.

### Capability or registration-adjacent movement

```bash
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_inline_completion_registration_tests --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_cap_snap --profile agent --locked
```

### Transport, lifecycle, or runtime movement

Run the relevant focused/unit proof plus these exact product-side receipts:

```bash
./scripts/cargo-safe test -p perl-lsp-rs --lib --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_3_17_lifecycle_tests --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked

PERL_LSP_E2E=1 \
PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 \
PERL_LSP_DIAGNOSTIC_MODE=syntax-only \
cargo test -p perl-lsp-ux-tests --test ux_latency_raw_rpc -- --test-threads=1 --nocapture

PERL_LSP_E2E=1 \
PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 \
PERL_LSP_DIAGNOSTIC_MODE=syntax-only \
cargo test -p perl-lsp-ux-tests --test ux_neovim_lean_startup_trace -- --test-threads=1 --nocapture
```

Receipt ownership and the synthetic-versus-actual-host claim boundary live in
[`docs/project/status/neovim_latency.md`](../../docs/project/status/neovim_latency.md).
Do not replace product receipts with isolated generic-crate tests: both sides of
the new boundary must remain proven.

## Change Declaration for This Audit

| Surface | Changed? |
| --- | --- |
| Code location | No |
| Runtime behavior | No |
| JSON-RPC behavior or wire shape | No |
| Capability shape | No |
| Dynamic registration | No |
| Editor integration | No |
| Dependencies | No |
| Release, package, signing, or marketplace behavior | No |
| `crates/lsp-stack` | Not created |

## Non-goals

This audit does not:

- authorize code movement;
- authorize a crate scaffold;
- define generic handler traits;
- rewrite dispatch or response routing;
- extract inline completion or capability policy;
- extract DAP;
- change editor or runtime behavior;
- change package/release metadata;
- claim that the future crate is publishable or release-ready.

## Rollback and Freshness

Rollback is a documentation revert. No runtime restoration is required because
this audit moves no code.

The classifications are source-backed at
`a9664af790888333efbe50a042fa060f3cc2d171`. A future implementation PR must
re-read the candidate files and prove their dependency closure at its own base.
New imports, moved policy, or changed consumers may invalidate an individual
row without invalidating the governing ADR or staged sequence.
