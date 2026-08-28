# lsp-stack Static Seam Audit

Status: complete
Owner: perl-lsp maintainers
Audited revision: `a9664af790888333efbe50a042fa060f3cc2d171`
Linked ADR: [PLSP-ADR-0004](../../docs/adr/PLSP-ADR-0004-lsp-stack-extraction.md)
Linked spec: [PLSP-SPEC-0028](../../docs/specs/PLSP-SPEC-0028-lsp-stack-extraction.md)
Linked plan: [implementation plan](implementation-plan.md)

## Conclusion

`perl-lsp-rs-core` is not itself an extraction unit. Its manifest directly
depends on the parser, lexer, semantic, workspace, tooling, and other Perl
crates, while its `protocol`, `transport`, and `runtime` modules still mix
language-neutral mechanics with Perl product policy.

One audited production file is ready to enter the dependency-boundary audit
without a preparatory split:

- `crates/perl-lsp-rs-core/src/protocol/document_version.rs`

The foundational JSON-RPC and framing seams are reusable but not currently
movable as whole files. Their remaining blockers are narrow and explicit:

- `protocol/jsonrpc.rs` implements the Perl workspace's
  `perl_parser_core::ErrorClass`
- `protocol/errors.rs` combines standard JSON-RPC/LSP codes with Perl server and
  provider-specific builders
- `transport/framing.rs` implements `ErrorClass` and converts server-originated
  responses into the product-specific `$/perl-lsp/clientResponse` pseudo-method

Do not create `crates/lsp-stack` yet. PR 3 must prove dependency closure for the
first candidate set, and the mixed foundational seams need in-place separation
before they can join that set.

## Claim Boundary

This is the PR 2 static source audit required by the implementation plan. It
classifies the current LSP-facing source and names the existing proof attached
to each candidate.

This audit does not:

- move production code
- create `crates/lsp-stack`
- change JSON-RPC or LSP wire behavior
- change capability JSON or dynamic registration
- change dependencies
- change DAP, packaging, signing, publishing, or release behavior
- prove runtime parity or independent compilation

The classifications are pinned to the audited revision above. Later source
changes require rechecking the affected rows rather than silently treating this
document as current proof.

## Classification Rules

| Classification | Meaning |
| --- | --- |
| Language-neutral | The whole audited file can plausibly compile in a Perl-free crate with only neutral dependencies. |
| Mixed | Reusable infrastructure and Perl/product policy currently share the same file or module subtree. |
| Perl-specific | The surface intentionally encodes Perl language, provider, feature, product, or release behavior. |
| Not extractable in this lane | The surface is test/governance inventory, DAP/release identity, or otherwise outside the accepted `lsp-stack` boundary. |

A reusable algorithm inside a mixed file is not a movable candidate until its
file-level dependency and policy blockers are separated.

## Current Dependency Fact

`crates/perl-lsp-rs-core/Cargo.toml` directly depends on, among others,
`perl-parser-core`, `perl-parser`, `perl-lexer`, `perl-ast`,
`perl-semantic-analyzer`, `perl-semantic-facts`, `perl-workspace`,
`perl-module`, `perl-symbol`, `perl-diagnostics`,
`perl-subprocess-runtime`, and `perl-lsp-perltidy`.

Therefore the current crate graph cannot establish a Perl-free stack boundary.
The audit must select source below the crate level and PR 3 must prove the
selected dependency closure independently.

## Protocol Audit

| Surface | Classification | Blocking evidence | Existing proof | Disposition |
| --- | --- | --- | --- | --- |
| `protocol/document_version.rs` | Language-neutral | None found. It uses `serde_json::Value` and `std`, and implements bounded LSP `textDocument.version` decoding without parser, provider, workspace, or product policy. | Colocated unit tests cover absence, explicit null, JSON type errors, i32 boundaries, out-of-range integers, and field-order independence. | Put this whole file in the PR 3 dependency audit. It is the safest first mechanical move after a scaffold exists. |
| `protocol/jsonrpc.rs` | Mixed | JSON-RPC IDs and envelopes are neutral, but `JsonRpcError` implements `perl_parser_core::ErrorClass` and reaches the current app's error-code module. | Colocated unit tests cover integer/string IDs, response ID echoing, response serialization, null-ID rejection, and fractional-ID rejection. | Move operational error classification to an app-owned adapter, retain standards-only codes beside the neutral types, then re-audit the file. |
| `protocol/errors.rs` | Mixed | Standard JSON-RPC/LSP codes share the file with provider-aware cancellation data, timestamps, a hard-coded `perl-lsp` server identity, and app response builders. | Colocated protocol error-builder tests and current `perl-lsp-rs-core` unit coverage. | Split standards-only codes and generic wire builders from app-rich error construction before extraction. |
| `protocol/methods.rs` | Mixed | Standard LSP method constants share the registry with `$/test/slowOperation` and `experimental/testDiscovery`. | Colocated lifecycle, document, workspace, hierarchy, special-method, refresh, and uniqueness tests. | Separate the standard registry from product/test extensions. The standard registry can then join the neutral candidate set. |
| `protocol/schema/**` | Mixed | Generic bounded JSON-RPC validation shares the subtree with `PerlLspExtension`, a project-specific extension fallback, the checked-in product schema manifest, and method-family policy. | Colocated schema validator and registry tests, plus current protocol contract tests. | Keep the registry and extension policy app-owned. Extract validator mechanics only after the split is explicit and separately tested. |
| `protocol/resolve_envelope/**` | Perl-specific | The token prefix is `perl-lsp.resolve.v1:` and the closed family registry encodes current provider families and methods. | Colocated issue/decode/authentication, bound, replay, and typed-subject tests. | Keep this provider contract in the current app. A generic authenticated-envelope primitive would need its own bounded design, not a rename. |
| `protocol/capabilities.rs` and `protocol/capabilities/**` | Perl-specific | Capability construction consumes the app feature flags, Perl trigger characters, Perl command IDs, experimental policy, and current static/dynamic advertisement choices. | Colocated capability tests, `lsp_cap_snap`, inline-completion registration tests, and general registration tests. | Keep capability policy in the current product. Only later-proven generic capability-shape helpers may cross the boundary. |
| `protocol/binary_identity.rs` | Not extractable in this lane | The contract binds Perl server, DAP, VSIX, repository, package, and artifact identity. | Colocated identity/compatibility tests and product-identity proof. | Keep with product and release identity. |
| `protocol/error_disposition.rs` and `protocol/error_inventory.rs` | Not extractable in this lane | They depend on the Perl workspace-wide `ErrorCategory` taxonomy and inventory LSP, parser, and DAP error types. | Colocated mapping and inventory tests. | Keep as app/workspace governance. Do not make the neutral stack depend on this taxonomy. |
| `protocol/final_surface_inventory/**` | Not extractable in this lane | Test-only final capability, registration, and mutation governance belongs to the current product surface. | Existing final-surface unit inventory and current registration/capability tests. | Retain as current-app parity proof while primitives move. |
| `protocol/mod.rs` | Mixed | It re-exports every category above and adds an app-facing `lsp_error` convenience. | Entire `perl-lsp-rs-core` unit suite. | Keep as the current-app integration facade until extracted modules have independent parity. |

## Transport Audit

| Surface | Classification | Blocking evidence | Existing proof | Disposition |
| --- | --- | --- | --- | --- |
| Low-level `ContentLengthFramer`, `FramingError`, and `frame` logic in `transport/framing.rs` | Mixed file; neutral subset | The codec itself uses `std`, but the same file implements `perl_parser_core::ErrorClass`. | Colocated framing tests cover split headers/bodies, malformed and oversized frames, resynchronization, and framing output. | First split the low-level codec and move operational error classification to the app boundary. |
| `ContentLengthMessageReader`, `read_message`, response/notification writers, and logging in `transport/framing.rs` | Mixed | The reader depends on current JSON-RPC types and rewrites client responses to `$/perl-lsp/clientResponse`; logging behavior is current-app policy. | Colocated reader/writer tests, including client-response conversion, plus the raw-RPC receipt. | Move only after neutral JSON-RPC types are proven. Keep pseudo-method routing and product logging outside the transport primitive. |
| `transport/mod.rs` | Mixed | It re-exports the low-level codec and current-app message I/O as one surface. | Current transport unit tests and raw-RPC receipt. | Preserve as an app facade until the two layers are separated. |

## Runtime Audit

| Surface | Classification | Blocking evidence | Existing proof | Disposition |
| --- | --- | --- | --- | --- |
| `runtime/cancellation/mod.rs` | Mixed | Atomic cancellation mechanics share the module with `PerlLspCancellationToken`, provider names, provider cleanup parameters, product metrics, and the current JSON-RPC ID type. | Colocated cancellation registry, cache, cleanup, metric, and model/property tests. | Do not move wholesale. First isolate a provider-neutral token/registry contract after neutral request IDs exist. |
| `runtime/limits/mod.rs` | Mixed | `MemoryBudget`, `MemoryPressure`, and `MemoryMonitor` are generic, while `LspLimits` encodes AST, parser, indexing, diagnostics, completion, code-lens, and provider policy. | Colocated memory-budget, pressure, cap, and default-limit tests. | A generic budget primitive may be extracted later; current LSP/provider limits remain app policy. |
| `runtime/tuning.rs` | Perl-specific | The shape owns `PERL_LSP_*` environment variables, Perl diagnostic modes, workspace indexing, watcher defaults, and launcher/feature-profile integration. | Colocated precedence/default/parse tests, registration tests, and the Neovim lean-startup receipt. | Keep in the current app until generic tuning primitives can be separated from named inputs and Perl workload policy. |
| `runtime/input_validation/**` | Perl-specific | File-extension, document-scheme, workspace, parser-buffer, and current custom-method admission policy are product decisions. | Colocated path/content/URI/request-bound tests. | Keep at the current app's trust and sync sinks. Generic string/size helpers are too small to justify an extraction seam now. |
| `runtime/launcher/**` | Perl-specific | The launcher consumes the feature catalog/profile, `PERL_LSP_*` logging inputs, product CLI identity, runtime tuning, and current transport defaults. | Colocated CLI, logging, transport, startup, and feature-grid tests. | Keep with the executable/application layer. |
| `runtime/text_utils/mod.rs` | Perl-specific | Statement, `sub`, pragma, `use`, and `require` placement encode Perl source-edit semantics. | Colocated helper coverage and provider/code-action tests. | Keep provider-adjacent in the Perl app. |
| `runtime/mod.rs` | Mixed | It aggregates all runtime classes and re-exports them as one surface. | Entire `perl-lsp-rs-core` unit suite and current editor receipts. | Keep as the app facade; do not treat the directory as one extraction unit. |

## Adjacent LSP-Facing Surfaces

| Surface | Classification | Blocking evidence | Existing proof | Disposition |
| --- | --- | --- | --- | --- |
| `uri/mod.rs` | Mixed | URI/path mechanics are neutral, but invalid input falls back through the product-specific `urn:perl-lsp:unknown` identity and the API owns permissive product fallback policy. | Colocated Unix/Windows path, URI parsing, fallback, and boundary tests. | Candidate only after fallback policy is injected or made strict. It is not part of the first set. |
| `capability_map.rs` | Perl-specific | Maps the product feature catalog and build flags to client capabilities. | Colocated capability-map tests and capability snapshots. | Keep with current feature policy. |
| `providers/**`, `features/**`, `config/**`, `governance/**`, `tooling/**`, `semantic_*`, parser/workspace modules | Perl-specific | These are explicitly retained by the ADR/spec or implement Perl language/provider behavior. | Current provider, feature, configuration, semantic, and integration suites. | Outside this extraction lane. |

## Candidate Sets

### Set A: Ready for PR 3 dependency audit

- `protocol/document_version.rs`
- required external dependencies: `serde_json`
- required standard-library dependencies: `std`
- required Perl dependencies: none found
- required app modules: none found

PR 3 must still prove this by compiling the candidate in isolation or by an
equivalent build-metadata experiment. Static inspection is not dependency
closure proof.

### Set B: Foundational after in-place decoupling

- `JsonRpcId`
- `JsonRpcRequest`
- `JsonRpcResponse`
- `JsonRpcError`
- `JSONRPC_VERSION`
- standards-only JSON-RPC/LSP error codes and generic builders
- standard LSP method constants

Required preparatory separations:

1. move `ErrorClass` implementations and `ErrorCategory` mapping to the current
   app boundary
2. separate standards-only error definitions from provider/product error data
3. separate standard method names from product and test extensions

This set should be preferred over transport work because the transport reader
already depends on the JSON-RPC envelope types.

### Set C: Later transport primitive

- `ContentLengthFramer`
- `FramingError`
- `MAX_FRAME_SIZE`
- `frame`

Required preparatory separations:

1. remove the Perl workspace error taxonomy from the codec file
2. keep `$/perl-lsp/clientResponse` conversion in an app-owned adapter
3. keep product logging and request routing outside the byte-framing primitive
4. prove raw-RPC parity after the move

## Recommended Sequence

1. Land this audit without code movement.
2. Run PR 3 against Set A and record the exact minimal dependency closure.
3. Make one in-place, behavior-preserving JSON-RPC decoupling PR for the Set B
   blockers.
4. Re-run the dependency audit for Set B.
5. Create the crate scaffold only after at least one candidate set compiles
   without a Perl dependency.
6. Move `document_version.rs` first as the lowest-risk whole-file primitive.
7. Move the neutral JSON-RPC set next because it unlocks transport.
8. Split and move the low-level frame codec only after JSON-RPC parity is green.
9. Consider cancellation, budgets, URI handling, schema mechanics, and generic
   registration helpers separately; none is admitted wholesale by this audit.

The deciding criterion is dependency and policy closure at the file boundary,
not how generic a module name sounds.

## Proof Map for Later Changes

| Change class | Minimum existing proof to preserve |
| --- | --- |
| Set A dependency audit or move | `./scripts/cargo-safe test -p perl-lsp-rs-core --profile agent --locked` and isolated candidate compilation |
| JSON-RPC split or move | `perl-lsp-rs-core` unit tests plus JSON-RPC serialization/ID tests |
| Method/error split | `perl-lsp-rs-core` unit tests plus registration tests |
| Capability or registration helper change | `lsp_inline_completion_registration_tests`, `lsp_registration_tests`, and `lsp_cap_snap` |
| Frame codec split or move | framing unit tests, `lsp_registration_tests`, and the raw-RPC receipt |
| Cancellation or tuning split | cancellation unit/property tests, registration tests, and the Neovim lean-startup receipt |
| Current-app integration cleanup | full `perl-lsp-rs` check plus registration/capability tests |

Use the exact commands in the implementation plan for the corresponding PR
stage. Do not delete or weaken current-app proof to make an extraction compile.

## Audit Proof and Limits

Performed at the audited revision:

- inspected `perl-lsp-rs-core` manifest dependencies
- inspected the public module surface in `src/lib.rs`
- inspected the protocol, transport, runtime, and URI candidate source
- checked for an existing static seam audit in the repository
- checked the open-PR surface for an exact `lsp-stack` audit collision

Not performed:

- local compilation or tests; the GitHub connector provides repository mutation
  but no checked-out workspace
- independent candidate compilation
- runtime/editor parity execution
- hosted CI proof for this documentation change

For this PR 2 documentation change, the required repository proof remains:

```bash
git diff --check
```

Rollback is a revert of the audit and implementation-plan link. No runtime
rollback is required because no production behavior changes.
