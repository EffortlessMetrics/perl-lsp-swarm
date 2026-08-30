# Acceptance — #9378 full-sync UTF-16 initialize/session contract

Typed proof rows. Each row names the falsifying test(s).

| Row | Law | Falsified by |
| --- | --- | --- |
| LSP-FS16-001 | Sync kind and wire encoding form one immutable session contract accepted together; no caller can set FULL, UTF-16, or another encoding independently after acceptance. | `session_contract` unit tests: single constructor, no setters, only constructible values; `text_sync_session_rejects_second_acceptance` |
| LSP-FS16-002 | Absent offer (and JSON null) selects UTF-16 by protocol default. | `initialize_without_offer_defaults_to_utf16` |
| LSP-FS16-003 | Present offer containing UTF-16 selects UTF-16 (first-supported pick does not exist; UTF-16 is the only selectable member). | `initialize_with_utf16_in_offer_selects_utf16` (matrix: `[utf-16]`, `[utf-8,utf-16]`, `[utf-32,utf-16]`, duplicates, unknown+utf-16) |
| LSP-FS16-004 | Present nonempty offer with no UTF-16 fails typed initialize before any accepted session/capability mutation. | `initialize_no_common_offer_fails_before_any_state_mutation`, `initialize_utf8_only_offer_fails`, `initialize_utf32_only_offer_fails`, negative control: restoring the old fallback turns the focused gate red |
| LSP-FS16-005 | Empty, malformed, null, and wrong-type offers have explicit, distinct, pinned dispositions. | `initialize_offer_dispositions_are_distinct` (absent/null → default; empty → default with `EmptyOffer` reason; malformed/non-array/non-string entries → typed -32602 failure) |
| LSP-FS16-006 | `InitializeResult` and the stored session derive from the same accepted value; divergence is a typed failure, not drift. | `initialize_response_matches_stored_contract`, `response_divergence_is_typed_failure` (unit-level verification seam) |
| LSP-FS16-007 | Rejected initialize publishes no partial capability/session/workspace state. | `initialize_no_common_offer_fails_before_any_state_mutation` (caps default, no folders, no root, no contract, not initialized) |
| LSP-FS16-008 | Repeated initialize cannot replace or partially alter the accepted contract. | `second_initialize_cannot_replace_accepted_contract` (different offer; error -32600; contract digest unchanged) |
| LSP-FS16-009 | Later mutation/range consumers receive this contract, not a free-standing negotiated value. | `pull_diagnostics_projection_reads_accepted_contract`; parity helper reads the accepted contract, not params; `client_capabilities` no longer carries a position-encoding field |
| LSP-FS16-010 | Bounded evidence retains session identity, offer class and offered values, selection, sync kind, encoding, response digest, stored digest, terminal outcome. | `session_evidence_agrees_with_response_and_contract` |
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
behavior, then observed green after the transaction cutover.
