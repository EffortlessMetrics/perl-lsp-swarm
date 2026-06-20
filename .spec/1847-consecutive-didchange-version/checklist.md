# Implementation Checklist: #1847 — Fix consecutive didChange auto-increment producing same version number

## Problem
When consecutive `textDocument/didChange` notifications arrive without explicit version numbers, the auto-increment logic can produce duplicate version numbers instead of monotonically increasing ones. This occurs because the auto-increment fallback uses `doc_state.version.saturating_add(1)` where `doc_state` is fetched once per notification, but multiple rapid calls use a stale snapshot.

## Solution
Ensure that when `didChange` lacks an explicit version number, the new version is always derived from the **current stored document state** at the moment of computation, not a stale snapshot captured earlier in the handler.

## Ordered Implementation Steps

### Step 1: Write failing tests
**File:** `crates/perl-lsp-rs/tests/lsp_generation_counter_tests.rs`
**Action:** Add new test `test_consecutive_didchange_without_version_increments_uniquely` at end of file
- Open a document with version 1
- Send 3 consecutive didChange notifications WITHOUT explicit version numbers (omit the "version" field)
- Verify that stored document versions are 2, 3, 4 (strictly increasing)
- Assert by calling `send_request` with a hover/symbol/diagnostic that requires document state
**Test pattern:** 
  - didChange 1: omit version field (should auto-increment to v2)
  - didChange 2: omit version field (should auto-increment to v3)
  - didChange 3: omit version field (should auto-increment to v4)
  - Call hover/symbol request to trigger a state-dependent operation
  - Assert no error (would occur if version tracking is broken)
**Verify command:** `RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs test_consecutive_didchange_without_version_increments_uniquely -- --test-threads=2 --nocapture`
**Expected result:** FAIL (test will fail until step 2 fixes the code)

### Step 2: Fix auto-increment logic in handle_did_change_with_cancellation
**File:** `crates/perl-lsp-rs/src/runtime/text_sync.rs`
**Location:** `handle_did_change_with_cancellation` function, around lines 429-462
**What changes:**
  - Current code fetches `doc_state` at line 429 and computes version at line 449-450 from that stale snapshot
  - New code must fetch the CURRENT document state from the map immediately before computing the version
  - This ensures version auto-increment is based on the latest persisted version, not a stale snapshot

**Detailed change:**
  1. At line 449-450, BEFORE computing `version`, add a line that re-fetches the latest document state:
     ```rust
     // Re-fetch current document state to ensure version auto-increment is based on latest version
     let current_version = documents
         .get(&normalized_uri)
         .or_else(|| documents.get(uri))
         .map(|d| d.version)
         .unwrap_or(doc_state.version);
     
     let version =
         incoming_version.unwrap_or_else(|| current_version.saturating_add(1));
     ```
  2. This one-liner change ensures the next auto-incremented version is always based on what's actually stored

**Dependency:** Step 1 test exists and fails
**Verify command:** `RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs test_consecutive_didchange_without_version_increments_uniquely -- --test-threads=2 --nocapture`
**Expected result:** PASS (test should now pass)

### Step 3: Verify existing tests still pass
**Command:** `RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs lsp_generation_counter_tests -- --test-threads=2`
**Verify:** Both `test_rapid_did_change_resolves_to_latest` and `test_incremental_edits_no_state_corruption` still pass
**Dependency:** Step 2 must be complete

### Step 4: Run full LSP test suite
**Command:** `RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs -- --test-threads=2 --lib`
**Verify:** All tests pass, no failures
**Dependency:** Step 3 must pass

### Step 5: Lint check
**Command:** `cargo clippy -p perl-lsp-rs --tests`
**Verify:** No new clippy warnings introduced
**Dependency:** Step 2 must be complete

## Acceptance Criteria
- ✓ New test `test_consecutive_didchange_without_version_increments_uniquely` PASSES
- ✓ All existing tests in `lsp_generation_counter_tests.rs` PASS
- ✓ Full `perl-lsp-rs` test suite PASSES
- ✓ No clippy warnings
- ✓ Version numbers are monotonically increasing (no duplicates) even with rapid consecutive didChange notifications lacking explicit versions
- ✓ Stale version snapshots no longer cause duplicate version calculation
