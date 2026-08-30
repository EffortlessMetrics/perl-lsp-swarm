# Acceptance — #9378 full-sync UTF-16 initialize/session contract

Typed proof rows. Each row names the falsifying test(s). Pointers re-resolved
against head tree after review 5059982819 (Finding 2); the post-rejection
lifecycle rows reference the review Finding 1 repair tests.

| Row | Law | Falsified by |
| --- | --- | --- |
| LSP-FS16-001 | Sync kind and wire encoding form one immutable session contract accepted together; no caller can set FULL, UTF-16, or another encoding independently after acceptance. | `accepted_values_are_the_only_constructible_contract_members` (unit: only constructible members, no setters); runtime `second_initialize_cannot_replace_accepted_contract` (contract digest unchanged) |
| LSP-FS16-002 | Absent offer (and JSON null) selects UTF-16 by protocol default. | `absent_offer_selects_utf16_by_protocol_default`; runtime `initialize_absent_null_and_empty_offers_default_to_utf16_with_distinct_reasons` |
| LSP-FS16-003 | Present offer containing UTF-16 selects UTF-16 (first-supported pick does not exist; UTF-16 is the only selectable member). | `offer_matrix_selects_utf16_only_from_offered_or_defaulted_members` (matrix: `[utf-16]`, `[utf-8,utf-16]`, `[utf-32,utf-16]`, duplicates, unknown+utf-16) + `unknown_entries_are_retained_in_the_receipt_without_blocking_selection`; runtime `initialize_offer_containing_utf16_accepts_and_stores_contract` |
| LSP-FS16-004 | Present nonempty offer with no UTF-16 fails typed initialize before any accepted session/capability mutation, and the rejection stays rejected at the lifecycle layer. | `nonempty_no_common_offers_fail_typed` (unit: utf-8-only, utf-32-only, mixed no-common); runtime `initialize_no_common_offer_fails_before_any_state_mutation` (negative control: restoring the old fallback turns the focused gate red); wire fail-closed regressions `initialize_rejection_then_initialized_notification_does_not_activate_server` and `initialize_rejection_then_plain_request_does_not_auto_initialize` (review 5059982819 Finding 1) |
| LSP-FS16-005 | Empty, malformed, null, and wrong-type offers have explicit, distinct, pinned dispositions. | unit `empty_offer_has_its_own_explicit_disposition`, `json_null_offer_is_recorded_distinctly_from_absence`, `malformed_offers_fail_typed`, `rejections_map_to_typed_invalid_params_errors`; runtime `initialize_absent_null_and_empty_offers_default_to_utf16_with_distinct_reasons`, `initialize_malformed_offers_fail_typed` (absent/null → default; empty → default with `offer-empty` reason, serialized from `Utf16SelectionReason::OfferEmpty`; malformed/non-array/non-string entries → typed -32602 failure) |
| LSP-FS16-006 | `InitializeResult` and the stored session derive from the same accepted value; divergence is a typed failure, not drift. | `response_must_match_the_accepted_contract` (unit: agreement verified, divergence refused as typed failure) |
| LSP-FS16-007 | Rejected initialize publishes no partial capability/session/workspace state. | `initialize_no_common_offer_fails_before_any_state_mutation` (caps default, no folders, no root, no contract, not initialized) |
| LSP-FS16-008 | Repeated initialize cannot replace or partially alter the accepted contract. | `second_initialize_cannot_replace_accepted_contract` (different offer; error -32600; contract digest unchanged) |
| LSP-FS16-009 | Later mutation/range consumers receive this contract, not a free-standing negotiated value. | parity surfaces read the accepted contract via `accepted_encoding_from` in `effective_surface_parity_tests.rs` (`minimal_client_surface_matches`, `vscode_like_client_surface_matches`, `opencode_push_retention_surface_matches`); the pull projection reads `accepted_text_sync_session()` in `runtime/diagnostics.rs::pull_position_encoding`; `client_capabilities` no longer carries a position-encoding field (compile-level absence) |
| LSP-FS16-010 | Bounded evidence retains session identity, offer class and offered values, selection, sync kind, encoding, response digest, stored digest, terminal outcome. | `evidence_projection_agrees_with_contract_and_response_digest` |
| LSP-FS16-011 | Contract success cannot promote process/editor/installed support claims. | claim boundary: initialize/session authority only; #9380/#9383/#9386 and later leaves own mutation, range refusal, outgoing UTF-16 closure, exact-process and installed-VSIX proof |
| LSP-FS16-012 | Production activation remains conditional on #8129 selecting branch B. | #8129 ruling recorded 2026-08-29 (`full_document_utf16`, `selected_for_implementation`); release claim ceiling: full-document sync, UTF-16 wire encoding only |

## Proof commands

```bash
cargo test -p perl-lsp-rs --lib --all-targets --locked -- session_contract
cargo test -p perl-lsp-rs --lib --all-targets --locked -- lifecycle::capabilities
cargo test -p perl-lsp-rs --all-targets --locked -- lsp_3_17_lifecycle
cargo test -p perl-lsp-rs --all-targets --locked -- final_surface_census
cargo test -p perl-lsp-rs-core --lib --locked -- final_surface_inventory
cargo test -p perl-lsp-rs --all-targets --locked -- effective_surface_parity
```

Red-then-green: the offer-matrix and no-common tests were added against the
unwired model first and observed red on the old fallback/negotiation
behavior, then observed green after the transaction cutover. The review
5059982819 Finding 1 regressions (LSP-FS16-004 wire rows) were observed red
on the pre-fix head `6ab3268d26` (both sequences served requests) and green
after the classify-before-CAS / accepted-contract-gate repair.
