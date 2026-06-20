# Acceptance Criteria: #1658 — perf(lsp): references request spawns full-workspace text scan at request time

## §Behavior

| Input / Condition | Expected Result | Notes |
|---|---|---|
| References request with index ready, symbol found by index | Return index results only; do NOT iterate documents for text search | Latency bound by index lookup, not O(N) document iteration |
| References request with index ready, symbol NOT found by index | Return index results (empty if none); do NOT perform fallback text scan | Matches LSP semantics: if index has no result, it is correct |
| References request with index in Partial mode, results found | Return partial index results; use same-file fallback only | No full-workspace scan occurs |
| References request with index in Partial mode, results NOT found | Use same-file semantic analyzer only; do NOT scan other documents | Single-file scope, bounded latency |
| References request with index unavailable (None mode) | Use same-file semantic analyzer only | Single-file scope, no index access |
| Same-file references on symbol with no cross-file results | Return same-file results from semantic analyzer | Fallback path works correctly |
| Symbol with qualified name (e.g., `Package::sub`) | Return index results for qualified symbol; do NOT fall back to text scan | Qualified-name regex extracts the symbol; index lookup resolves it |
| Deadline exceeded during index lookup | Return partial results from what was indexed so far | Deadline applies only to index operations, not text fallback |

**All tests pass:** `cargo test -p perl-lsp-rs -- --test-threads=2`
**No clippy warnings:** `cargo clippy -p perl-lsp-rs`
**Formatted:** `cargo xtask fmt`

## §Hazards

| Class | Invariant | Surface (file:fn) | Required adversarial test |
|---|---|---|---|
| **LSP-1: Protocol-safety (response completeness)** | textDocument/references response format is always valid LSP Location[] (uri + range) or null | `references.rs:handle_references_inner()` | `test_references_response_format_valid` — mock index, verify `json!()` output matches LSP Location schema |
| **LSP-2: Index-correctness boundary** | If index returns a result, that result is correct (index completeness is out of scope for this fix) | `references.rs:handle_references_inner()` (lines 245–251, 367–384) | `test_references_trusts_index_results` — verify index results are returned unchanged without text-based validation |
| **LSP-3: Degraded-mode behavior** | Partial and None modes fall through to same-file analysis without attempting full-workspace scan | `references.rs:handle_references_inner()` (lines 531–587) | `test_references_partial_mode_no_workspace_scan`, `test_references_none_mode_uses_same_file` — mock Partial/None modes, verify no document iteration |
| **LSP-4: Request-time latency** | Request latency is bounded by index lookup time, not O(N) document iteration | `references.rs:handle_references_inner()` (entire function) | `test_references_latency_not_quadratic` — measure elapsed time for 100+ open documents, confirm O(1) to O(log N) index lookup, not O(N) scan |
| **Cross-subsystem-1: Test-encodes-the-bug** | Removed fallback text scan is not re-introduced by accident in future PRs | `references.rs:handle_references_inner()` (lines 279–340 and 453–523, now deleted) | `test_references_no_documents_snapshot_in_full_mode` — use Grep to verify `docs_snapshot` does not appear in IndexAccessMode::Full branch; add to CI as regression check |
| **Cross-subsystem-2: Index-gap exposure** | If the index misses references that the text scan was silently masking, removing the scan exposes the gap and allows follow-up issue to fix the index | `references.rs:handle_references_inner()` | Observational: monitor user reports of "missing references" in workspace-level queries after this PR merges; file #1660 or similar if reports occur |

**Subsystem-specific defaults consulted:** docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md — LSP (LSP-1 through LSP-4 selected and adapted)

## §Contracts

| Contract | Source document + section | How this change satisfies or extends it |
|---|---|---|
| **CLAUDE.md request-time scan policy** | CLAUDE.md: "CLAUDE.md forbids request-time scans" | This fix eliminates the request-time full-workspace scan fallback, bringing `handle_references_inner` into full compliance with the policy. |
| **LSP textDocument/references spec** | LSP Specification: `textDocument/references` | No protocol change; response format remains unchanged (Location[] or null). The handler correctly returns references found by the index without performing request-time scans. |
| **Index lifecycle semantics** | PARSER_CONTRACTS.md (if applicable) or internal docs/reference/ORCHESTRATION_DOCTRINE.md | The handler uses the index as the authoritative source (IndexAccessMode::Full uses `index.find_refs()` and `index.find_references()`); degraded modes fall through to same-file analysis, respecting the index state machine. |
| **Deadline enforcement** | docs/reference/LSP_IMPLEMENTATION_GUIDE.md (if exists) or internal patterns | Deadlines are removed as part of the fallback elimination. The index-backed path has no deadline checks (index lookups are pre-computed and cached). |

## §API-Shape

| Item | Kind | Signature / Range | Dup-risk (grep result) | Caller count |
|---|---|---|---|---|
| N/A — No new public API | — | — | — | — |

**Justification:** This change is a pure deletion of implementation details (fallback text-scan code paths) within the existing `handle_references_inner()` function. No new function, struct, enum, or public method is added. The function signature of `handle_references_inner()` remains unchanged.

## §Test-Grid

| Scenario | Kind | Test name | Invariant discharged |
|---|---|---|---|
| Index returns results with full mode ready | positive | `test_references_returns_index_results_in_full_mode` | Index results are correctly returned as LSP Locations |
| Index returns empty with full mode ready | positive | `test_references_empty_index_returns_empty_with_full_mode` | No panic on empty index result; returns `[]` |
| No documents snapshot created in full mode | negative | `test_references_no_documents_snapshot_iteration_in_full_mode` | Verify `documents.iter().map(...).collect()` pattern does NOT appear in IndexAccessMode::Full branch (grep `docs_snapshot` in the Full arm) |
| Partial mode falls through to same-file | positive | `test_references_partial_mode_uses_same_file_fallback` | Partial mode does not attempt workspace scan; uses same-file analysis |
| None mode uses same-file analysis | positive | `test_references_none_mode_uses_same_file_analysis` | No index access; same-file semantic analyzer provides results |
| Qualified-name symbol (Package::sub) with index | positive | `test_references_qualified_name_index_lookup` | Qualified-name regex extracts symbol; index lookup resolves correctly |
| Qualified-name symbol without index results | positive | `test_references_qualified_name_no_results` | No fallback text scan; returns empty or index results only |
| LSP response format validity | negative | `test_references_response_always_valid_lsp_location_format` | Response matches LSP Location[] schema (uri, range with line/character) |
| Large workspace (100+ files) latency | adversarial | `test_references_latency_constant_with_many_documents` | Elapsed time is O(log N) or better (index lookup), not O(N) (text scan) |
| Deadline exceeded during index lookup | adversarial | `test_references_deadline_expired_returns_partial_results` | Deadline check (if any) applies to index operations; partial results returned without text scan |

**Test file location:** `crates/perl-lsp-rs/tests/integration_references.rs` (or inline in `references.rs` as `#[cfg(test)]` module)

## §Blast-Radius

| Consumer | Crate | Dependency type | Impact | Required update |
|---|---|---|---|---|
| `on_references_document_highlight()` | perl-lsp-rs | Direct: calls `handle_references()` → `handle_references_inner()` | None — function signature unchanged; performance improves (no fallback scan) | None |
| `on_references()` legacy handler | perl-lsp-rs | Direct: standalone handler, independent of `handle_references_inner()` | None — separate code path; intentionally does full-workspace scan | None |
| LSP `textDocument/references` protocol handler | perl-lsp-rs core server loop | Indirect: marshaled by protocol dispatcher | None — response format unchanged | None |
| Workspace index (`perl-workspace`) | perl-workspace | Indirect: `handle_references_inner` uses `index.find_refs()` and `index.find_references()` | None — no index API changes; index completeness gaps (if any) become visible post-merge | File follow-up issue if users report missing references |
| Semantic analyzer (`perl-semantic-analyzer`) | perl-semantic-analyzer | Indirect: same-file fallback uses `SemanticAnalyzer::analyze()` and `find_all_references()` | None — no API changes; fallback is still used in Partial/None modes | None |

**Must-not-touch boundary:**
- `crates/perl-workspace/src/` — Index API and behavior unchanged
- `crates/perl-semantic-analyzer/src/` — Semantic analysis unchanged
- `crates/perl-lsp-rs/src/runtime/language/references.rs:on_references()` — Legacy handler kept intentionally (different design, known performance characteristics)
- `.spec/`, `docs/`, `README.md` — No documentation files modified
- All other crates and test suites — Fallback to their own testing

**Observables post-merge:**
- If users report "missing references" in workspace-level queries, this indicates the removed fallback was masking index gaps. File follow-up issue (#1660 or similar) to improve index completeness for dynamic references.
- Monitor LSP request latency in telemetry for textDocument/references before and after merge to confirm O(N) scan is eliminated.
