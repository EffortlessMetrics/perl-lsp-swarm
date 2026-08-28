# lsp-stack Static Seam Audit

Status: candidate
Date: 2026-08-27
Owner: perl-lsp maintainers
Tracking issue: #13080
Audit base: `a9664af790888333efbe50a042fa060f3cc2d171`
Governing ADR: [PLSP-ADR-0004](../../docs/adr/PLSP-ADR-0004-lsp-stack-extraction.md)
Governing spec: [PLSP-SPEC-0028](../../docs/specs/PLSP-SPEC-0028-lsp-stack-extraction.md)
Implementation plan: [lsp-stack extraction](implementation-plan.md)

## Decision

The current `perl-lsp-rs-core` source tree does not contain a whole protocol,
transport, or runtime module that can move to `lsp-stack` unchanged. The public
module layout now exposes the intended areas, but each area still mixes reusable
LSP mechanics with at least one current-app authority: Perl error taxonomy,
product identity, provider behavior, feature policy, internal routing, editor
receipts, or runtime tuning.

Two whole files inspected here are language-neutral:

- `protocol/document_version.rs` is LSP-specific, self-contained, and has no
  direct Perl dependency. It is the lowest-risk first mechanical move after the
  dependency audit and crate scaffold.
- `runtime/launcher/timing.rs` is also language-neutral, but it is generic
  startup telemetry rather than an LSP-stack responsibility. It should stay in
  the current app unless a second LSP-stack consumer establishes that ownership.

`protocol/document_version.rs` currently has no production consumer outside its
protocol export. Moving it would therefore prove packaging, dependency, and test
separation with very low behavior risk, but it would not prove current-app
integration parity. The first meaningful integrated primitive should be the
`JsonRpcId` and JSON-RPC envelope core after the app-owned operational error
classification is split out of `protocol/jsonrpc.rs`.

No `crates/lsp-stack` scaffold or production code move is justified by this
audit alone. PR 3 must still prove the selected dependency boundary.

## Classification Rules

This audit classifies the current source, not an imagined abstraction after a
rewrite.

- **Language-neutral**: the current unit expresses LSP, JSON-RPC, transport, or
  runtime mechanics without depending on Perl crates or current-app policy.
- **Mixed**: reusable mechanics and current-app ownership occupy the same file or
  module. The unit must be split before any move.
- **Perl-specific**: the unit owns Perl parsing, provider behavior, feature
  policy, editor behavior, or another language-specific contract.
- **Not extractable**: the unit belongs to product, DAP, packaging, release,
  identity, or current-app proof authority even when its implementation is not
  syntactically Perl-specific.

A blocking dependency can be either:

1. a direct Rust dependency such as `perl_parser_core`; or
2. a current-app contract such as the `perl-lsp` product name, a private RPC
   method, provider-family registry, feature catalog, or editor receipt.

The second form is still a dependency. Cargo metadata alone cannot prove that a
file is reusable.

## Candidate Audit

| Candidate | Class | Blocking coupling or ownership | Existing tests and receipts | Disposition |
| --- | --- | --- | --- | --- |
| `protocol/document_version.rs` | Language-neutral | None found. Imports only `serde_json::Value` and `std`; performs no stale/equal-version policy. Repo search found no production consumer outside the protocol export. | Inline `document_version::tests` cover absent fields, explicit null, valid i32 values, wrong JSON kinds, fractional values, and bounded out-of-range classes. | First mechanical move after PR 3 and the crate scaffold. Do not claim integration parity from this move alone. |
| `protocol/jsonrpc.rs` | Mixed | `JsonRpcId`, request/response envelopes, and serialization are neutral. `impl perl_parser_core::ErrorClass for JsonRpcError` imports the Perl workspace error taxonomy and joins the envelope to app operational policy. | Inline tests: `json_rpc_request_accepts_integer_id`, `json_rpc_request_accepts_string_id`, `json_rpc_response_echoes_string_id`, `json_rpc_response_serializes_static_jsonrpc_version`, `json_rpc_rejects_null_id_for_request`, and `json_rpc_rejects_fractional_id`. Current app consumers include server, dispatch, serving, response, cancellation, outbound, and scheduler paths. | Split the `ErrorClass` adapter into current-app code, then move the typed ID and envelope core as the first integrated primitive. |
| `protocol/errors.rs` | Mixed | Standard JSON-RPC/LSP codes are neutral. The same file embeds `perl-lsp` server identity, provider-aware cancellation metadata, current-app document-not-found payloads, transport helpers, and request-parameter extraction policy. | Inline `errors::tests` pin error-code values, builders, cancellation responses, and request URI/position/range extraction. `lsp_protocol_violations` and cancellation tests exercise app-level behavior. | Separate standard codes and generic builders from product identity and provider/application helpers before considering a move. |
| `protocol/methods.rs` | Mixed | Standard LSP method constants are neutral. `$/test/slowOperation` and `experimental/testDiscovery` are current-app or harness extensions, not standard stack authority. There is no direct Perl crate import; the blocker is extension-method ownership. | Inline tests cover lifecycle, text-document, workspace, hierarchy, special, window, notification, refresh, and uniqueness contracts. | Split standard methods from app-private and test methods. Standard constants may follow the JSON-RPC core; private methods stay in the app. |
| `transport/framing.rs` | Mixed | `ContentLengthFramer`, `FramingError`, and `frame` are neutral mechanics. `FramingError` implements `perl_parser_core::ErrorClass`; response decoding synthesizes the private `$/perl-lsp/clientResponse` method; higher-level read policy, logging, and current request routing occupy the same file. | Inline framing tests cover malformed and oversized headers, split and back-to-back frames, UTF-8 handling, request/response conversion, writers, and logging. Current receipts include `perl-lsp-ux-tests --test ux_latency_raw_rpc` and app protocol-violation tests. | First transport move should contain only the low-level framer after the error adapter and private response-routing conversion are split out. |
| `runtime/cancellation/mod.rs` | Mixed | Atomic token and registry mechanics are reusable. The current file owns `PerlLspCancellationToken`, provider strings, provider cleanup callbacks, a `CancellableProvider` trait, a process-global registry, and `perl_parser_core::ErrorClass`. | Inline tests cover token creation, atomic cancellation, registry behavior, provider cleanup, metrics, RAII cleanup, and typed integer/string IDs. App tests include `lsp_cancel_test`, `lsp_cancellation_protocol_tests`, `lsp_cancellation_infrastructure_tests`, `lsp_cancellation_performance_tests`, and `lsp_concurrent_request_management_tests_simple`. | Split a language-neutral token/registry core from provider cleanup, global ownership, and operational classification. Preserve all app cancellation tests when the core later moves. |
| `runtime/limits/mod.rs` | Mixed | `MemoryBudget`, `MemoryPressure`, and `MemoryMonitor` are neutral and have no production consumer found outside this module. `LspLimits` also owns AST and symbol cache limits, workspace indexing, diagnostics, completion, code-lens and inlay-hint caps, and the current `perl.limits` settings shape. | Inline `limits::tests` cover presets, settings updates, and result caps. `tests/runtime_limits_memory.rs` covers memory budgets, pressure, monitor accounting, saturation, concurrency, logging, global accessors, and settings joins; `runtime_g2_api_stability` and `runtime_g2_module_shape` protect the public runtime surface. | Keep `LspLimits` in the app. Extract the memory monitor only after a real LSP-stack consumer exists; do not use `lsp-stack` as a generic utility crate. |
| `runtime/launcher/timing.rs` | Language-neutral | No Perl dependency in the timer or report model. Its only product coupling is the logging example and current launcher placement. The mechanism is generic startup telemetry rather than protocol/runtime authority. | Inline tests: `timer_records_phases`, `empty_timer_reports_total`, `to_json_is_valid`, and `display_contains_phase_names`. | Leave in the current app. Reconsider only if stack-owned lifecycle startup needs the same timer as a second consumer. |

## Candidate Notes

### Lowest-risk first mechanical move

`protocol/document_version.rs` wins on the deciding criterion: it is the
smallest whole file that is both LSP-specific and already free of Perl and
current-app dependencies. Its bounded typed decoder and inline tests can move
without changing request routing, capabilities, providers, transport, or runtime
state.

That recommendation changes in either of two cases:

- PR 3 finds a hidden build, generated-code, or packaging dependency that is not
  visible in the source import graph; or
- the extraction train requires the first move to prove a live current-app call
  path rather than the crate and dependency seam.

In the second case, the replacement recommendation is not a broader file move.
It is a preliminary split of `protocol/jsonrpc.rs`, followed by moving
`JsonRpcId`, `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcError`, and
`JSONRPC_VERSION` while retaining the `perl_parser_core::ErrorClass` adapter in
the current app.

### First transport move

Do not move `transport/framing.rs` as one unit. The low-level framing boundary is:

- `ContentLengthFramer`
- `FramingError`
- `MAX_FRAME_SIZE`
- `frame`
- private header parsing and resynchronization helpers

Before that subset moves, the current-app operational error adapter must leave
the file. The higher-level conversion of server-originated responses into
`$/perl-lsp/clientResponse` must remain app-owned unless it is replaced by a
language-neutral response channel with equivalent receipts.

### First runtime move

Cancellation is a better runtime candidate than tuning or limits, but only after
one cut. A future stack can own typed request cancellation and bounded registry
mechanics. The current app must continue to own provider cleanup policy,
provider names, process-global registry placement, and the mapping into the Perl
workspace error taxonomy.

## Surfaces That Stay in the Current App

| Surface | Class | Reason |
| --- | --- | --- |
| `providers/**` | Perl-specific | Provider implementations, facts, routing, resolve behavior, and inline-completion behavior are the language service, not reusable stack infrastructure. |
| `protocol/capabilities.rs` and `protocol/capabilities/**` | Perl-specific | Capability shape is built from the Perl feature catalog, Perl trigger characters, `perl.*` commands, current lsp-types compatibility patches, and current-app advertisement policy. Generic capability primitives must be introduced separately, not extracted by moving this builder. |
| `features/**` and `governance/**` | Perl-specific | They own feature availability, profiles, support claims, and final-surface policy for this product. |
| `runtime/tuning.rs` | Perl-specific | The dials encode `PERL_LSP_*` environment variables, diagnostic pipeline scope, eager indexing, watcher behavior, and editor/e2e defaults. These are current-app workload policy. |
| `runtime/input_validation/**` | Mixed, retained | File validation includes Perl extension policy; URI and request admission encode current sync-sink and private-method behavior; content bounds join current `LspLimits`. Small generic helpers do not justify moving the package. |
| `runtime/text_utils/**` | Perl-specific | Statement boundaries, `sub`, shebang, pragma, `use`, and `require` placement encode Perl source-edit semantics. |
| `runtime/launcher/mod.rs` | Not extractable | The launcher owns the Perl binary CLI, feature profiles, `PERL_LSP_*` logging, transport selection, process startup, and product output. The generic timing child does not make the launcher a stack boundary. |
| `protocol/resolve_envelope.rs` | Mixed, retained | The implementation is generic-looking, but the wire prefix, resolve-family registry, provider methods, profile/currentness relations, and session ownership are current-app provider contracts. |
| `protocol/binary_identity.rs` | Not extractable | This is canonical server, DAP, VSIX, repository, candidate, and release identity. It belongs to product and packaging authority. |
| `protocol/error_disposition.rs` and `protocol/error_inventory.rs` | Perl-specific, retained | They are built on `perl_parser_core::ErrorCategory` and inventory current Perl/DAP error families. They are operational governance, not JSON-RPC protocol mechanics. |
| `protocol/final_surface_inventory.rs` and `protocol/final_surface_inventory/**` | Not extractable | The inventory is current-app proof authority for advertised, registered, routed, and implemented surfaces. It must remain the parity oracle while primitives move. |
| `protocol/schema/**` | Mixed, retained | Payload schemas include current product methods and provider payloads. A later per-payload audit may identify neutral types; the directory is not a move unit. |
| DAP, perltidy, subprocess runtime, packaging, signing, publishing, marketplace, and release automation | Not extractable | Explicitly excluded by the ADR and spec. |

## Intended Dependency Direction

The extraction boundary should produce one dependency direction:

```text
perl-lsp current app
  ├── app adapters: error classification, private methods, provider cleanup,
  │                capability policy, tuning, identity, receipts
  ├── Perl domain: parser, lexer, AST, semantic analysis, workspace, providers
  └── future lsp-stack
       ├── JSON-RPC ids and envelopes
       ├── standard LSP method/error primitives
       ├── Content-Length framing
       └── bounded cancellation/lifecycle primitives
```

`lsp-stack` must not depend upward on app adapters or sideways on any `perl-*`
crate. The current app may depend on both the future stack and the Perl domain.
Operational classification must be an app-owned adapter over stack errors, not a
trait implementation that pulls `perl_parser_core` into the stack.

## Observed Authority Drift

Two current comments/inventories must not be treated as extraction authority:

1. `runtime/mod.rs` still says transport absorption was deferred because of a
   dependency cycle, while `transport` is now a top-level module in the same
   crate. The current tree, ADR, spec, and this audit supersede that stale
   narrative.
2. `protocol/error_inventory.rs` records `JsonRpcError` as lacking
   `ErrorClass`, while `protocol/jsonrpc.rs` currently implements
   `perl_parser_core::ErrorClass` for it. The inventory is stale and cannot be
   used as proof that JSON-RPC is already free of Perl operational policy.

These are follow-up current-app documentation/proof repairs. Correcting them is
not part of this docs-only PR because PR 2 permits no production-source change.

## Recommended PR Sequence After This Audit

1. **PR 3: dependency audit.** Prove `protocol/document_version.rs` compiles
   with only language-neutral dependencies. Record the absence of production
   consumers so later reviewers do not overstate parity.
2. **PR 4: crate scaffold.** Create the empty `lsp-stack` crate only after the
   dependency proof lands. It must have no `perl-*` dependency.
3. **PR 5: first primitive move.** Move `document_version.rs` and its tests with
   no behavior or capability change. Treat this as crate-seam proof, not live
   integration proof.
4. **JSON-RPC preparation.** Move the `ErrorClass` implementation and any
   current-app classification join out of `protocol/jsonrpc.rs`; then audit the
   resulting envelope imports again.
5. **First integrated move.** Move the typed JSON-RPC ID and envelope core while
   preserving all current request, response, cancellation, and protocol
   violation tests.
6. **Transport preparation and move.** Split the low-level framer from private
   response routing and app error classification, then move only the framer.
7. **Runtime preparation and move.** Split cancellation mechanics from provider
   cleanup and global app ownership, then move the neutral core.
8. **Defer generic utilities.** Move memory monitoring or startup timing only
   when stack-owned behavior supplies a real second consumer.

## Future Proof Map

This audit does not claim these commands passed. They are the focused proof map
for the dependency and move PRs.

### Dependency and first primitive

```bash
git diff --check
./scripts/cargo-safe test -p perl-lsp-rs-core --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs-core --all-targets --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs --all-targets --profile agent --locked
```

### JSON-RPC integrated move

```bash
./scripts/cargo-safe test -p perl-lsp-rs-core --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_protocol_violations --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_cancel_test --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs --all-targets --profile agent --locked
git diff --check
```

### Framing move

```bash
./scripts/cargo-safe test -p perl-lsp-rs-core --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_protocol_violations --profile agent --locked
PERL_LSP_E2E=1 PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 PERL_LSP_DIAGNOSTIC_MODE=syntax-only cargo test -p perl-lsp-ux-tests --test ux_latency_raw_rpc -- --test-threads=1 --nocapture
git diff --check
```

### Cancellation move

```bash
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_cancel_test --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_cancellation_protocol_tests --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_cancellation_infrastructure_tests --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_concurrent_request_management_tests_simple --profile agent --locked
git diff --check
```

## Change Declaration for This Audit

- Production code location changed: no
- Runtime behavior changed: no
- JSON-RPC parse or serialization behavior changed: no
- Capability JSON changed: no
- Dynamic registration changed: no
- Editor integration or receipts changed: no
- Dependencies changed: no
- DAP changed: no
- Release, package, signing, publish, or marketplace surfaces changed: no
- Tests or fixtures changed: no
- Documentation changed: this audit only

## Limits

This is a static source and ownership audit at the exact base named above. It is
not a compiler-backed dependency proof, a public-API compatibility proof, or a
runtime parity receipt. PR 3 must refresh any row whose source changes before it
uses this audit to authorize a scaffold or move.

## Rollback

Revert this document. No runtime rollback is required because no code,
dependency, capability, registration, editor, DAP, or release surface changed.
