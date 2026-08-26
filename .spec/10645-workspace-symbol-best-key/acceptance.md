# Acceptance — #10645 retain the best matched key for each canonical row

Required invariant:

```text
best_match(row, query) == max(match(query, key) for every searchable key owned by row)
```

A row is absent only when every admitted key returns `None`.

## Stable rows → proofs

| Row | Proof |
| --- | --- |
| WS-BEST-001 bare exact replaces qualified substring for one row | `ws_best_001_bare_exact_beats_qualified_substring_under_both_orders` (selector), `ws_best_bare_exact_survives_qualified_substring_across_hash_seeds` (index metamorphic, fresh-thread seeds) |
| WS-BEST-002 qualified exact replaces weaker bare evidence | `ws_best_002_qualified_exact_beats_weaker_bare_match` |
| WS-BEST-003 legacy separator alias deterministic winner | `ws_best_003_legacy_separator_alias_tie_is_deterministic` (`Package'run` vs `Package::run`, both input orders) |
| WS-BEST-004 browse disposition per row, alias-count independent | `ws_best_004_browse_winner_is_alias_count_and_order_independent` |
| WS-BEST-005 permutation invariance (key/map/row/seed) | proptests `ws_best_prop_*` + index metamorphic test |
| WS-BEST-006 distinct projections sharing geometry stay distinct | `ws_best_006_distinct_projection_rows_sharing_anchor_stay_distinct` |
| WS-BEST-007 same-looking rows across roots/sources stay distinct | `ws_best_007_same_name_rows_in_two_roots_keep_independent_evidence` |
| WS-BEST-008 profile digest mismatch refuses mixed evidence | `ws_best_008_profile_mismatch_evidence_is_refused_not_mixed` |
| WS-BEST-009 future profile versions reuse one aggregator | digest-guard design + `ws_best_008`; no tier/version branching exists in aggregator |
| WS-BEST-010 complete/accelerated parity of winner/evidence | single accumulator consumed by both tiers of one request; #8262 differential corpus unchanged and green |
| WS-BEST-011 lifecycle retires keys/winner atomically | existing parity/update/remove suites + `ws_best_011_document_replacement_retires_old_keys` |
| WS-BEST-012 no cap before per-row best-key selection | cap applied after aggregation; `ws_best_012_cap_cannot_preserve_weaker_row_over_exact_row` |
| WS-BEST-013 #10642-consumable handoff without query reconstruction | typed `BestWorkspaceSymbolRowMatch` + receipt API; no LSP types involved |

## Negative controls (mutations must fail)

M1 restores early `seen.insert((uri, start_byte))` → banned-pattern
architecture gate `cargo xtask check-workspace-symbol-best-key` +
metamorphic seed test fail.
M2/M3 first/last alias wins → order-permutation selector tests/proptests.
M4 key/map/order changes winner or result order → total-order sort +
permutation tests.
M5 qualified substring kept over bare exact → WS-BEST-001.
M6 geometry collapse of distinct projections → WS-BEST-006.
M7 cross-root merge → WS-BEST-007.
M8 row retained with all-None keys → rows materialize only from admitted
keys (selector returns None); asserted in WS-BEST-001 fixtures.
M9 cap before best-key selection → WS-BEST-012.
M10 stale alias after replacement → WS-BEST-011.
M11 affinity used as semantic/edit identity → evidence carried alongside
payload only; no identity field consumes it (review-checked).
M12 local comparator reconstruction → aggregator contains no tier scoring;
only `compare()` + role ordinal; architecture gate bans new u8 tier tables.
M13 rejects later Q02/Q03 evidence version → genericity: no version/tier
branching; mismatch refusal path is the only version-sensitive branch.
M14 mixes digests within a row → WS-BEST-008 refusal.
M15 missing instrumentation reported as zero/pass → counters are
`Option<u64>`; unset stays `None` (`not_proven`).
