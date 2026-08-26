# Acceptance: iterative AST read traversal (#8867)

| Row | Proposition | Proof |
| --- | --- | --- |
| READ-001 | Depth 513+ chain count is exact, not a 512-ceiling usize | `chain_past_legacy_ceiling_returns_exact_count_not_truncated_512` |
| READ-002 | 50k count and deepest lookup complete on a 256 KiB stack | `fifty_thousand_node_count_and_lookup_are_exact_on_small_stack` |
| READ-003 | Later shallow overlap cannot beat a deeper match | `later_shallow_overlap_cannot_beat_deeper_match` |
| READ-004 | Equal-depth overlap follows canonical visit order | `equal_depth_overlap_keeps_earliest_canonical_path` |
| READ-005 | Omitting a #8424 field fails representative exact counts | `omitted_optional_field_fails_representative_exact_counts`; `every_populated_fixture_count_matches_visit_table_walk`; `cursor_dfs_fields_match_visit_table_without_a_second_match` |
| READ-006 | Bounded count/lookup expose Truncated, not usize/Some | `bounded_count_must_not_return_ordinary_usize_after_truncation`; `bounded_lookup_must_not_return_ordinary_some_after_truncation` |
| READ-007 | Half-open containment and zero-width recovery are preserved | `half_open_containment_and_zero_width_are_preserved`; `unicode_byte_offsets_stay_half_open` |
| READ-008 | Wide `Program` count/lookup is exact and does not restart the visit table per sibling | `wide_program_count_is_linear_in_statement_count`; `wide_program_loads_children_once_and_counts_exactly` |
| READ-009 | A child whose span lies outside the walk root cannot match | `child_outside_root_span_cannot_match` |
| READ-010 | `AstReadPathStep` Ord agrees with Eq when ordinals match | `path_step_ord_agrees_with_eq_when_ordinals_match` |

## Oracle notes

READ-005 compares the cursor against the same #8424 visit table
(`for_each_child` / `for_each_child_with_field`). That proves this crate did
not copy a second child-match table. Visit-table completeness itself remains
#8424's claim.

## Mutation controls

- recursive `MAX_AST_DEPTH` return of `1` / `Some(self)` → READ-001, READ-002, READ-006
- last-writer child assignment → READ-003, READ-004
- second child match omitting an optional field → READ-005
- `match bounded { Complete(v) \| Truncated { partial } => partial }` → READ-006
- `mem::forget` as leak-free proof → 50k tests drop the tree
- `nth_child` restarting the visit table from 0 on every sibling → READ-008
- descending into a child whose span is outside the root → READ-009
- `Ord` on sibling ordinal only → READ-010
