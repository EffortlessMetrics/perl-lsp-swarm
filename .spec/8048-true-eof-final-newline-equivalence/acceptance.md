# Acceptance: #8048 shift-left equivalence seam

Each row binds one stable proposition to its discriminating executable proof.
Rows marked `offline` are proven entirely by this candidate's fixtures in
`crates/perl-lsp-perltidy/tests/`. Rows marked `deferred` require the byte-native
train (#10237/#10239/#10242) or geometry cutover (#11873) and are named so they
cannot be silently dropped from the issue's closeout.

| Row | Proposition | Proof | Status |
| --- | --- | --- | --- |
| FMT-FINAL-001 | All whole-document bytes are inside a replacement target reaching true EOF | `final_newline_policy_tests::str_lines_trailing_loss_cannot_return_as_terminal_authority`; `edit_application_equivalence_tests::true_eof_whole_document_edits_apply_byte_exactly` (hand-built ranges); terminated-source parity over produced edits deferred to #11873/#10239 | offline + deferred |
| FMT-FINAL-002 | Insert-only preserves existing complete terminal sequences | `insert_only_adds_one_sequence_only_when_none_exists` | offline |
| FMT-FINAL-003 | Trim-only retains one final sequence and removes only excess | `trim_only_retains_the_one_final_sequence_and_removes_only_excess` | offline |
| FMT-FINAL-004 | Both options produce exactly one accepted sequence | `both_options_produce_exactly_one_accepted_final_sequence` | offline |
| FMT-FINAL-005 | LF/CRLF/bare-CR are atomic and convention-aware | `trailing_run_scans_complete_sequences_atomically`; `insert_only_preserves_the_source_convention` | offline |
| FMT-FINAL-006 | Native plan and wire plan independently reproduce final bytes | `edit_application_equivalence_tests` oracle rows (UTF-16 and UTF-8-byte schemes); production-parity wiring deferred to #10239/#10242 | offline + deferred |
| FMT-FINAL-007 | Final evidence binds exact predecessor/plan/final bytes | `evidence_binds_to_the_final_returned_bytes_not_an_intermediate` | offline |
| FMT-FINAL-008 | Stale/invalid/conflicting/unrepresentable plans are non-applied, while distinct same-position insertions preserve LSP order | `reversed_ranges_are_rejected_not_clamped`; `unreachable_positions_are_rejected_not_clamped`; `mid_code_point_positions_are_rejected_in_both_encodings`; `adjacent_edits_are_allowed_but_overlap_is_rejected`; `distinct_zero_width_insertions_preserve_lsp_order` | offline |
| FMT-FINAL-009 | NoChange carries no edits and never looks applied | `no_byte_delta_is_classified_as_no_change_with_no_actions`; `empty_and_identity_edits_reproduce_exact_source` | offline |
| FMT-FINAL-010 | Manual/save/CLI/installed routes consume one newline policy | single-policy module exists for #10239/#10242 adoption; route parity receipts remain with the train | deferred |
| FMT-FINAL-011 | Proof oracle is independent of production mapper/range builders | oracle module shares no geometry code; `historical_last_content_line_geometry_is_detectably_wrong_through_the_oracle` reproduces the defect without production constructors | offline |
| FMT-FINAL-012 | #7138 localized plans preserve the same invariant | oracle accepts arbitrary non-overlapping edit sets; binding proof lands with #7138 | deferred |

## Mutation controls (must stay red if reintroduced)

- force-LF insertion into a CRLF-preserving source →
  `insert_only_preserves_the_source_convention`
- collapse of existing sequences under insert-only →
  `insert_only_adds_one_sequence_only_when_none_exists`
- removal of the one final newline under trim-only →
  `trim_only_retains_the_one_final_sequence_and_removes_only_excess`
- partial CRLF split / bare-CR residue reported as preserved →
  `evidence_reports_change_after_partial_crlf_splitting_and_conversion`
- evidence computed before final projection →
  `evidence_binds_to_the_final_returned_bytes_not_an_intermediate`
- `str::lines()` trailing-loss as whole-document authority →
  `str_lines_trailing_loss_cannot_return_as_terminal_authority`
- clamped invalid geometry → every rejection row above
