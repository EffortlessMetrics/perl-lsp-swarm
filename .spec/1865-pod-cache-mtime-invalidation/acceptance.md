# Acceptance Criteria: #1865 — fix(hover): POD documentation cache not invalidated when module file changes externally

## §Behavior

| Input / Condition | Expected Result | Notes |
|---|---|---|
| Hover over a module, cache is populated with (mtime, POD) | Cached entry stored | Normal case: cache stores file mtime alongside POD |
| Hover same module again within 2 seconds (mtime unchanged) | Return cached POD immediately | mtime match triggers fast path, no re-extraction |
| File is modified externally (e.g., `echo "=head1 NEW" >> module.pm`), then hover on same path | Return updated POD, not cached stale version | mtime differs, cache is invalidated, file is re-read and re-extracted |
| Metadata read fails (e.g., file deleted, permission denied) on cache-hit check | Return cached POD gracefully | Fallback to cached POD when mtime read fails, no crash |
| Metadata read fails on first extraction (e.g., temp file with no read permission) | Cache entry with synthetic mtime (SystemTime::now()), POD is returned | Synthetic mtime ensures next mtime check will likely differ, triggering re-extraction |
| Cache capacity reached (1024 entries), new hover added | Prune to 512 entries, new entry added | Capacity-prune logic unchanged, continues to work with (mtime, POD) tuples |

All tests pass: `cargo test -p perl-lsp-rs`
No clippy warnings: `cargo clippy -p perl-lsp-rs`
Formatted: `cargo xtask fmt`

## §Hazards

**Subsystem-specific defaults consulted**: docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md — LSP subsystem (LSP-1, LSP-2, LSP-3, LSP-4)

| Class | Invariant | Surface (file:fn) | Required adversarial test |
|---|---|---|---|
| **LSP-1: Request-shape validation** | Every LSP request handler validates required fields before processing. A missing or wrong-type field returns `ErrorCode::InvalidParams` with a message that names the missing field and its expected type. The handler never panics on malformed input. | `crates/perl-lsp-rs/src/runtime/language/hover.rs:format_pod_for_hover()` — This is an internal function called by LSP handlers, not a handler itself. No protocol validation needed. | N/A — `format_pod_for_hover()` is internal; no LSP request is directly involved. The function accepts a `&Path` (Rust type) so protocol validation is upstream. |
| **LSP-2: Document lifecycle (didOpen sequencing)** | Any handler that accesses document state must tolerate being called before `textDocument/didOpen` completes, after `textDocument/didClose`, and on a URI that was never opened. The result must be an empty/null response — never stale data from a previously-open document with the same URI. | `crates/perl-lsp-rs/src/runtime/language/hover.rs:format_pod_for_hover()` — function reads file metadata and extracts POD from the filesystem, independent of LSP document lifecycle. | N/A — `format_pod_for_hover()` does not access the LSP document store; it reads files directly from the filesystem. Lifecycle hazard does not apply. |
| **LSP-3: URI normalization (cross-platform + UNC)** | All URI handling round-trips correctly for Unix absolute paths, Windows drive-letter paths, Windows forward-slash paths, and UNC paths. The canonical form must be stable across round-trips. | `crates/perl-lsp-rs/src/runtime/language/hover.rs:format_pod_for_hover()` receives a `&Path` (already filesystem-canonical). Callers normalize paths before passing to this function. | Verify that paths passed to `format_pod_for_hover()` from upstream callers (`build_module_hover()`) are canonicalized. No new path-normalization code is introduced in this change. |
| **LSP-4: Actionable error guidance** | `ErrorResponse` messages must name what went wrong and, where possible, what the client should do. Error messages follow the pattern `{what failed}: {specific cause} (hint: {what client can do})`. | `crates/perl-lsp-rs/src/runtime/language/hover.rs:format_pod_for_hover()` — function returns `String`, not `ErrorResponse`. No error messages are introduced or changed. | N/A — `format_pod_for_hover()` returns `String` (empty on failure). No LSP error messages are involved. |
| **Correctness: mtime comparison stability** | SystemTime equality comparison (`==`) is reliable for filesystem mtime values. Two reads of an unmodified file must return equal SystemTime values. Floating-point or rounding errors do not cause false cache invalidations. | `crates/perl-lsp-rs/src/runtime/language/hover.rs:1434-1435` (cache hit check: `*cached_mtime == current_mtime`) | Test that the same file read twice in immediate succession returns equal SystemTime values. Test that a file modified by `std::fs::write()` shows a different SystemTime. |
| **Robustness: graceful fallback on metadata read failure** | When `std::fs::metadata()` fails during cache-hit mtime check (e.g., file deleted between cache insertion and check), the function returns cached POD gracefully without panicking or logging an error. | `crates/perl-lsp-rs/src/runtime/language/hover.rs:1434-1435` (error arm: `Err(_) => { ... cached_doc.clone() }`) | Test that calling `format_pod_for_hover()` on a deleted file (populated in cache, then deleted before second call) returns the cached POD. Test that the function does not panic. |
| **Correctness: synthetic mtime handles metadata failures gracefully** | When metadata read fails during cache insertion (file unreadable immediately after extraction), a synthetic mtime (`SystemTime::now()`) is used, allowing the next cache-hit check to safely compare times. | `crates/perl-lsp-rs/src/runtime/language/hover.rs:1449-1450` (cache insertion: `.unwrap_or_else(\|_\| SystemTime::now())`) | N/A — This is a graceful fallback. The behavior is correct by construction (SystemTime::now() will differ from the cached time on the next access, triggering re-extraction). Existing tests verify that new hovers on different files are extracted correctly. |

## §Contracts

| Contract | Source document + section | How this change satisfies or extends it |
|---|---|---|
| LSP hover protocol | LSP specification: `textDocument/hover` request/response | This change does not modify the LSP protocol shape. It improves the correctness of hover POD caching by detecting external file modifications. The response type (`Hover` with markdown content) is unchanged. |
| File modification detection | CLAUDE.md: "Workspace file-watcher contract — LSP server must detect external file modifications and invalidate caches accordingly" | This change adds mtime-based cache invalidation to the POD documentation cache, fulfilling the broader file-watcher contract. This is a direct correctness fix. |

## §API-Shape

| Item | Kind | Signature / Range | Dup-risk (grep result) | Caller count |
|---|---|---|---|---|
| `pod_cache` | field | `Arc<Mutex<HashMap<PathBuf, (SystemTime, perl_pod::PodDoc)>>>` | One definition in `mod.rs:217`, zero public references | N/A — field is private, only accessed via `self.pod_cache` within `format_pod_for_hover()` |
| `format_pod_for_hover()` | private method | `fn format_pod_for_hover(&self, path: &Path) -> String` | Signature unchanged; only callers are internal | 2 internal call sites + 2 test call sites |

N/A — No new public API surface introduced. Cache type change is internal.

## §Test-Grid

| Scenario | Kind | Test name | Invariant discharged |
|---|---|---|---|
| Normal input: file with POD, cache miss | positive | `test_pod_cache_external_modification_invalidates_on_mtime_change` | POD is extracted and cached with correct mtime |
| External file modification detected | positive | `test_pod_cache_external_modification_invalidates_on_mtime_change` | mtime differs, cache is invalidated, updated POD is returned |
| File unchanged within same second | positive | `test_pod_cache_mtime_match_returns_cached_pod` (inferred from existing capacity-prune test) | mtime matches, cached POD returned immediately, no re-extraction |
| Metadata read failure on cache-hit check | negative | `test_pod_cache_mtime_check_failure_returns_cached_gracefully` | File deleted/unreadable after cache insertion; function returns cached POD without panicking |
| Metadata read failure on insertion | negative | `test_pod_cache_synthetic_mtime_on_metadata_read_failure` | File unreadable immediately after extraction; synthetic mtime allows next access to safely invalidate |
| Empty POD file | negative | `test_pod_cache_empty_file_cached_and_verified` | Empty file is cached, mtime is stored, next access returns empty string |
| File in read-only directory (cannot re-extract) | adversarial | `test_pod_cache_permission_denied_falls_back_gracefully` | Read-only file is cached; mtime check fails with permission error; function returns cached POD |
| Rapid successive hovers (same file) | state-transition | `test_pod_cache_successive_hovers_same_file_share_cache` | First hover caches, second hover returns same cached entry, no re-extraction observed via memory snapshots |
| Very large POD document (edge case) | positive | Existing capacity-prune test covers this | Large (>1MB) POD is cached and pruned correctly when capacity is reached |

## §Blast-Radius

| Consumer | Crate | Dependency type | Impact | Required update |
|---|---|---|---|---|
| `build_module_hover()` | `perl-lsp-rs` | internal call to `format_pod_for_hover()` | None — return type unchanged (`String`) | None — callers unaffected |
| Hover LSP handler | `perl-lsp-rs` | indirect via `build_module_hover()` | Improved correctness — external edits now detected | None — no handler changes required |
| Memory snapshot test | `perl-lsp-rs` | reads `pod_cache_entries` | None — field count unchanged (still counts HashMap entries) | None — tests pass unchanged |

**Must-not-touch boundary:**
- `perl_pod` crate — no changes to extraction logic or PodDoc type
- LSP protocol types — hover request/response types unchanged
- Other caches — semantic_analyzer_cache, module_scan_cache unaffected
- File-watcher integration — this change is independent of the file-watcher, improving correctness independently

**Scope containment:**
- Changes are localized to two files: `mod.rs` (type definition) and `hover.rs` (cache logic)
- No cascading changes to callers (they only see `String` return type)
- No new dependencies introduced (SystemTime is std::time)
- No changes to public API surface (pod_cache field is private)
