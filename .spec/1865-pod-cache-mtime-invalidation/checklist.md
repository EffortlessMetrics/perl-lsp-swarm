# Implementation Checklist: #1865 — fix(hover): POD documentation cache not invalidated when module file changes externally

## Change order (compiles at each step)

### Step 1: Change pod_cache type to store (SystemTime, PodDoc) tuples

- **File:** `crates/perl-lsp-rs/src/runtime/mod.rs:217`
- **Change:** Update the `pod_cache` field type from `HashMap<PathBuf, perl_pod::PodDoc>` to `HashMap<PathBuf, (SystemTime, perl_pod::PodDoc)>`
- **Details:** 
  - Change line 217 from:
    ```rust
    pod_cache: Arc<Mutex<HashMap<PathBuf, perl_pod::PodDoc>>>,
    ```
    to:
    ```rust
    pod_cache: Arc<Mutex<HashMap<PathBuf, (SystemTime, perl_pod::PodDoc)>>>,
    ```
  - Add `use std::time::SystemTime;` to the imports at the top of mod.rs if not already present
  - This is a type-only change that will cause compilation errors in `hover.rs` which we fix in the next steps
- **Verify:** `cargo check -p perl-lsp-rs` (will fail with unresolved errors, expected)

### Step 2: Update cache hit path to validate mtime before returning cached POD

- **File:** `crates/perl-lsp-rs/src/runtime/language/hover.rs:1434-1435`
- **Change:** Replace the simple cache hit branch with mtime validation logic
- **Details:**
  - Replace lines 1434-1435:
    ```rust
    if let Some(cached) = cache.get(path) {
        cached.clone()
    ```
    with:
    ```rust
    if let Some((cached_mtime, cached_doc)) = cache.get(path) {
        // Check if file has been modified since we cached it
        match std::fs::metadata(path)
            .and_then(|m| m.modified())
        {
            Ok(current_mtime) if *cached_mtime == current_mtime => {
                // mtime matches, return cached POD
                cached_doc.clone()
            }
            Ok(_) => {
                // mtime differs, invalidate cache entry and fall through to re-extract
                drop(cached);
                cache.remove(path);
                // Will be re-extracted in the else block below
                let doc = perl_pod::extract_pod_from_file(path).unwrap_or_default();
                let mtime = std::fs::metadata(path)
                    .and_then(|m| m.modified())
                    .unwrap_or_else(|_| SystemTime::now());
                cache.insert(path.to_path_buf(), (mtime, doc.clone()));
                doc
            }
            Err(_) => {
                // Cannot read mtime (file deleted, permission denied, etc.)
                // Fall back to returning cached POD gracefully
                cached_doc.clone()
            }
        }
    ```
  - This creates a new local scope that consumes the cache guard, allowing `cache.remove()` to work
- **Depends on:** Step 1
- **Verify:** `cargo check -p perl-lsp-rs` (will still have errors from cache insertion, expected)

### Step 3: Update cache insertion to store mtime alongside POD

- **File:** `crates/perl-lsp-rs/src/runtime/language/hover.rs:1449-1450`
- **Change:** When inserting newly-extracted POD into the cache, also store the file's current modification time
- **Details:**
  - The cache hit branch now handles insertion internally (Step 2), so only the "cache miss" path needs updating
  - In the final `else` block (lines 1449-1450), replace:
    ```rust
    let doc = perl_pod::extract_pod_from_file(path).unwrap_or_default();
    cache.insert(path.to_path_buf(), doc.clone());
    doc
    ```
    with:
    ```rust
    let doc = perl_pod::extract_pod_from_file(path).unwrap_or_default();
    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or_else(|_| SystemTime::now());
    cache.insert(path.to_path_buf(), (mtime, doc.clone()));
    doc
    ```
  - Note: We use `unwrap_or_else` to gracefully handle metadata read failures (file deleted mid-extraction, permission denied, etc.) by falling back to `SystemTime::now()`
- **Depends on:** Steps 1–2
- **Verify:** `cargo check -p perl-lsp-rs` (should now compile with no errors)

### Step 4: Verify tests compile and run (no changes to test infrastructure)

- **File:** `crates/perl-lsp-rs/src/runtime/language/hover/hover_tests.rs`
- **Change:** None — existing tests should continue to work unchanged
- **Details:**
  - The existing test `pod_hover_cache_prunes_at_cap_and_evicts_active_document_path` at line 35 calls `format_pod_for_hover()` and checks `pod_cache_entries` count via `memory_state_snapshot()`
  - This test exercises both the cache miss path (lines 40–51) and the capacity-prune logic (lines 53–58)
  - No test modifications are needed because the public API of `format_pod_for_hover()` and the memory snapshot remain unchanged
  - The mtime invalidation logic is tested in Step 5 (red-TDD test)
- **Verify:** `RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs --lib hover_tests::pod_hover_cache_prunes_at_cap -- --exact --nocapture`

### Step 5: Verify no other callers are broken

- **File:** `crates/perl-lsp-rs/src/runtime/language/hover.rs` (two call sites: lines ~1400 and ~1440 in other functions)
- **Change:** None — the two internal call sites (`build_module_hover()` and one other) call `format_pod_for_hover()` and receive a `String`, which is unchanged
- **Details:**
  - Callers only care about the return type (`String`), not the cache structure
  - No changes needed to call sites
- **Verify:** `cargo clippy -p perl-lsp-rs --lib`

### Step 6: Final verification

- **Verify:** 
  - `cargo test -p perl-lsp-rs --lib` — all existing tests pass
  - `cargo xtask fmt` — formatted correctly
  - `cargo clippy -p perl-lsp-rs` — no clippy warnings
  - `RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs -- --test-threads=2` — integration tests pass

## Callers and consumers

- `format_pod_for_hover()` is called from:
  - `build_module_hover()` in `crates/perl-lsp-rs/src/runtime/language/hover.rs:~1400`
  - Another internal caller in the same file (line ~1440)
  - Tests in `crates/perl-lsp-rs/src/runtime/language/hover/hover_tests.rs:49,76`

- `pod_cache` field is accessed/modified only within `format_pod_for_hover()` itself

## Scope boundary

### Files IN scope:
- `crates/perl-lsp-rs/src/runtime/mod.rs` — cache type definition (line 217)
- `crates/perl-lsp-rs/src/runtime/language/hover.rs` — cache hit/miss logic (lines 1434–1450)

### Files OUT of scope:
- No public API changes; all internal implementation
- No changes to `perl_pod` crate (sibling crate; no mtime info stored there)
- No changes to LSP protocol or request/response types
- No changes to test infrastructure
- No changes to other caches (semantic_analyzer_cache, module_scan_cache, etc.)

## Flags for builder

1. **SystemTime comparison**: The mtime check uses `==` equality (`*cached_mtime == current_mtime`). This is safe because `SystemTime` derives `PartialEq` and we are comparing filesystem mtimes (which are stable for the same file in most environments). However, be aware that on some systems (high-resolution mtimes, filesystems that round mtimes), two reads of the same unmodified file might differ slightly. The implementation uses exact equality; if testing reveals false positives (cache invalidated despite no changes), consider using a small time delta (`< 1ms`) instead. The issue does not specify tolerance, so exact equality is chosen as the conservative default.

2. **Graceful fallback on mtime read failure**: When `std::fs::metadata()` fails (file deleted, permission denied, etc.), we use `unwrap_or_else(|_| SystemTime::now())` to generate a synthetic mtime. This means:
   - If a file is deleted mid-extract, we cache it with `SystemTime::now()`, and the next hover on that path will likely return the deleted version
   - This is acceptable because the deleted file will not be hovered again anyway
   - If permissions change between cache hit and mtime check, we return the cached POD (line ~1442), which is safe

3. **Cache prune logic unchanged**: The capacity-prune logic (lines 1437–1447) remains unchanged and operates on `HashMap<PathBuf, (SystemTime, PodDoc)>`. The `cache.retain()` closure receives `|_, _|` (key and value), and we don't examine them, so no changes needed there.

4. **No new syscalls in hot path**: Per the issue, mtime check is only on cache hit (already behind a lock acquisition), so perf impact is minimal.

5. **Test assertions**: The red-TDD builder will write a test that modifies a file's content on disk (not via LSP `didChange`) and verifies that `format_pod_for_hover()` detects the change. The test will need to use `std::fs::write()` to modify the file and possibly use `std::thread::sleep()` to ensure the OS advances the mtime (some filesystems have 1-second granularity).
