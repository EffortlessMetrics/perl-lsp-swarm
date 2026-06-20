# Acceptance Criteria: #1668 — cap O(n) workspace/symbol scan

## §Behavior

| Input / Condition | Expected Result | Notes |
|---|---|---|
| Query 'get' in 1000-symbol index with cap=200 | Returns exactly 200 symbols, search exits early | No unnecessary clones beyond 200 |
| Query 'get' in 100-symbol index with cap=200 | Returns all 100 symbols | Cap is larger than result set |
| Empty query in any index with cap=200 | Returns empty vec after trimming | `query.trim()` filters empty queries |
| Method lookup in signature_help with cap=None | Returns all matching symbols regardless of count | Signature help needs full candidate list |
| Both source and generated symbols, cap=200 | Returns at most 200 total from both searches combined | Early exit stops both searches once cap reached |
| Partial index during Building state with cap=200 | Returns at most 200 from partial index (open docs only) | Same capping applied to degraded path |

**All tests pass:** `cargo test -p perl-workspace --lib && cargo test -p perl-lsp-rs --lib`
**No clippy warnings:** `cargo clippy -p perl-workspace -p perl-lsp-rs`
**Formatted:** `cargo xtask fmt`

## §Hazards

| Class | Invariant | Surface (file:fn) | Required adversarial test |
|---|---|---|---|
| **Bounds/overflow** | Cap is never exceeded; results vec never grows beyond cap | `workspace_index.rs:search_source_symbols` + `search_generated_workspace_symbols` | `test_search_source_symbols_respects_cap_exact_boundary` — test cap=200 with exactly 200 matches |
| **State-transition coherence** | Early exit in one search path doesn't break state machine transitions (Building→Ready→Degraded) | `workspace.rs:handle_workspace_symbol` (lines 290, 337) | `test_workspace_symbol_cap_during_building_transition` — cap applied before and after index transition |
| **ID/reference-space collision** | Capping preserves symbol identity (URIs, qualified names, positions remain valid after truncation) | `workspace_index.rs:search_source_symbols` + `workspace.rs:handle_workspace_symbol` | `test_capped_symbols_have_valid_identifiers` — all 200 returned symbols have valid, unique URIs and qualified names |
| **Off-by-one in loop termination** | Cap check `results.len() >= cap.unwrap()` correctly stops at exactly cap, not cap-1 or cap+1 | `workspace_index.rs:search_source_symbols` (lines 2917-2936) + `search_generated_workspace_symbols` (lines 2943-3001) | `test_search_cap_off_by_one_lower` (199 symbols, cap=200, expect 199) + `test_search_cap_off_by_one_upper` (201 symbols, cap=200, expect 200) |
| **Performance / measurement integrity** | Benchmarking shows O(k) where k=cap, not O(n) where n=total symbols | `workspace_index.rs:search_source_symbols` internals | `bench_search_source_symbols_capped_vs_uncapped` — elapsed time with cap=200 must be < 5ms even on 10k symbol index |
| **Protocol safety** | LSP workspace/symbol response cap is always respected; client never receives more than 200 results | `workspace.rs:handle_workspace_symbol` (lines 290-305, 337-348) | `test_workspace_symbol_response_respects_lsp_cap` — mock LSP client request, verify response.len() <= cap |

**Subsystem-specific defaults consulted:** docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md — LSP (LSP-1 through LSP-4) + Workspace index (internal perf hazards)

## §Contracts

| Contract | Source document + section | How this change satisfies or extends it |
|---|---|---|
| WorkspaceIndex dual indexing | PARSER_CONTRACTS.md (if exists) or internal design | Capping preserves dual-index invariant: both qualified and bare names are searched with same cap applied |
| LSP workspace/symbol cap protocol | LSP specification v3.17 § workspace/symbol | Response must not exceed `cap` results; this change ensures cap is applied at search boundary, not post-collection |
| Open document fallback | workspace.rs lifecycle comments (lines 276-283, 331-367) | Capping applied consistently to both full-index (Ready state) and partial-index (Building/Degraded state) paths |

## §API-Shape

| Item | Kind | Signature / Range | Dup-risk (grep result) | Caller count |
|---|---|---|---|---|
| `search_source_symbols` | public method | `pub fn search_source_symbols(&self, query: &str, cap: Option<usize>) -> Vec<WorkspaceSymbol>` | 3 results in search (1 definition + 2 in docs) | 5 call sites (workspace.rs ×3, signature_help.rs ×1, workspace_index.rs ×1) |
| `search_generated_workspace_symbols` | public method | `pub fn search_generated_workspace_symbols(&self, query: &str, cap: Option<usize>) -> Vec<WorkspaceSymbol>` | 2 results in search (1 definition + 1 in docs) | 2 call sites (workspace.rs ×1, workspace_index.rs ×1) |

## §Test-Grid

| Scenario | Kind | Test name | Invariant discharged |
|---|---|---|---|
| Query matches exactly cap symbols | positive | `test_search_source_symbols_returns_cap_when_exact_match` | Basic capping behavior: 200 query matches cap=200 returns 200 |
| Query matches fewer than cap symbols | positive | `test_search_source_symbols_returns_all_when_fewer_than_cap` | No truncation when results < cap: 50 matches with cap=200 returns 50 |
| Query matches more than cap symbols | positive | `test_search_source_symbols_truncates_excess_results` | Excess truncation: 500 matches with cap=200 returns exactly 200 |
| Empty query string (before trim) | negative | `test_search_source_symbols_empty_query_returns_empty` | No panic on empty; returns empty vec after trim() |
| Query with only whitespace | negative | `test_search_source_symbols_whitespace_only_query` | Trim prevents false matches; returns empty when query is whitespace |
| Cap is None (no limit) | negative | `test_search_source_symbols_cap_none_returns_all` | Backward compatibility: None cap returns all matches regardless of count |
| Cap is 0 | edge | `test_search_source_symbols_cap_zero_returns_empty` | Zero cap returns empty vec immediately |
| Off-by-one boundary: cap-1 | adversarial | `test_search_source_symbols_cap_boundary_minus_one` | (Bounds/overflow class) cap=200 with 199 matches returns 199 exactly |
| Off-by-one boundary: cap+1 | adversarial | `test_search_source_symbols_cap_boundary_plus_one` | (Bounds/overflow class) cap=200 with 201 matches returns 200 exactly, not 201 |
| Generated symbols with cap | positive | `test_search_generated_symbols_respects_cap` | Generated symbol search also respects cap: 1000 generated with cap=200 returns 200 |
| Combined source + generated with cap | positive | `test_workspace_symbol_combined_sources_respects_cap` | Combined search (source + generated) respects overall cap; doesn't combine and then cap |
| Signature help (method lookup) with no cap | positive | `test_signature_help_search_no_cap_returns_all` | Signature help path (None cap) returns all candidates; no truncation |
| Large workspace (10k symbols) latency | adversarial | `bench_search_source_symbols_10k_symbols_latency` | (Performance/measurement class) cap=200 on 10k symbol index completes in <5ms |
| Determinism under cap | adversarial | `test_search_source_symbols_capped_results_deterministic` | (ID/reference-space collision class) Same query+cap always returns same symbols in same order (or at least same set) |

## §Blast-Radius

| Consumer | Crate | Dependency type | Impact | Required update |
|---|---|---|---|---|
| `handle_workspace_symbol` | perl-lsp-rs | direct call (line 290, 337) | high — two call sites updated to pass cap | Update both calls to pass `Some(cap)` instead of uncapped |
| `search_source_symbols` test callers | perl-workspace | direct call (lines 4710, 4715, 4779) | low — internal tests | Update test calls to pass `None` as cap parameter |
| signature_help method lookup | perl-lsp-rs | direct call (line 890) | low — method resolution unchanged | Update call to pass `None` (no cap for signature help) |
| `search_generated_workspace_symbols` | perl-lsp-rs | direct call (line 291) | high — workspace/symbol response depends on it | Update to pass `Some(cap)` to respect workspace symbol cap |

**Must-not-touch boundary:**
- `crates/perl-workspace/src/workspace/document_store.rs` — document lifecycle unrelated to symbol search
- `crates/perl-lsp-rs-core/src/runtime/limits/mod.rs` — workspace_symbol_cap() function (read-only)
- Parser (perl-parser, perl-parser-core, perl-lexer) — symbol extraction unchanged
- Protocol types (lsp-types, JsonRpc types) — no wire format changes
