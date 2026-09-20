# Acceptance — #9378 full-sync UTF-16 initialize/session contract

Typed proof rows. Every row names the live falsifier that owns the law after
#14301 reconciled mandatory UTF-16 fallback with one-shot initialize authority.

| Row | Law | Falsified by |
| --- | --- | --- |
| LSP-FS16-001 | Sync kind and wire encoding form one immutable session contract accepted together; no caller can set FULL, UTF-16, or another encoding independently after acceptance. | `accepted_values_are_the_only_constructible_contract_members`; runtime `second_initialize_cannot_replace_accepted_contract` |
| LSP-FS16-002 | Absent offer and JSON null select UTF-16 by protocol default; a present empty list selects the same contract with its own reason. | `absent_offer_selects_utf16_by_protocol_default`, `json_null_offer_is_recorded_distinctly_from_absence`, `empty_offer_has_its_own_explicit_disposition`; runtime `initialize_absent_null_and_empty_offers_default_to_utf16_with_distinct_reasons` |
| LSP-FS16-003 | Every valid string list selects FULL + UTF-16. Lists containing UTF-16 record `client-offered-utf16`; nonempty valid lists omitting it record `mandatory-utf16-fallback`. | `valid_offer_matrix_always_selects_full_utf16`, `unknown_entries_are_retained_without_changing_mandatory_selection`; runtime `initialize_offer_containing_utf16_accepts_and_stores_contract`, `initialize_valid_offer_omitting_utf16_uses_mandatory_fallback` |
| LSP-FS16-004 | Non-array offers and arrays containing non-string entries fail typed `InvalidParams` before any accepted session or lifecycle side effect. | `malformed_offers_fail_typed`, `malformed_rejection_maps_to_typed_invalid_params_error`; runtime `initialize_malformed_offers_fail_typed`; wire malformed-first regressions in `lsp_3_17_lifecycle_tests.rs` |
| LSP-FS16-005 | The first initialize request, accepted or rejected, consumes one-shot attempt authority before parameter classification. Every later initialize returns `InvalidRequest` before its own parameters can change the error class. | runtime `malformed_first_initialize_consumes_one_shot_authority`, `second_initialize_cannot_replace_accepted_contract`, `handle_initialize_exact_error_variant` |
| LSP-FS16-006 | Concurrent initialize requests have exactly one attempt owner. Every loser returns `InvalidRequest`; no mixed or replaced accepted state is possible. | runtime `concurrent_initialize_attempts_have_exactly_one_owner` |
| LSP-FS16-007 | Attempted-but-unaccepted state is not serving authority. It cannot complete initialization, serve requests, intercept formatting, mutate documents, start watchers/index/bootstrap, or emit readiness. | `initialization_accepted()` is the single serving/completion predicate; wire rejection-followup tests in `lsp_3_17_lifecycle_tests.rs` |
| LSP-FS16-008 | `InitializeResult` and stored session derive from the same accepted value; divergence is a typed internal failure, not drift. | `response_must_match_the_accepted_contract`; runtime response/state digest checks |
| LSP-FS16-009 | Repeated initialize cannot replace or partially alter an accepted contract, including when the second request is malformed. | `second_initialize_cannot_replace_accepted_contract` |
| LSP-FS16-010 | Later mutation/range consumers receive this contract, not a free-standing negotiated value. | parity surfaces read `accepted_text_sync_session()`; pull diagnostics use the accepted contract; `ClientCapabilities.position_encoding` is absent |
| LSP-FS16-011 | Bounded evidence retains session identity, offer class and entries, selection reason, sync kind, encoding, response digest, stored digest, and terminal outcome. | `evidence_projection_agrees_with_contract_and_response_digest` |
| LSP-FS16-012 | Contract success cannot promote mutation, editor, installed, or public support claims. Production use remains bounded to #8129's selected `full_document_utf16` branch. | claim boundary and downstream owners #9380, #9382, #9383, #9386, #9388, #9389 |

## Proof commands

```bash
cargo test -p perl-lsp-rs --lib --locked -- session_contract
cargo test -p perl-lsp-rs --lib --locked -- lifecycle::capabilities::tests::initialize
cargo test -p perl-lsp-rs --lib --locked -- dispatch
cargo test -p perl-lsp-rs --test lsp_3_17_lifecycle_tests --locked
cargo clippy -p perl-lsp-rs --all-targets --locked -- -D warnings
cargo fmt -p perl-lsp-rs -- --check
git diff --check
```

The discriminating mutations are: classify before attempt ownership; restore a
valid-list `no-common-encoding` rejection; gate serving on attempted state;
classify a malformed second request before the one-shot guard; or allow two
concurrent attempts to install or classify as first.
