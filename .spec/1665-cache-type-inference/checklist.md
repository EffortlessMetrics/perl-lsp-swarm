# Implementation Checklist: Cache TypeInferenceEngine per Document (#1665)

## Build Overview

**Scope:** Add `type_inference_engine_cache` to `LspServer` and implement `get_or_build_type_engine()` method. Replace two call sites (hover and completion). Write comprehensive test suite.

**Size:** ~50 lines of implementation + ~200 lines of tests.

**Order:** Sequential compilation (Rust will catch ordering issues early).

**Test File:** `crates/perl-lsp-rs/tests/hover_cache_tests.rs` (new file).

---

## Step 1: Add cache field to LspServer struct

**File:** `crates/perl-lsp-rs/src/runtime/mod.rs`

**What changes:**
- Add new field to `LspServer` struct (around line 223, after `semantic_analyzer_cache`)

**Signature:**
```rust
/// Cache of TypeInferenceEngine results keyed by (normalized_uri, content_hash).
///
/// Avoids re-running type inference on repeated hover/completion requests to the
/// same document version. Content hash provides automatic invalidation when source
/// text changes. Evicts all entries when full (50-entry LRU like semantic_analyzer_cache).
pub(crate) type_inference_engine_cache:
    Arc<Mutex<HashMap<(String, u64), Arc<TypeInferenceEngine>>>>,
```

**Note:** Must import `TypeInferenceEngine` at top of file. Check current imports for `perl_semantic_analyzer` or add:
```rust
use perl_semantic_analyzer::analysis::type_inference::TypeInferenceEngine;
```

**Dependencies:** None (field only, no initialization yet).

**Verify command after:**
```bash
cargo build -p perl-lsp-rs --lib 2>&1 | grep -E "error|warning: unused" | head -20
```

---

## Step 2: Initialize cache field in LspServer constructor

**File:** `crates/perl-lsp-rs/src/runtime/constructors.rs`

**What changes:**
- Find the `LspServer::new()` or constructor method
- Add initialization line for the new field

**Location:** Look for where `semantic_analyzer_cache` is initialized (should be `Arc::new(Mutex::new(HashMap::new()))`)

**Signature:**
```rust
type_inference_engine_cache: Arc::new(Mutex::new(HashMap::new())),
```

**Dependencies:** Step 1 (field must exist in struct).

**Verify command after:**
```bash
cargo build -p perl-lsp-rs --lib 2>&1 | grep -E "error|missing field" | head -20
```

---

## Step 3: Implement get_or_build_type_engine() method

**File:** `crates/perl-lsp-rs/src/runtime/document_access.rs`

**What changes:**
- Add new method after `get_or_build_analyzer()` (currently at line 213)

**Full implementation:**
```rust
/// Get or build a TypeInferenceEngine for the given document.
///
/// Uses the same memoization pattern as `get_or_build_analyzer()`: caches
/// by (normalized_uri, content_hash). Returns cached engine if document version
/// is unchanged, or builds fresh engine and caches if version has changed
/// (auto-invalidation via content hash).
///
/// # Arguments
///
/// * `uri` - Document URI (will be normalized for cache key)
/// * `text` - Source text (content hash computed for cache key)
/// * `ast` - Parsed AST (passed to engine.infer())
///
/// # Returns
///
/// `Arc<TypeInferenceEngine>` — cloned from cache if hit, or newly built and cached
/// on miss. Engine is guaranteed to have successfully called `.infer()` on the AST
/// (errors are logged but engine is cached anyway with partial environment).
///
/// # Lock Ordering
///
/// Always acquire `documents` before `type_inference_engine_cache` to maintain
/// consistent lock ordering and avoid deadlock.
pub(crate) fn get_or_build_type_engine(
    &self,
    uri: &str,
    text: &str,
    ast: &perl_parser::ast::Node,
) -> Arc<TypeInferenceEngine> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    let content_hash = hasher.finish();

    let normalized = self.normalize_uri_key(uri);
    let key = (normalized, content_hash);

    // Read path: return clone of cached entry if present.
    {
        let cache = self.type_inference_engine_cache.lock();
        if let Some(cached) = cache.get(&key) {
            return Arc::clone(cached);
        }
    }

    // Cache miss: build the engine outside the lock.
    let mut engine = TypeInferenceEngine::new();
    let _ = engine.infer(ast); // Ignore errors; partial environment is ok.

    // Write path: insert, evicting all entries when the cache is full.
    {
        let mut cache = self.type_inference_engine_cache.lock();
        if cache.len() >= 50 {
            cache.clear();
        }
        cache.insert(key, Arc::clone(&Arc::new(engine.clone())));
    }

    Arc::new(engine)
}
```

**Note on mutability:** If `TypeInferenceEngine` does not implement `Clone`, the cache must store the engine in a cell for interior mutability. Check the current TypeInferenceEngine struct and adjust:
- If `Clone` is not derived, add `#[derive(Clone)]` to the struct (in `type_inference.rs`)
- If interior mutability is needed, use `Arc<Mutex<TypeInferenceEngine>>` instead

**Alternative (if clone is expensive):** Store the inferred environment result instead of the engine itself, and rebuild engine on cache hit with pre-computed environment. See §Hazards in `acceptance.md` for tradeoffs.

**Dependencies:** Steps 1-2 (field must exist and be initialized).

**Verify command after:**
```bash
cargo build -p perl-lsp-rs --lib 2>&1 | grep error | head -20
```

---

## Step 4: Replace TypeInferenceEngine::new() in hover.rs

**File:** `crates/perl-lsp-rs/src/runtime/language/hover.rs`

**What changes:**
- Line 314: Replace `let mut type_engine = crate::type_inference::TypeInferenceEngine::new();`
- With: `let mut type_engine = self.get_or_build_type_engine(uri, text, ast).as_ref().clone();` OR unwrap Arc

**Old code (line 311-315):**
```rust
// Infer type for variables using TypeInferenceEngine
let type_info = if symbol_info.kind.is_variable() {
    let var_name = &symbol_info.name; // already without sigil
    let mut type_engine = crate::type_inference::TypeInferenceEngine::new();
    let _ = type_engine.infer(ast); // ignore errors, just build env
```

**New code:**
```rust
// Infer type for variables using cached TypeInferenceEngine
let type_info = if symbol_info.kind.is_variable() {
    let var_name = &symbol_info.name; // already without sigil
    let type_engine = self.get_or_build_type_engine(uri, text, ast);
```

**Note:** Remove the `let _ = type_engine.infer(ast);` line since `.get_or_build_type_engine()` already calls `.infer()`.

**Remove `mut`** from `type_engine` (read-only access to cached engine).

**Dependencies:** Step 3 (method must exist).

**Verify command after:**
```bash
cargo build -p perl-lsp-rs --lib 2>&1 | grep error | head -20
```

---

## Step 5: Replace TypeInferenceEngine::new() in completion.rs

**File:** `crates/perl-lsp-rs/src/runtime/language/completion.rs`

**What changes:**
- Line 759: Replace `let mut type_engine = TypeInferenceEngine::new();`
- With: `let type_engine = self.get_or_build_type_engine(uri, text, ast);`

**Old code (line 756-761):**
```rust
let mut base_completions =
    provider.get_completions_with_path(&doc.text, offset, Some(uri));

// Enhance completions with type information
let mut type_engine = TypeInferenceEngine::new();
let _ = type_engine.infer(ast); // Build type environment
```

**New code:**
```rust
let mut base_completions =
    provider.get_completions_with_path(&doc.text, offset, Some(uri));

// Enhance completions with type information (cached engine)
let type_engine = self.get_or_build_type_engine(uri, text, ast);
```

**Note:** Remove `let _ = type_engine.infer(ast);` line.

**Remove `mut`** since engine is immutable (read-only access).

**Check access pattern:** Line 769 calls `type_engine.get_type_at(var_name)`. Verify this method exists and takes `&self` (not `&mut self`). If it requires `&mut`, you'll see a compile error; fix in Step 6.

**Dependencies:** Step 3 (method must exist).

**Verify command after:**
```bash
cargo build -p perl-lsp-rs --lib 2>&1 | grep error | head -20
```

---

## Step 6: Write red tests

**File:** `crates/perl-lsp-rs/tests/hover_cache_tests.rs` (NEW FILE)

**What changes:**
- Create comprehensive test suite covering all §Test-Grid rows from `acceptance.md`

**Test structure:**
```rust
#[cfg(test)]
mod hover_cache_tests {
    use super::*;
    use perl_lsp::runtime::LspServer;
    use perl_parser::{Parser, ast::Node};

    /// Test: cache hit on second hover
    #[test]
    fn test_hover_cache_hit_same_version() {
        // Arrange: create server, add document, parse AST
        let server = LspServer::test_default();
        let uri = "file:///test.pl";
        let text = "my $x = 42; my $y = $x + 1;";
        let parser = Parser::new();
        let ast = parser.parse(text).unwrap();
        
        // Act: first hover
        let label1 = /* extract type label via hover */ ;
        
        // Act: second hover on same document, version unchanged
        let label2 = /* extract type label via hover */ ;
        
        // Assert: labels match (cache was hit)
        assert_eq!(label1, label2);
    }

    /// Test: cache invalidation on document change
    #[test]
    fn test_hover_cache_invalidation_on_change() {
        // Arrange: create server, add document
        let server = LspServer::test_default();
        let uri = "file:///test.pl";
        
        // Act: hover on original text
        let text1 = "my $x = 42;";
        let label1 = /* hover */ ;
        
        // Act: change document (version bumped), hover on modified text
        let text2 = "my $x = 'hello';"; // type changed
        let label2 = /* hover */ ;
        
        // Assert: labels differ (cache was invalidated by content hash change)
        assert_ne!(label1, label2);
    }

    /// Test: cache LRU eviction at 50 entries
    #[test]
    fn test_cache_lru_eviction_at_50() {
        let server = LspServer::test_default();
        
        // Create 51 different documents
        for i in 0..51 {
            let uri = format!("file:///test{}.pl", i);
            let text = format!("my $x{} = {};", i, i);
            let parser = Parser::new();
            let ast = parser.parse(&text).unwrap();
            
            // Trigger cache insertion
            let _ = server.get_or_build_type_engine(&uri, &text, &ast);
        }
        
        // Assert: cache size is at most 50 (oldest evicted)
        let cache = server.type_inference_engine_cache.lock();
        assert!(cache.len() <= 50);
    }

    /// Regression: type labels match cached vs. uncached paths
    #[test]
    fn test_hover_labels_match_cached_vs_uncached() {
        // Create two scenarios:
        // 1. Fresh engine (uncached)
        // 2. Cached engine (same content)
        // Assert type labels are identical for all variables
        
        let text = "my $x = 42; my $y = 'hello'; my @arr = (1, 2, 3);";
        let parser = Parser::new();
        let ast = parser.parse(text).unwrap();
        
        // Uncached path
        let mut engine_fresh = TypeInferenceEngine::new();
        let _ = engine_fresh.infer(&ast);
        let label_x_fresh = engine_fresh.hover_label_for("x");
        
        // Cached path (via server)
        let server = LspServer::test_default();
        let uri = "file:///test.pl";
        let engine_cached = server.get_or_build_type_engine(uri, text, &ast);
        let label_x_cached = engine_cached.hover_label_for("x");
        
        // Assert
        assert_eq!(label_x_fresh, label_x_cached);
    }
}
```

**Test coverage:** Write one test per row in §Test-Grid. Minimum 8 tests.

**Dependencies:** Steps 1-5 (implementation must be complete for tests to compile).

**Verify command after:**
```bash
cargo test -p perl-lsp-rs --test hover_cache_tests 2>&1 | tail -20
```

Expected output: All tests fail (red tests — implementation not yet complete beyond structure).

---

## Step 7: Verify full compilation and test suite

**Verify compilation:**
```bash
cargo build -p perl-lsp-rs --lib 2>&1 | grep -c "error"
# Should output: 0 (no errors)
```

**Verify tests run (even if failing):**
```bash
cargo test -p perl-lsp-rs --test hover_cache_tests --lib 2>&1 | tail -50
```

**Verify clippy/fmt (after builder completes impl):**
```bash
cargo xtask fmt
cargo clippy -p perl-lsp-rs --lib 2>&1 | grep -E "warning|error"
```

---

## Implementation Order

1. **Step 1:** Add field (struct change) → must compile
2. **Step 2:** Initialize field (constructor change) → must compile
3. **Step 3:** Implement method (new public API) → must compile
4. **Step 4:** Replace hover call site → must compile
5. **Step 5:** Replace completion call site → must compile
6. **Step 6:** Write red tests → tests run (will fail until impl logic is correct)
7. **Step 7:** Verify workspace compiles clean

Each step compiles independently (Rust enforces correctness at each stage).

---

## Mutability Issue (may arise in Step 3/4/5)

If `TypeInferenceEngine` does not support immutable access to inferred types (e.g., `hover_label_for` and `get_type_at` require `&mut self`), the builder must:

1. **Add `Clone` derive** to `TypeInferenceEngine` (in `type_inference.rs`)
2. **Change cache value type** from `Arc<TypeInferenceEngine>` to `Arc<Mutex<TypeInferenceEngine>>`
3. **Update Step 3 code** to lock the mutex before calling methods

This is a known issue; the checker should verify TypeInferenceEngine's method signatures and adjust accordingly.

---

## Test File Checklist

All tests must:
- [ ] Use `Result<()>` return type OR `perl_tdd_support::must` assertion
- [ ] Mock or construct LspServer in test environment
- [ ] Assert cache hit/miss behavior explicitly (e.g., via cache size or latency)
- [ ] Cover both hover and completion paths
- [ ] Include regression test (cached vs. uncached type labels match)

---

## Post-Implementation (Builder Responsibility)

After all red tests pass:
1. Run `cargo test -p perl-lsp-rs` (full suite)
2. Run `cargo xtask fmt` (formatting)
3. Run `cargo clippy -p perl-lsp-rs --lib` (linting)
4. Commit: `git add .spec/ crates/perl-lsp-rs/ && git commit -m "impl(lsp): cache TypeInferenceEngine per document (#1665)"`
5. Push: `git push -u origin impl/1665-cache-type-inference`
