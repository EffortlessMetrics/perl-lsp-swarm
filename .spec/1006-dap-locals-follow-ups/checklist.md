# Implementation Checklist: #1006 — DAP Locals scope: follow-ups from #997 deep review

## Summary

Three non-blocking follow-up enhancements to the #997 Locals scope fix:

1. **Multi-frame Locals**: Support requesting Locals for any stack frame (not just current), respecting the `frameId` parameter.
2. **Array/hash lexical rendering**: Render `@arr` and `%hash` lexicals with proper array/hash representations instead of scalar 0.
3. **Fallback hardening**: Return empty Locals (not fake `$self`/`@_`) when B module is unavailable, preventing regression to #997 bug.

All three changes are localized to DAP native adapter (perl-dap crate). No LSP or bridge changes. No new public API surface.

---

## Change order (compiles at each step)

### Step 1: Add multi-frame Locals support to B-module Perl code
- **File:** `crates/perl-dap/src/debug_adapter/variables.rs` lines 145-163
- **Change:** Extend the B-module concat! string to accept and use the requested frame_id to index into the stack
- **Details:**
  - Decode `scope_frame_id` at line 116 (already done; verify it's used now)
  - Modify the Perl eval to take frame_id as a parameter (or compute it from a caller stack count)
  - The Perl code currently uses `$DB::sub` (the current frame); instead, index `@va` by a depth derived from the requested `frame_id`
  - The stack_ref module likely provides a method to map frame_id to stack depth; investigate `crates/perl-dap-stack` for frame_id semantics
  - Approach: add a parameter to the concat! string (e.g., add a Perl variable set at the start: `my $frame_id=<VALUE>;` where <VALUE> comes from scope_frame_id), then use it to select the correct pad from `@va`
  - **Constraint**: The concat! string must remain a compile-time constant; no runtime interpolation. Use Rust side to compute the frame_id, pass it as a literal in the concat!
  - **Actually**: The frame_id comes from the DAP client request, embedded in variablesReference, and decoded at line 116. The challenge is that concat! is compile-time. **Solution**: Modify the Perl eval to COMPUTE the frame depth from the current stack, then index into that. OR: use Rust to build the command string dynamically (not concat!) with the frame_id included. The dynamic approach trades compile-time safety for runtime flexibility. Since frame_id is client-supplied, runtime is acceptable if validated. **Recommendation**: Build the command string dynamically at line 145, including frame_id as a Perl variable assignment.
- **Verify:** `cargo check -p perl-dap`

### Step 2: Implement array/hash lexical value extraction
- **File:** `crates/perl-dap/src/debug_adapter/variables.rs` lines 145-163
- **Change:** Modify the Perl eval to detect B::AV and B::HV objects and format their values as lists/maps
- **Details:**
  - The current line 160 chain: `my $v=eval{$s->SV->PV}//eval{$s->SV->IV}//eval{$s->IV}//eval{$s->PV}//'undef'`
  - For B::AV (array reference): use `$s->ARRAY` to get the SV array and format as `[elem1, elem2, ...]`
  - For B::HV (hash reference): use `$s->HASH` to get the SV hash and format as `{key1=>val1, key2=>val2, ...}`
  - For B::SV (scalar): keep the existing chain as fallback
  - The Perl code must detect the ref type (via ref($s) =~ /B::AV|B::HV/) and branch appropriately
  - String building approach: build the output string with Perl array/hash syntax inline (quoted, escaped properly for newline handling)
  - **Note**: This is a refinement to the existing concat! Perl eval; it's still compile-time constant (no user input involved)
- **Verify:** `cargo check -p perl-dap`

### Step 3: Modify fallback_scope_variables to return empty Locals (not fake vars)
- **File:** `crates/perl-dap/src/debug_adapter/parsing.rs` lines 234-282
- **Change:** Update the ScopeKind::Locals arm to return an empty Vec instead of fake `$self` and `@_`
- **Details:**
  - Current: lines 243-260 return a Vec with two Variable entries (fake $self and @_)
  - New: change ScopeKind::Locals branch to return `Vec::new()` (empty)
  - Keep Package (lines 261-268) and Globals (lines 269-276) fallback placeholders unchanged (they are less likely to reintroduce bugs, and the issue only mentions Locals fallback hardening)
- **Verify:** `cargo check -p perl-dap`

### Step 4: Update fallback test expectations
- **File:** `crates/perl-dap/tests/dap_scope_filtering_tests.rs`
- **Change:** Update test expectations for the Locals fallback (now empty instead of fake vars)
- **Details:**
  - Search for tests that verify fallback Locals scope variables (e.g., `test_fallback_locals_scope_has_variables` or similar)
  - Update assertions to expect empty variable list instead of `$self` and `@_`
  - Verify Package and Globals fallbacks still work as before
  - Run: `cargo test -p perl-dap -- --test-threads=1` to ensure tests pass
- **Verify:** `cargo test -p perl-dap --lib` (no e2e needed yet)

### Step 5: Add red-TDD tests for the three features (writer responsibility)
- **File:** `crates/perl-dap/tests/dap_scope_filtering_tests.rs` (or a new test module)
- **Change:** Red-TDD builder adds failing tests for each follow-up (this is a red-TDD step, not spec-planner)
- **Details:** (For reference; red-TDD builder will handle)
  - `test_multi_frame_locals_frame_0_vs_1`: Create a stack, request Locals for frame 0 and frame 1, assert different variable sets
  - `test_locals_array_rendering_happy`: Create a breakpoint with `@arr = (1,2,3)`, assert Locals contains array-formatted value
  - `test_locals_hash_rendering_happy`: Create a breakpoint with `%hash = (a=>1)`, assert Locals contains hash-formatted value
  - `test_locals_b_unavailable_returns_empty`: Mock B module unavailability, assert Locals fallback is empty (not fake)
  - Plus adversarial tests from acceptance.md §Test-Grid
- **Verify:** Tests fail with current code (red), then pass after implementation (green)

### Step 6: Implement multi-frame support (logic refinement)
- **File:** `crates/perl-dap/src/debug_adapter/variables.rs` lines 120-178
- **Change:** Complete the multi-frame Locals implementation by using scope_frame_id to index the correct frame's pad
- **Details:**
  - At line 116, scope_frame_id is already decoded. Verify it's not zero (or handle zero as current frame).
  - Build the Perl eval command dynamically (not via concat!) to include frame_id as a parameter
  - The Perl code must map frame_id to a depth in the current call stack and select the correct pad
  - **Challenge**: The Perl debugger's @va (PADLIST array) is indexed 0=protpad, 1..N=invocation frames (newest to oldest). The frame_id from DAP is numbered 0=innermost (current), 1=caller, 2=caller's caller. Map frame_id to @va index as: `@va_index = @va - 1 - frame_id` (if frame_id < @va size; else out of bounds).
  - **Actually simpler**: The current code uses `$va[-1]` (innermost). For multi-frame, use `$va[-1-frame_id]` to access frame_id steps up the stack. Verify bounds.
  - Return honest empty if frame_id is out of bounds (no panic).
- **Verify:** `cargo check -p perl-dap && cargo test -p perl-dap --lib`

### Step 7: Array/hash rendering refinement (implementation)
- **File:** `crates/perl-dap/src/debug_adapter/variables.rs` lines 145-163
- **Change:** Implement the array/hash value extraction logic in the Perl eval string
- **Details:**
  - Detect B::AV via `ref($s) eq 'B::AV'`; extract array elements via `$s->ARRAY` and format as comma-separated list in brackets
  - Detect B::HV via `ref($s) eq 'B::HV'`; extract hash pairs via `$s->HASH` and format as key=>value pairs in braces
  - Fall back to the existing scalar chain for other types
  - Format examples:
    - `@arr = (1, 2, 3)` → `$pv = [1, 2, 3]` (or similar list notation)
    - `%hash = (a => 1, b => 2)` → `$pv = {a=>1, b=>2}` (or similar map notation)
  - The format must be parseable by `parse_scope_variables_from_lines` (which expects `name = value` lines); test with snapshots.
- **Verify:** `cargo test -p perl-dap --lib`

### Step 8: Integration and e2e testing
- **File:** `crates/perl-dap/tests/dap_scope_filtering_tests.rs`
- **Change:** Run full e2e test suite; update snapshots if array/hash rendering changes output format
- **Details:**
  - Run: `cargo test -p perl-dap` (includes e2e tests if Perl is available)
  - Verify snapshot tests (`dap_scope_filtering_tests.rs::test_e2e_*`) still pass or update snapshots as needed
  - Verify new red-TDD tests now pass (green)
  - No regressions in Package and Globals scope tests
- **Verify:** `cargo test -p perl-dap`

### Step 9: Final verification
- **Verify:** 
  - `cargo test -p perl-dap` — all tests pass
  - `cargo xtask fmt` — formatting check
  - `cargo clippy -p perl-dap` — no warnings
  - `cargo check --workspace` — no transitive breaks

---

## Callers and consumers

- `handle_variables` is called from the DAP message dispatcher in `debug_adapter.rs`
- `fallback_scope_variables` is called from `handle_variables` at line 251 when framed output is empty
- Variables scope test files: `crates/perl-dap/tests/dap_scope_filtering_tests.rs` (19 tests)

---

## Scope boundary

**Files IN scope:**
- `crates/perl-dap/src/debug_adapter/variables.rs` (Locals scope path, Perl eval command, fallback call)
- `crates/perl-dap/src/debug_adapter/parsing.rs` (fallback_scope_variables function)
- `crates/perl-dap/tests/dap_scope_filtering_tests.rs` (test expectations update + new red-TDD tests)

**Files OUT of scope:**
- `crates/perl-dap/src/debug_adapter/var_ref.rs` (VariableReference codec — no changes)
- `crates/perl-dap-stack/` (frame_id semantics already defined; no changes)
- `crates/perl-lsp-rs/` (DAP bridge unaffected)
- Any parser, lexer, or non-DAP crate

---

## Flags for builder

1. **Frame-id to stack-depth mapping**: The exact formula for mapping DAP frame_id to Perl @va index may need adjustment based on the stack_ref module's semantics. Current assumption: `$va_index = @va - 1 - frame_id`. Verify with a multi-frame e2e test.

2. **Dynamic Perl command building**: The current code uses compile-time `concat!` for the B-module eval. Multi-frame support requires embedding the frame_id parameter. Since frame_id comes from the DAP client (already validated at line 57-67), building the command dynamically (using `format!`) is acceptable. This trades compile-time safety for runtime flexibility. Confirm with code review if static concat! is a hard requirement.

3. **Array/hash rendering format**: The exact format for array/hash output (e.g., `[1, 2, 3]` vs `(1, 2, 3)` vs Perl list syntax) should match what the variable parser expects and what debugger clients understand. E2E tests and snapshot verification will guide this. Start with simple formats; refine if tests fail.

4. **Fallback hardening decision**: The issue states to return empty Locals when B is unavailable. This is a safety choice (prefer honest empty over fake vars). Confirm with maintainer that this doesn't break existing fallback-based workflows (though the issue strongly suggests it does not).

5. **Test snapshot updates**: Array/hash rendering changes may require snapshot updates in e2e tests. The builder should run tests and verify diffs before committing.

---

## Dependency chain

- Step 1 (multi-frame Perl code) is independent but requires understanding frame_id semantics
- Step 2 (array/hash rendering) is independent of step 1 (both modify the same concat! string)
- Step 3 (fallback hardening) is independent of steps 1-2
- Steps 4-5 (test updates) depend on steps 1-3 being mostly complete
- Step 6 (multi-frame implementation) refines step 1
- Step 7 (array/hash implementation) refines step 2
- Steps 1-2 and 3 should be implemented together (same commit or closely timed)

---

## Notes for red-TDD builder

When writing red tests, use the existing test patterns in `dap_scope_filtering_tests.rs`:

- **E2E fixture approach**: Create a Perl script with lexicals (@arr, %hash, $scalar), hit a breakpoint, and request Locals. Assert variable names and values.
- **Fallback test approach**: Create an adapter without a live session, request Locals (or mock B unavailability), assert empty list.
- **Multi-frame approach**: Create a recursive or multi-call fixture, hit a breakpoint in a nested frame, request Locals for multiple frame_ids, assert each frame returns its own variables.

Examples in the test file to reference:
- `test_e2e_locals_scope_returns_user_lexicals_not_db_internals` — existing positive e2e test
- `test_fallback_scope_variables_locals_returns_placeholder_self_and_underscore` — fallback test (expectations will change)
