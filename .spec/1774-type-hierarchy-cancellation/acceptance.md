# Acceptance Criteria: Type Hierarchy Cancellation Support

## §Behavior

| Input | Condition | Expected Result | Acceptance Test |
|-------|-----------|-----------------|-----------------|
| `textDocument/prepareTypeHierarchy` request | Normal processing without cancellation | Returns TypeHierarchyItem array or empty array | test_type_hierarchy_prepare_normal |
| `textDocument/prepareTypeHierarchy` request | `$/cancelRequest` sent with same ID before traversal completes | Returns JSON-RPC error with code -32800 (RequestCancelled) and "Request cancelled" message | test_type_hierarchy_prepare_cancellation |
| `typeHierarchy/supertypes` request | Normal processing without cancellation | Returns TypeHierarchyItem array of parent types or empty array | test_type_hierarchy_supertypes_normal |
| `typeHierarchy/supertypes` request | `$/cancelRequest` sent with same ID during traversal | Returns JSON-RPC error with code -32800 (RequestCancelled) and "Request cancelled" message | test_type_hierarchy_supertypes_cancellation |
| `typeHierarchy/subtypes` request | Normal processing without cancellation | Returns TypeHierarchyItem array of child types or empty array | test_type_hierarchy_subtypes_normal |
| `typeHierarchy/subtypes` request | `$/cancelRequest` sent with same ID during traversal | Returns JSON-RPC error with code -32800 (RequestCancelled) and "Request cancelled" message | test_type_hierarchy_subtypes_cancellation |

## §Hazards

LSP subsystem hazards seeded from SUBSYSTEM_HAZARD_DEFAULTS.md (LSP-1 through LSP-4):

| Hazard Class | Surface | Failure Mode | Mitigation | Test |
|--------------|---------|--------------|-----------|------|
| LSP-1: Provider contract violation | `crates/perl-lsp-rs/src/runtime/dispatch/routing.rs:75-87` (all 3 type hierarchy method routes wrapped with `route_cancellable()`) | Handler called after cancellation mark, returning response that conflicts with cancellation response, or handler not respecting cancellation token | Verify `route_cancellable()` checks `is_cancelled(&typed_id)` before calling handler; handler result is discarded when pre-cancelled | test_type_hierarchy_prepare_cancellation_routing_check |
| LSP-2: Cancellation registry state machine violation | `crates/perl-lsp-rs/src/runtime/dispatch/request_cancellation.rs:46-60` (all 3 methods in `needs_cancellation` match) | Method not registered in cancellation whitelist, `$/cancelRequest` silently ignored, request continues despite user abort | Verify method appears in `needs_cancellation` match arms; verify GLOBAL_CANCELLATION_REGISTRY.register_token() called for these methods | test_type_hierarchy_cancellation_registry_registration |
| LSP-3: Response code mismatch | Handlers return success or error OTHER than RequestCancelled (-32800) after cancellation | Client receives wrong error code, unable to distinguish cancellation from normal error | Verify cancelled_response_with_method() in routing returns exactly -32800; no custom error codes from handler override it | test_type_hierarchy_cancellation_error_code_invariant |
| LSP-4: Concurrent cancellation interference | Multiple type hierarchy requests (prepare, supertypes, subtypes) sent in parallel, each with independent cancellation signals | Cancelling one request incorrectly affects another due to shared state or ID collision in registry | Verify JsonRpcId uniqueness per request; registry uses typed_id (string or number + method) as key, not just ID | test_type_hierarchy_concurrent_cancellation_isolation |

Non-LSP cross-subsystem hazards:

| Hazard Class | Surface | Failure Mode | Mitigation | Test |
|--------------|---------|--------------|-----------|------|
| CROSS-1: Regression in non-cancellable ops | All LSP routing (routing.rs lines 50-200 for non-cancellable methods) | Changes to routing pattern mistakenly wrap operations that should NOT be cancellable (e.g., lifecycle, notifications), breaking those handlers | Verify only type hierarchy routes use route_cancellable(); all other routes unchanged; run full routing test suite | test_regression_non_type_hierarchy_routes_unchanged |
| CROSS-2: Test fixture contamination | lsp_cancellation_protocol_tests.rs test setup/teardown | Tests for type hierarchy cancellation interfere with other cancellation tests due to shared GLOBAL_CANCELLATION_REGISTRY state | Verify each test independently initializes server, sends cancellation signal, cleans up registry; no shared test state | test_type_hierarchy_cancellation_test_isolation |

## §Contracts

**LSP Protocol Contracts** (per [microsoft.github.io/language-server-protocol/3.17](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)):
- `typeHierarchy/prepare` (LSP 3.17 §typeHierarchy): Client sends `textDocument/prepareTypeHierarchy` request or `typeHierarchy/prepare` alias; server responds with `TypeHierarchyItem[]` or null.
- `typeHierarchy/supertypes` (LSP 3.17): Client sends `typeHierarchy/supertypes` with request body containing `item: TypeHierarchyItem`; server responds with `TypeHierarchyItem[]` or null.
- `typeHierarchy/subtypes` (LSP 3.17): Client sends `typeHierarchy/subtypes` with request body containing `item: TypeHierarchyItem`; server responds with `TypeHierarchyItem[]` or null.
- **Cancellation support (LSP 3.17 §cancellation)**: All three methods MUST support `$/cancelRequest` with error code -32800 (RequestCancelled) per LSP cancellation protocol.

**Crate API Contracts** (perl-lsp-rs, perl-lsp-rs-core):
- `register_request_cancellation()` in request_cancellation.rs: If a request method is in the `needs_cancellation` match list, the method is eligible for cancellation token registration via `GLOBAL_CANCELLATION_REGISTRY.register_token()`.
- `route_cancellable()` in routing.rs: If handler is called with a request_id that exists in the registry as cancelled, returns `RoutedResponse::Immediate(cancelled_response_with_method())` before calling the handler. This is the enforcement point — no handler output is used.
- `handle_prepare_type_hierarchy_dispatch()`, `handle_type_hierarchy_supertypes_dispatch()`, `handle_type_hierarchy_subtypes_dispatch()` in text_document.rs: Each returns `Result<Option<Value>, JsonRpcError>` and MUST be safe to skip (no side effects if the Result is discarded after cancellation).

## §API-Shape

**New public API surface:** NONE. All changes are internal dispatch configuration and cancellation routing.

**Modified API surface:**
- `crates/perl-lsp-rs/src/runtime/dispatch/request_cancellation.rs`: `register_request_cancellation()` function behavior changes — adds 3 method names to the `needs_cancellation` match.
- `crates/perl-lsp-rs/src/runtime/dispatch/routing.rs`: `route_request()` method behavior changes — 3 additional `route_cancellable()` wraps (no signature change).

**Affected ID spaces:** None (all request IDs are existing, no new namespaces).

**Dup-risk grep queries** (verify no hidden callers):
- `grep -rn "typeHierarchy/prepare" crates/perl-lsp-rs/src/` — should find only routing.rs and request_cancellation.rs entries. Currently 3 hits in routing.rs; after change, should add 1 hit in request_cancellation.rs.
- `grep -rn "typeHierarchy/supertypes\|typeHierarchy/subtypes" crates/perl-lsp-rs/src/` — should find only routing.rs entries. Currently 2 hits in routing.rs; after change, should add 2 hits in request_cancellation.rs.
- `grep -rn "handle_prepare_type_hierarchy_dispatch\|handle_type_hierarchy_supertypes_dispatch\|handle_type_hierarchy_subtypes_dispatch" crates/perl-lsp-rs/src/` — should find routing.rs (before wrapping) and text_document.rs (definition). No new callers expected.

**Caller count (estimated impact):**
- `handle_prepare_type_hierarchy_dispatch()`: 2 callers in routing.rs (both wrapped).
- `handle_type_hierarchy_supertypes_dispatch()`: 1 caller in routing.rs (wrapped).
- `handle_type_hierarchy_subtypes_dispatch()`: 1 caller in routing.rs (wrapped).
- No external callers (both .rs modules are internal to perl-lsp-rs crate).

## §Test-Grid

| Acceptance Row | Test Name | Test Type | Invariant | Location |
|---|---|---|---|---|
| Type hierarchy prepare normal | test_type_hierarchy_prepare_normal | Positive | prepare() without cancellation returns items or empty array, response code is 200 | lsp_cancellation_protocol_tests.rs |
| Type hierarchy prepare cancellation | test_type_hierarchy_prepare_cancellation | Negative (cancellation) | prepare() with $/cancelRequest returns -32800, no items | lsp_cancellation_protocol_tests.rs |
| Type hierarchy supertypes normal | test_type_hierarchy_supertypes_normal | Positive | supertypes() without cancellation returns items or empty array, response code is 200 | lsp_cancellation_protocol_tests.rs |
| Type hierarchy supertypes cancellation | test_type_hierarchy_supertypes_cancellation | Negative (cancellation) | supertypes() with $/cancelRequest returns -32800, no items | lsp_cancellation_protocol_tests.rs |
| Type hierarchy subtypes normal | test_type_hierarchy_subtypes_normal | Positive | subtypes() without cancellation returns items or empty array, response code is 200 | lsp_cancellation_protocol_tests.rs |
| Type hierarchy subtypes cancellation | test_type_hierarchy_subtypes_cancellation | Negative (cancellation) | subtypes() with $/cancelRequest returns -32800, no items | lsp_cancellation_protocol_tests.rs |
| Type hierarchy concurrent isolation | test_type_hierarchy_concurrent_cancellation_isolation | Adversarial | Multiple type hierarchy requests with independent cancellation signals do not interfere | lsp_cancellation_protocol_tests.rs |
| Registry registration | test_type_hierarchy_cancellation_registry_registration | State-transition | After registering prepare/supertypes/subtypes methods, GLOBAL_CANCELLATION_REGISTRY has tokens for all three | lsp_cancellation_protocol_tests.rs |
| Error code invariant | test_type_hierarchy_cancellation_error_code_invariant | Invariant | Cancelled response is ALWAYS -32800, never -32700 or success code | lsp_cancellation_protocol_tests.rs |
| Routing check | test_type_hierarchy_prepare_cancellation_routing_check | State-transition | route_cancellable() checks is_cancelled() BEFORE calling handler; handler output ignored when pre-cancelled | lsp_cancellation_protocol_tests.rs |
| Non-type-hierarchy regression | test_regression_non_type_hierarchy_routes_unchanged | Regression | All non-type-hierarchy routes (lines 50-74, 88-200) unchanged; no new route_cancellable wraps | routing_tests.rs or routing.rs integration tests |
| Test isolation | test_type_hierarchy_cancellation_test_isolation | Infrastructure | Each type hierarchy cancellation test cleans up registry; no shared state between tests | lsp_cancellation_protocol_tests.rs |

## §Blast-Radius

**Consumers (other crates, other tests, client code):**
- **No new LSP protocol features:** Type hierarchy operations are already part of LSP; this change only adds cancellation support (does not change method signatures, request/response bodies, or capabilities).
- **No new public APIs:** Changes are internal to perl-lsp-rs dispatch layer.
- **Type hierarchy provider (crates/perl-lsp-rs-core):** The three handler methods (`handle_prepare_type_hierarchy`, `handle_type_hierarchy_supertypes`, `handle_type_hierarchy_subtypes`) continue to work unchanged. They receive `params` and return `Result<Option<Value>, JsonRpcError>` as before. The only difference is that if a request is cancelled, the routing layer discards the result — no change to the provider's contract.
- **LSP client (VSCode extension vscode-extension/):** No change to LSP protocol surface; VSCode client's existing `$/cancelRequest` support continues to work as before.
- **Other LSP operations:** No impact on other operations. The changes to request_cancellation.rs and routing.rs are pure additions (new match arms, not modifications to existing arms). The callHierarchy pattern is proven; type hierarchy follows the same pattern.

**Downstream crates (must not touch):**
- `crates/perl-parser*` — No changes.
- `crates/perl-semantic-*` — No changes.
- `crates/perl-dap/` — No changes.
- `crates/perl-workspace/` — No changes.
- All other crates — No changes.

**Boundary conditions (internal LSP routing only):**
- If request_cancellation.rs is not updated to add the 3 methods, routing.rs wraps are "dead" (cancellation tokens registered but no check). Red-TDD builder should catch this in tests.
- If routing.rs is not updated to use route_cancellable, cancellation tokens are registered but ignored (requests still complete normally). Tests should verify this does NOT happen.
- If tests are not added, the feature has no validation; existing behavior is preserved but not tested.

**Breaking changes:** NONE. All changes are additive (new match arms, new route_cancellable wraps for already-routed methods).

**Deprecation:** NONE. Type hierarchy methods are LSP 3.17 standard; no deprecation is involved.

