# Acceptance Criteria: Cache TypeInferenceEngine per Document (#1665)

## §Behavior

| Input/Condition | Expected Result | Test Name |
|---|---|---|
| First hover on variable `$x` in unchanged file | TypeInferenceEngine created, types inferred, hover label shown | `test_hover_first_request_creates_engine` |
| Second hover on same variable `$x`, document version unchanged | Engine retrieved from cache, same type label shown, inferred environment reused | `test_hover_cache_hit_same_version` |
| Third hover on different variable `$y`, document version unchanged | Engine retrieved from cache (same as #2), both variables' types inferred, both labels shown | `test_hover_cache_hit_different_variable` |
| Document text changes (version increments), then hover on `$x` | Cache entry invalidated, new engine created for new content, correct type for new context | `test_hover_cache_invalidation_on_change` |
| Completion on same unchanged file, after prior hovers | Engine retrieved from cache, type information enriches completion items | `test_completion_cache_hit_after_hover` |
| Cache exceeds 50 entries (LRU), then new document hovered | Oldest entries evicted, new engine created and cached | `test_cache_lru_eviction_at_50` |
| Hover on document with type inference error (e.g., syntax error) | Engine infers partial environment, graceful degradation (no crash), fallback type `Any` | `test_hover_type_engine_error_recovery` |
| Two sequential hovers on same URI with different content hashes | First hover: cache miss, engine created. Second hover: cache miss (different hash), new engine created | `test_two_hovers_different_content_same_uri` |

## §Hazards

| Hazard Class | Surface | Invariant | Test Name |
|---|---|---|---|
| **LSP-3: Cache Coherency** | `LspServer::get_or_build_type_engine(uri, text, ast)` | Cache key is `(normalized_uri, content_hash)`. When document version increments in `didChange`, content hash changes → automatic invalidation. No stale entries returned. | `test_cache_key_includes_content_hash` |
| **LSP-4: Null/Missing Entry** | `get_or_build_type_engine()` return path | Engine construction never returns `None`; always returns `Arc<TypeInferenceEngine>`. If `.infer()` fails (e.g., malformed AST), engine is cached anyway with partial environment. | `test_cache_hit_on_malformed_ast` |
| **LSP-2: Type Correctness** | `hover_label_for()` called on cached engine | Type labels must match between cached and non-cached paths. Regression test: run hover on same code before/after caching, assert labels are identical. | `test_hover_labels_match_cached_vs_uncached` |
| **LSP-1: Request Latency** | Hover/completion hot path | Cache hit latency < cache miss latency (no re-inferencing). Benchmark: 100 hovers on 500-line file, measure cache hits vs. misses. | `test_hover_cache_latency_improvement` |
| **ASYNC-1: Concurrent Requests** | Lock contention on `type_inference_engine_cache` Mutex | Two simultaneous hovers on different documents do not block each other (lock held only for cache lookup/insert, not for inference). | `test_concurrent_hovers_different_docs` |
| **CONFIG-1: Cache Strategy Alignment** | Cache eviction policy, LRU threshold | Cache eviction mirrors `semantic_analyzer_cache`: LRU at 50 entries, clear-all on overflow. Strategy is consistent across both caches. | `test_cache_eviction_policy_lru` |

## §Contracts

| Subsystem | Contract | Status |
|---|---|---|
| **PARSER_CONTRACTS.md** | No parser surface changes; type inference runs on existing parsed AST. | N/A — parser not touched |
| **LSP Protocol** | `textDocument/hover` and `textDocument/completion` response content unchanged; caching is transparent to client. | N/A — protocol layer unchanged |
| **TypeInferenceEngine** | `.new()` and `.infer()` signatures unchanged. Cache retrieves pre-computed instance, no API evolution. | Preserved — no breaking changes |
| **SemanticAnalyzer** | Cache key pattern mirrors `semantic_analyzer_cache`; independent lifecycle. No shared state. | Aligned — consistent memoization pattern |
| **DocumentState** | Document version in `didChange` is authoritative invalidation trigger. Content hash auto-updates. | Preserved — existing lifecycle respected |

## §API-Shape

| Item | Type | Signature | Callers | Risk | Notes |
|---|---|---|---|---|---|
| `type_inference_engine_cache` | Field (new) | `Arc<Mutex<HashMap<(String, u64), Arc<TypeInferenceEngine>>>>` | None (internal) | Low | Mirrors `semantic_analyzer_cache` field. |
| `get_or_build_type_engine()` | Method (new) | `pub(crate) fn get_or_build_type_engine(&self, uri: &str, text: &str, ast: &Node) -> Arc<TypeInferenceEngine>` | `hover.rs:314`, `completion.rs:759` | Low | Mirrors `get_or_build_analyzer()` exactly; callers replace `TypeInferenceEngine::new()` with this call. |
| **In hover.rs** | Call site (replace) | `let mut type_engine = self.get_or_build_type_engine(uri, text, ast);` | Line 314 | Low | Replace fresh `.new()` with cached accessor. |
| **In completion.rs** | Call site (replace) | `let mut type_engine = self.get_or_build_type_engine(uri, text, ast);` | Line 759 | Low | Replace fresh `.new()` with cached accessor. |

**Dup-risk grep:**
```bash
grep -rn "TypeInferenceEngine::new()" crates/perl-lsp-rs/src --include="*.rs"
```
Result: 2 instances (hover.rs:314, completion.rs:759). Both replaced.

**Call frequency:** High (every hover/completion request on large files). Cache hit rate expected >80% within a single file's hover/completion burst.

## §Test-Grid

| Test Category | Test Name | Assertion | File |
|---|---|---|---|
| **Positive: Cache Hit** | `test_hover_cache_hit_same_version` | `assert_eq!(label1, label2)` where label1 is first hover, label2 is second hover on same variable, same version. Verify via telemetry that engine was reused (or explicit cache instrumentation). | `crates/perl-lsp-rs/tests/hover_cache_tests.rs` |
| **Positive: Completion Uses Cache** | `test_completion_cache_hit_after_hover` | Completion items include type details (via `type_engine.get_type_at()`). Verify cache was hit by checking request latency < uncached baseline. | `crates/perl-lsp-rs/tests/completion_cache_tests.rs` |
| **Negative: Cache Invalidation** | `test_hover_cache_invalidation_on_change` | Hover before didChange, didChange (version bumped), hover after. Assert types reflect new code (e.g., variable reassigned to different type). Cache entry invalidated by content hash change. | `crates/perl-lsp-rs/tests/hover_cache_tests.rs` |
| **Negative: Malformed AST** | `test_cache_hit_on_malformed_ast` | AST with parse errors. Engine infers partial environment (no crash). Second hover hits cache, returns partial environment again. | `crates/perl-lsp-rs/tests/hover_cache_tests.rs` |
| **Boundary: Cache Eviction** | `test_cache_lru_eviction_at_50` | Create 51 different documents, hover on each. Assert oldest entry evicted. 51st entry cached, 1st not retrievable. | `crates/perl-lsp-rs/tests/hover_cache_tests.rs` |
| **Regression: Type Correctness** | `test_hover_labels_match_cached_vs_uncached` | Compare type labels from cached path vs. fresh engine (benchmark mode or unit test). Assert labels identical for all variables. | `crates/perl-lsp-rs/tests/hover_cache_tests.rs` |
| **State Transition: URI Normalization** | `test_cache_key_uri_normalization` | Same file, different URI formats (e.g., `file:///c:/foo` vs. `file:///C:/foo` on Windows). Assert single cache entry (URI normalized). | `crates/perl-lsp-rs/tests/hover_cache_tests.rs` |
| **Adversarial: Concurrent Requests** | `test_concurrent_hovers_different_docs` | Two threads, two different documents, hovers in parallel. Assert no lock contention (both complete ~instantly). Verify via lock acquisition time < 1ms. | `crates/perl-lsp-rs/tests/hover_cache_tests.rs` |

## §Blast-Radius

| Boundary | Consumer | Risk | Mitigation |
|---|---|---|---|
| **Hover request handler** | `crates/perl-lsp-rs/src/runtime/language/hover.rs:314` | Moderate: change from fresh engine to cached. If cache coherency is wrong, hover shows stale types. | Regression test (§Test-Grid `test_hover_labels_match_cached_vs_uncached`). Invalidation automatic via content hash. |
| **Completion request handler** | `crates/perl-lsp-rs/src/runtime/language/completion.rs:759` | Moderate: same as hover. Completion items may show stale type details. | Same regression test coverage. |
| **LspServer struct** | `crates/perl-lsp-rs/src/runtime/mod.rs:137` | Low: new field added, no mutation of existing fields. Constructor unchanged (cache initialized empty). | Field initialization in constructor must be reviewed (line TBD, see checklist). |
| **Cache coherency** | `document_access.rs:213` (get_or_build_analyzer) | Low: Type engine cache is independent (separate Mutex, separate eviction policy). Lifecycle tied to content hash, same as analyzer cache. | No shared state with analyzer cache. Eviction policy mirrors existing pattern. |
| **Parser/semantic layer** | `crates/perl-semantic-analyzer/` | None: TypeInferenceEngine API unchanged. No new public methods. | No changes to parser or semantic analyzer. |
| **Future refactorings** | Embedding TypeInferenceEngine in SemanticAnalyzer | Low: Cache location (LspServer) is independent. If future refactoring moves caching to analyzer, this cache can be removed as superseded. | Design does not preclude future embedding. |

## §Coverage-Map

N/A — this is a performance optimization (memoization), not a feature or coverage-impacting change. The test matrix in §Test-Grid provides sufficient coverage for cache correctness and regression prevention.

---

## Implementation Checklist References

(From `checklist.md` — test names link acceptance rows to implementation steps):

- **Step 1:** Add `type_inference_engine_cache` field to LspServer → validates §API-Shape
- **Step 2:** Implement `get_or_build_type_engine()` → validates §API-Shape, §Hazards LSP-3
- **Step 3:** Replace hover call site → validates §Behavior, §Blast-Radius
- **Step 4:** Replace completion call site → validates §Behavior, §Blast-Radius
- **Step 5:** Write red tests (all §Test-Grid rows) → validates §Behavior, §Hazards, §Test-Grid
- **Step 6:** Verify cache coherency and latency → validates §Hazards LSP-1, LSP-4
