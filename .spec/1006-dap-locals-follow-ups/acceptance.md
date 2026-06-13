# Acceptance Criteria: #1006 — DAP Locals scope: follow-ups from #997 deep review

## §Behavior

Tabular summary of the three follow-up enhancements to the Locals scope implementation.

| Input / Condition | Expected Result | Notes |
|---|---|---|
| Request Locals for a non-current stack frame (e.g. frame_id=2 in a 3-frame stack) | Return lexical variables for that frame's pad, not the current (stopped) frame | Multi-frame support; requires walking PADLIST at the requested depth |
| Array lexical (`@arr = (1, 2, 3)`) appears in Locals scope | Render as array-like representation (e.g. `[1, 2, 3]` or similar) instead of scalar 0 | Fix the B::AV / B::HV value chain to use `->ARRAY` / `->HASH` instead of `->SV->PV` |
| Hash lexical (`%hash = (a=>1, b=>2)`) appears in Locals scope | Render as hash-like representation (e.g. `{a=>1, b=>2}` or similar) instead of scalar 0 | Ditto for B::HV |
| B module unavailable in debugger, requesting Locals scope (scope type 1) | Return empty variables list (honest), not fake `$self` / `@_` placeholders | Fallback hardening: no longer reintroduce the original bug in degraded environments |
| B module available, requesting Locals at current stopped frame | Return same lexical variables as before (non-regression) | The fix for #997 (using B::main_cv() + PADLIST) remains the default path |

All tests pass: `cargo test -p perl-dap`
No clippy warnings: `cargo clippy -p perl-dap`
Formatted: `cargo xtask fmt`

## §Hazards

Hazard invariants applicable to this DAP subsystem change. Rows copied from [SUBSYSTEM_HAZARD_DEFAULTS.md](../reference/SUBSYSTEM_HAZARD_DEFAULTS.md).

| Class | Invariant | Surface (specific file/fn this change touches) | Required adversarial test |
|---|---|---|---|
| **DAP-1: ID / ref-space collision** | All numeric reference spaces (variablesReference, frameId, scope IDs) are provably disjoint. No two allocators share an untyped integer range without a named constant boundary. | `variables.rs:120-178` (Locals scope path decodes scope_frame_id at line 116; multi-frame support uses it to index into stack frame list) | `test_multi_frame_locals_uses_correct_frame_id` — allocate refs for frame_id=1 and frame_id=2, request variables for both, assert each returns its own frame's variables (not cross-contamination) |
| **DAP-2: Bounds / overflow on client-supplied IDs** | All `frameId`, `variablesReference`, `threadId` values originating from DAP client are validated before array subscript or arithmetic. Out-of-range → honest ErrorResponse, never panic or wrap. | `variables.rs:115-118` (decode scope_frame_id from variablesReference); follow-up will index stack with this ID | `test_locals_frame_id_oob` — request Locals with frame_id > max-possible-depth, assert honest empty (no panic) |
| **DAP-3: Document lifecycle safety** | Debugger state queried (pad walk, B module introspection) only when session is DebugState::Stopped; running/exiting → return empty with success=true. | `variables.rs:78-92` (stale-ref guard); follows through to Locals eval at 144-177 | `test_locals_while_running` — mark session as Running, request Locals, assert empty (no query to debugger) |
| **DAP-4: Scanner/injection safety** | All framed eval commands use concat! (compile-time constants), never runtime string interpolation of user input. | `variables.rs:145-163` (the B-module concat! string remains unchanged in structure; multi-frame support derives frame index from internal stack, not client input) | `test_locals_no_injection` — no new injection surface; existing test `test_e2e_locals_scope_returns_user_lexicals_not_db_internals` continues to verify |
| **DAP-5: Cache invalidation** | When session state changes (pause→continue, frame change), variable_cache is cleared. Stale Locals refs from prior stop are rejected. | `variables.rs:74-92` (stale-ref guard checks DebugState); follows through to Locals path | `test_locals_stale_ref_after_continue` — stop, request Locals (caches ref), continue, request Locals again with same ref, assert honest empty (cache invalidated) |
| **DAP-6: Error recovery (B unavailable)** | If B module cannot be loaded, handle gracefully: emit no panic, return honest empty instead of fake placeholders. | `variables.rs:144-177` (outer eval absorbs B::require errors; fallback_scope_variables at 251 consulted if framed_scope_lines is empty) | `test_locals_b_unavailable_returns_empty` — inject eval failure (simulate B load failure), assert empty Locals (not fake $self/@_) |

**Subsystem-specific defaults consulted**: [SUBSYSTEM_HAZARD_DEFAULTS.md — DAP section](../reference/SUBSYSTEM_HAZARD_DEFAULTS.md#dap-subsystem)

## §Contracts

Which contracts from protocol specs or this codebase this change touches.

| Contract | Source document + section | How this change satisfies or extends it |
|---|---|---|
| DAP Variables Request scope filtering | [DAP_IMPLEMENTATION_SPECIFICATION.md §Variables](../reference/DAP_IMPLEMENTATION_SPECIFICATION.md) | #997 introduced scope-filtered Locals via B::PADLIST. This follow-up extends it to multi-frame (frame_id-aware) and improves array/hash rendering to match the spec's expected variable-type display |
| LSP/DAP Type rendering | Protocol expectation: arrays render as arrays, hashes as hashes | Change improves the value display by using B::AV->ARRAY and B::HV->HASH instead of scalar coercion |
| Error handling: Fallback → empty, not fake | DAP protocol safety: unknown/unavailable scope must return empty, not invented placeholders | Fallback hardening removes the reintroduction of the #997 bug in B-unavailable cases |

## §API-Shape

New public types, functions, enum variants, or ID-spaces introduced by this change.

| Item | Kind | Signature / Range | Dup-risk (grep result) | Caller count |
|---|---|---|---|---|
| N/A — no new public API | | | | |

This change modifies the internal logic of `handle_variables` (private method) and `fallback_scope_variables` (private helper) without introducing new public surface. The B-module Perl code in the concat! string changes structurally to support frame-id-indexed PADLIST access, but this is internal to the debugger transport.

## §Test-Grid

Enumeration of test cases covering axes of variation. Red-TDD builder writes failing versions before any implementation.

| Scenario | Kind | Test name | Invariant discharged |
|---|---|---|---|
| Multi-frame request: frame 0 vs frame 1 | positive | `test_multi_frame_locals_frame_0_vs_1` | Different frames return different locals (frame_id correctly indexes the stack) |
| Multi-frame request: frame_id out of bounds | negative | `test_multi_frame_locals_frame_oob` | Out-of-range frame_id returns honest empty, not panic |
| Array lexical rendering | positive | `test_locals_array_rendering_happy` | `@arr = (1,2,3)` renders as array type, not scalar 0 |
| Hash lexical rendering | positive | `test_locals_hash_rendering_happy` | `%hash = (a=>1)` renders as hash type, not scalar 0 |
| Scalar lexical (non-regression) | positive | `test_locals_scalar_still_renders_happy` | `$x = 42` still renders correctly (no regression from array/hash fix) |
| Array with empty content | edge | `test_locals_array_empty_rendering` | `@arr = ()` renders as empty array, not scalar 0 |
| Hash with empty content | edge | `test_locals_hash_empty_rendering` | `%hash = ()` renders as empty hash, not scalar 0 |
| B module unavailable | negative | `test_locals_b_unavailable_returns_empty` | Fallback returns empty Locals, not fake `$self`/`@_` |
| B module unavailable, Package scope (non-regression) | negative | `test_locals_b_unavailable_package_still_works` | Package scope unaffected by B unavailability (still uses V command) |
| Nested call stack (frame_id=2 in recursive context) | adversarial | `test_multi_frame_locals_recursive_frame_id` | Each frame's Locals shows correct lexicals; innermost frame doesn't leak outer-frame variables |
| Stale ref after debugger resume | state | `test_locals_stale_ref_after_continue` | Session resume clears cache; stale ref from prior stop returns empty |

## §Blast-Radius

Subsystems and crates that consume the Locals scope path.

| Consumer | Crate | Dependency type | Impact | Required update |
|---|---|---|---|---|
| `handle_variables` request handler | `perl-dap` | internal method | None — signature unchanged; only internal logic and fallback behavior change | None |
| Variables response serialization | `perl-dap` | protocol output | None — Variable struct shape unchanged | None |
| E2E workflow tests | `perl-dap` tests | test fixture | Snapshot updates may be required if array/hash rendering changes visual format | TBD — builder verifies snapshot diffs |
| Fallback variable tests | `dap_scope_filtering_tests.rs` | test suite | Fallback for Locals scope no longer emits fake $self/@_; other scope fallbacks (Package, Globals) unchanged | Update test expectations for Locals fallback (now empty) |
| DAP client (debuggers) | external | protocol consumer | Improvement: Locals now includes all reachable frames, not just current frame; arrays/hashes render correctly | No changes required; clients receive better variable display |

Must-not-touch boundary:
- `crates/perl-dap/src/debug_adapter/parsing.rs:parse_scope_variables_from_lines()` — no changes to the scope filtering regex or variable name/value parsing
- `crates/perl-dap/src/debug_adapter/var_ref.rs` — no changes to the VariableReference codec or scope-ref encoding formula
- Package and Globals scope paths — no changes to the `V <frame_id> ::` and `V <frame_id> *` commands
- `crates/perl-lsp-rs/` — DAP bridge unaffected; this is native adapter internal logic

## §Coverage-Map

Coverage details for new code paths:

| New code path | Covered by | Test file |
|---|---|---|
| `variables.rs:120-178` — multi-frame frame_id lookup into stack | `test_multi_frame_locals_frame_0_vs_1` | `dap_scope_filtering_tests.rs` (e2e) |
| `variables.rs:120-178` — array B::AV rendering logic | `test_locals_array_rendering_happy` | `dap_scope_filtering_tests.rs` (e2e) |
| `variables.rs:120-178` — hash B::HV rendering logic | `test_locals_hash_rendering_happy` | `dap_scope_filtering_tests.rs` (e2e) |
| `parsing.rs:234-282` — fallback Locals returns empty | `test_locals_b_unavailable_returns_empty` | `dap_scope_filtering_tests.rs` (negative) |
| Error arm: B require fails, framed_scope_lines empty → fallback | `test_locals_b_unavailable_returns_empty` | `dap_scope_filtering_tests.rs` (negative) |
