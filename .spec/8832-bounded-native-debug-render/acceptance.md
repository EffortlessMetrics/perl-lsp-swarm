# Acceptance: bounded native debug rendering (#8832)

| Row | Proposition | Proof |
| --- | --- | --- |
| SEXP-001 | Small complete trees are byte-identical to the #8829 projection | `small_trees_match_8829_golden_bytes`; `every_representative_unbounded_render_matches_visit_order` |
| SEXP-002 | Node, depth, byte, and work limits trip independently at limit-1/limit/limit+1; node capacity is checked before descend so simultaneous node+depth exhaustion is `NodeLimit` and a rejected child charges no edge; a rejected descent charges no edge-work | `node_depth_byte_and_work_limits_trip_independently`; `node_limit_precedes_depth_and_does_not_charge_a_rejected_edge`; `rejected_descent_charges_no_edge_work` |
| SEXP-003 | Declared byte limit is never exceeded | `output_never_exceeds_declared_byte_limit` |
| SEXP-004 | 50,000-node chain completes on a 256 KiB stack without `mem::forget` | `fifty_thousand_node_chain_renders_on_small_stack` |
| SEXP-005 | Nested and concurrent renders do not share counters or depth budget | `nested_writer_callback_does_not_inherit_outer_budget`; `concurrent_renders_are_isolated` |
| SEXP-006 | Truncation is not a fake AST node | `truncation_is_not_a_fake_ast_node` |
| SEXP-007 | Incomplete debug output cannot satisfy #7045 or #8044 | `truncated_prefix_cannot_stand_in_for_ast_equality`; `incomplete_debug_output_cannot_satisfy_machine_output` |
| SEXP-008 | Writer failure is `InstrumentFailure`, not truncation or completeness | `writer_failure_is_instrument_failure` |
| SEXP-009 | `to_sexp()` cannot prove completeness; Display/render can stream | `to_sexp_string_cannot_prove_completeness`; `render_streams_without_requiring_an_intermediate_string` |
| SEXP-010 | `omitted` is not fabricated from unvisited state | `omitted_count_is_unknown_when_subtree_was_not_walked` |

## Mutation controls

- recursive `MAX_AST_DEPTH` returning `(depth_limit_exceeded)` as ordinary text → SEXP-004, SEXP-006
- thread-local depth inherited by nested calls → SEXP-005
- wrapping a writer and checking `String::len()` after `write_str` → SEXP-003
- descending (and charging an edge) before checking exhausted node capacity → SEXP-002
- charging edge-work for a descent that depth already forbids → SEXP-002
- `match result { Complete {..} \| Truncated {..} => treat as complete }` → SEXP-007, SEXP-008
- second NodeKind child-match table omitting a #8424 field → SEXP-001
- `mem::forget` as leak-free proof → SEXP-004
