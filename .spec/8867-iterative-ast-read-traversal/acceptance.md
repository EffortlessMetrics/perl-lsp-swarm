# Acceptance: iterative AST read traversal (#8867)

| Row | Proposition | Proof |
| --- | --- | --- |
| READ-001 | Depth 513+ chain count is exact, not a 512-ceiling usize | `chain_past_legacy_ceiling_returns_exact_count_not_truncated_512` |
| READ-002 | 50k count and deepest lookup complete on a 256 KiB stack | `fifty_thousand_node_count_and_lookup_are_exact_on_small_stack` |
| READ-003 | Later shallow overlap cannot beat a deeper match | `later_shallow_overlap_cannot_beat_deeper_match` |
| READ-004 | Equal-depth overlap follows canonical visit order | `equal_depth_overlap_keeps_earliest_canonical_path` |
| READ-005 | Omitting a #8424 field fails representative exact counts | `omitted_optional_field_fails_representative_exact_counts`; `every_populated_fixture_count_matches_visit_table_walk` |
| READ-006 | Bounded count/lookup expose Truncated, not usize/Some | `bounded_count_must_not_return_ordinary_usize_after_truncation`; `bounded_lookup_must_not_return_ordinary_some_after_truncation` |
| READ-007 | Half-open containment and zero-width recovery are preserved | `half_open_containment_and_zero_width_are_preserved`; `unicode_byte_offsets_stay_half_open` |

## Mutation controls

- recursive `MAX_AST_DEPTH` return of `1` / `Some(self)` → READ-001, READ-002, READ-006
- last-writer child assignment → READ-003, READ-004
- second child match omitting an optional field → READ-005
- `match bounded { Complete(v) \| Truncated { partial } => partial }` → READ-006
- `mem::forget` as leak-free proof → 50k tests drop the tree
