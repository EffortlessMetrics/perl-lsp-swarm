# Context: #1847 — Fix consecutive didChange auto-increment producing same version number

## Problem Statement

When multiple consecutive `textDocument/didChange` LSP notifications arrive in rapid succession **without explicit version numbers**, the auto-increment fallback logic can produce duplicate version numbers instead of monotonically increasing ones.

This occurs because:
1. `handle_did_change_with_cancellation` fetches the current document state once (line 429)
2. Uses that snapshot to compute the next version (line 449-450): `doc_state.version.saturating_add(1)`
3. If a second didChange arrives before the first completes and updates the stored document, both use the same stale `doc_state.version`
4. Result: duplicate version numbers (e.g., both produce version 5)

### Affected Code Path
- **File:** `crates/perl-lsp-rs/src/runtime/text_sync.rs`
- **Function:** `handle_did_change_with_cancellation` (lines 369-881)
- **Bug location:** Lines 449-450 (version auto-increment uses stale snapshot)

### Triggering Conditions
- Client sends didChange WITHOUT explicit `version` field (non-LSP-compliant but possible)
- Multiple consecutive notifications arrive before the first completes
- No explicit version field means fallback to auto-increment is used

### Impact
- Version tracking becomes unreliable
- Potential state corruption if version is used as a cache key or sequence number
- Violates LSP protocol invariant that version numbers are monotonically increasing

## Why This Bug Exists

The original code (line 449-450) assumes `doc_state` is current. However, in a concurrent/async environment:
1. Document state is stored in a `Mutex<HashMap>` (line 395 in perl-lsp-rs/src/lib.rs)
2. After the version is computed and the doc is updated, the state is re-inserted (line 801)
3. If a second didChange arrives between the fetch (line 429) and the insert (line 801), it sees a stale version

The fix is simple: re-fetch the current version from the map immediately before computing the next version.

## Solution Approach

**Key insight:** The `documents` map is held under exclusive lock throughout the handler (line 395 through line 804). We can safely query it at any point for the latest version.

**Implementation:** Add a single line before version computation to fetch the current version from the map:

```rust
// At line 449, before let version = ...

// Re-fetch current document state to ensure version auto-increment
// is based on latest version, not stale snapshot from line 429
let current_version = documents
    .get(&normalized_uri)
    .or_else(|| documents.get(uri))
    .map(|d| d.version)
    .unwrap_or(doc_state.version);

let version =
    incoming_version.unwrap_or_else(|| current_version.saturating_add(1));
```

**Why this works:**
1. Lock is held throughout (line 395 → 804)
2. Fetching current version from map is safe and atomic (under lock)
3. Version auto-increment is now based on current state, not stale snapshot
4. No change to public API or external contracts

## Verification Strategy

### Test Coverage
1. **Existing test:** `test_rapid_did_change_resolves_to_latest` (lsp_generation_counter_tests.rs:16) — uses explicit versions
2. **New test:** `test_consecutive_didchange_without_version_increments_uniquely` — sends 3+ didChange with omitted version fields
   - Open document v1
   - Send didChange (no version) → should become v2
   - Send didChange (no version) → should become v3
   - Send didChange (no version) → should become v4
   - Verify via hover/symbol request that document is in valid state
   - Assert no errors

### Regression Prevention
- Run full `perl-lsp-rs` test suite
- Check existing generation counter tests (`test_rapid_did_change_resolves_to_latest`, `test_incremental_edits_no_state_corruption`)
- Lint via `cargo clippy -p perl-lsp-rs --tests`

## Alternatives Considered

1. **Option 1: Use a global version counter** — Would require adding a per-URI atomic counter; more complex and unnecessary given the lock is held.
2. **Option 2: Require explicit version in all clients** — Not practical; must support non-compliant clients per LSP robustness guidelines.
3. **Option 3: Track version outside DocumentState** — Adds complexity; version is logically part of document state and belongs there.
4. **Selected: Re-fetch from map before computing version** — Simplest, safest, leverages existing lock, no new fields or global state.

## References

- **LSP Spec § 3.8.5** (textDocument/didChange): "The version number of this change (if in full document mode with send full document mode)."
- **Related commits:**
  - `6d8db8295` — "fix(lsp): harden didChange version handling (#4950)"
  - `08c7546a8` — "fix(lsp): correct incremental didChange range mapping across multi-edits (#2080) (#4999)"
- **Related test file:** `crates/perl-lsp-rs/tests/lsp_generation_counter_tests.rs` — existing generation counter race prevention tests
- **Code artifact:** `crates/perl-lsp-rs/src/runtime/text_sync/document_state.rs` — DocumentState struct definition with version and generation fields

## Decision Log

- **Why not use generation counter for version?** Generation counter (`u32 AtomicU32`) is for detecting stale parse results; version (`i32`) is LSP-visible and must match client expectations. They serve different purposes.
- **Why re-fetch from map instead of modifying data structures?** Adding a global atomic counter or per-URI version tracker would be over-engineered. The lock already ensures exclusive access; we just need to use it correctly.
- **Why this location (immediately before computing version)?** Because that's when we need the latest version; fetching earlier would create the same stale-snapshot problem.

## Questions for Builder / Red-TDD

1. Can the test harness support sending didChange notifications with omitted `version` fields? (May need to bypass JSON schema validation in the harness.)
2. Should the test verify version directly (e.g., via `get_document()` API) or indirectly (via document-dependent requests like hover)?
3. Are there any performance implications of re-fetching from the map? (Should be negligible since it's a HashMap lookup, not I/O.)
