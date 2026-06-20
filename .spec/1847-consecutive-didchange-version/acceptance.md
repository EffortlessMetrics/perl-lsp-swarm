# Acceptance Criteria: #1847 — Fix consecutive didChange auto-increment producing same version number

## §Behavior

| Input | Condition | Expected Result |
|-------|-----------|-----------------|
| Three consecutive didChange notifications (no explicit version) | Client sends v1, then v2, then v3 without explicit `version` field | Stored document versions are 2, 3, 4 (monotonically increasing) |
| didChange with explicit version | Client sends `version: 5` | Uses explicit version 5 (no auto-increment) |
| didChange without version, after explicit version | First explicit `version: 10`, then didChange without version | Auto-incremented version is 11 (based on current stored state) |
| Multiple rapid didChange without versions (stress) | 10 consecutive notifications, all without explicit versions | All versions unique and strictly increasing (2, 3, 4, ..., 11) |
| Stale didChange (version <= current) | Current version is 5, incoming explicit version is 3 | Notification is ignored, document state unchanged |

## §Hazards

| Class | Surface | Risk | Mitigation | Invariant |
|-------|---------|------|-----------|-----------|
| **LSP-1** | `handle_did_change_with_cancellation` / version computation (line 449-450) | Auto-increment reads stale `doc_state` snapshot; multiple rapid calls produce duplicate versions | Re-fetch current document state from synchronized map immediately before computing version | Version numbers are strictly monotonically increasing across all didChange notifications for a URI |
| **LSP-2** | Document state mutation / map insert at line 801 | If document is updated concurrently by another thread, version field may become inconsistent | Lock acquisition (line 395) ensures exclusive access to documents map during version computation and storage | Document map contains exactly one state per normalized URI; version field matches the stored state version |
| **LSP-3** | stale check at lines 435-445 | If version comparison uses pre-update snapshot, stale detection may fail on edge cases | Stale check happens BEFORE version auto-increment; incoming version is compared against doc_state version at entry time | Stale didChange (version <= stored) are rejected and do not mutate state |
| **LSP-4** | Fallback for missing explicit version (line 449-450) | Non-LSP-compliant clients omit version field; auto-increment must handle undefined/null gracefully | Use `unwrap_or_else` fallback; if version is missing, derive from current stored version | All documents have valid positive version numbers; no version is ever zero or negative after assignment |
| **COV-1** | Test coverage of consecutive didChange without versions | Existing tests (`test_rapid_did_change_resolves_to_latest`) use explicit versions; edge case of missing versions is untested | Add new test `test_consecutive_didchange_without_version_increments_uniquely` that sends N consecutive didChange with omitted version fields | Version auto-increment code path is exercised and verified to produce unique versions |
| **COV-2** | Edge case: generation counter vs. version counter | Two independent counters (`generation` at line 455, `version` at line 449); mismatch could cause stale/fresh confusion | Generation is incremented per didChange; version is stored in DocumentState; both are checked for staleness | Generation and version counters track independently; staleness logic checks generation first (line 780-784) |

## §Contracts

| Subsystem | Contract | Impact | Implementation |
|-----------|----------|--------|-----------------|
| **LSP Protocol** | `textDocument/didChange` version field (LSP 3.17 spec § 3.8.5) | Version MUST be provided by client; if omitted, server must auto-increment monotonically | Compute new version as `current_stored_version + 1` (never duplicate) |
| **LSP Protocol** | Text document synchronization state (§ 3.8.1-3.8.5) | Client and server versions must stay synchronized; version mismatch indicates corruption or message loss | Each didChange increments version by exactly 1; server rejects stale versions (≤ current) |
| **Document State Lock** | `documents: Mutex<HashMap>` (perl-lsp-rs/src/lib.rs) | Exclusive access to document map; all reads/writes must hold lock | Lock is acquired at line 395 and held until line 804; version computation happens within locked region |
| **Generation Counter** | Per-document generation tracking (perl-lsp-rs/src/runtime/text_sync/document_state.rs) | Generation increments on each change to detect stale parse results; must not be confused with version | Generation is `u32 AtomicU32` (internal); Version is `i32` (LSP-visible); both tracked separately in DocumentState |

## §API-Shape

**No new public API surface introduced.**

| Category | Item | Change | Dup-Risk Grep | Caller Count |
|----------|------|--------|----------------|--------------|
| Function | `handle_did_change_with_cancellation` | Signature unchanged; internal logic fixed | `grep -rn "handle_did_change_with_cancellation" crates/perl-lsp-rs/src/` | 2 callers: `handle_did_change_dispatch` (line 35 of text_document.rs), `handle_did_change` (line 360 of text_sync.rs) — no changes needed |
| Struct | `DocumentState` | No new fields; `version: i32` unchanged | `grep -rn "DocumentState {" crates/perl-lsp-rs/src/` | Used at 5 locations; all continue to work with existing `version` field |
| Struct | `Doc` | Constructor unchanged; version parameter remains `i32` | `grep -rn "Doc {" crates/perl-lsp-rs/src/` | One location (line 462 of text_sync.rs) — will use new version computation |

## §Test-Grid

| Category | Scenario | Test Name | Assertion | Invariant |
|----------|----------|-----------|-----------|-----------|
| **Positive** | Single didChange with explicit version | (existing) `test_rapid_did_change_resolves_to_latest` | Hover succeeds after rapid explicit-versioned changes | Explicit versions are respected |
| **Positive** | Consecutive didChange without explicit versions | **NEW:** `test_consecutive_didchange_without_version_increments_uniquely` | Three didChange with omitted `version` field; document remains parseable and accessible | Auto-incremented versions are unique and monotonically increasing |
| **Positive** | Mix of explicit and auto-incremented versions | **NEW:** Helper in `test_consecutive_didchange_without_version_increments_uniquely` | Explicit version 5, then auto-increment, then explicit version 10 | Explicit versions are used as-is; auto-increments fill gaps without duplication |
| **Negative** | Stale didChange (version ≤ current) | (existing) Stale check at line 435-445 | Notification is ignored; document state unchanged | Stale notifications do not corrupt state |
| **Negative** | Rapid concurrent didChange (stress) | **NEW:** `test_consecutive_didchange_without_version_increments_uniquely` extended with 10 rapid calls | All 10 notifications succeed; all versions unique | No race condition causes duplicate versions |
| **Adversarial** | didChange with version = 0 / negative | (existing) Stale check / numeric validation | Treated as valid (not stale); stored as-is | Zero and negative versions are accepted if sent explicitly by client |
| **State-Transition** | Open document (v1) → didChange (no version) → didChange (no version) → hover | **NEW:** Part of `test_consecutive_didchange_without_version_increments_uniquely` | Hover succeeds; document version is 3 | State transitions are correct; no stale state persists |

## §Blast-Radius

| Item | Consumers | Boundary | Must-Not-Touch |
|------|-----------|----------|-----------------|
| **Function `handle_did_change_with_cancellation`** | `handle_did_change_dispatch` (routing.rs:36), `handle_did_change` (text_sync.rs:360) | Internal implementation of LSP method handler; signature unchanged | LSP dispatch routing (routing.rs) — no changes |
| **Version field in DocumentState** | Document storage, version comparison (lines 436, 781), version assignment (line 765, 504) | Version is computed locally and stored; read during stale checks | Parser (perl-parser/*) — parser is independent of LSP version; no coupling |
| **Document map lock (Mutex)** | All document reads/writes in text_sync.rs, symbol indexing, diagnostics | Lock is acquired, held during mutation, dropped before async operations | Workspace coordinator (perl-workspace/*) — reads only; locking is unaffected |
| **Auto-increment fallback** | Only used when client omits version field; existing code with explicit versions is unaffected | Fallback is self-contained; no external side effects | Non-LSP transports (DAP, etc.) — DAP has its own version handling; no cross-subsystem impact |

