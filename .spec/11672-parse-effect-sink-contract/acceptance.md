# Acceptance: #11672

| Obligation | Evidence |
| --- | --- |
| One closed common effect-commit outcome vocabulary exists | `ParseEffectCommitOutcomeV1` (13 variants) with closed-partition test `parse_effect_sink_outcome_vocabulary_closed_partition` |
| Stale/superseded tickets have typed non-application outcomes, not silent success | `RejectedStaleTicket`, `RejectedWrongDocumentInstance`, `SupersededBeforeMutation`, et al. classified `is_non_application()` and excluded from `is_committed()` by test |
| Missing/instrument-failed evidence cannot pass or disappear | `NotProven`, `InstrumentOrSchemaFailure`, `SinkUnavailable` are distinct classes; `!NotProven.is_committed()` asserted |
| One checked static inventory covers every parse-derived effect and direct/helper path | `parse_effect_sinks_v1` (14 rows) + call-site ledger over text_sync/symbols/diagnostics/semantic_tokens/mod sources with exact-count ratchets (`parse_effect_sink_call_site_ledger_matches_source`) |
| An effect emitted without an inventory row is structurally forced to register | Every registered needle has an exact per-file count ratchet plus a completeness sweep with counted exemptions; any new occurrence of a registered needle fails (`parse_effect_sink_call_site_ledger_matches_source` / `..._covers_registered_needles`) and every mutating row must own ≥1 registered site. Boundary: a brand-new mutation API spelling is not discoverable by this text-level instrument -- novel-name discovery stays review-owned until the focused children cut over to sink-local commits |
| Every row names one exact owner and one disposition | `parse_effect_sink_rows_have_exactly_one_owner_and_disposition`; unknown/duplicated rows fail `parse_effect_sink_ids_unique_and_stable_format` / authority checks |
| No duplicate mutation authority | Needle-level partition: one registered site maps to exactly one row; two rows can never claim the same registered mutation decision |
| Ticket inputs declared wherever a currentness comparison applies | `parse_effect_sink_ticket_fields_declared_for_governed_rows` requires instance+generation for helper-routed rows |
| Terminal/empty/failure clear-or-replace behavior explicit per sink | `parse_effect_sink_terminal_policy_total_per_class`: all 8 terminal classes covered; content stores must replace/clear/publish on current results — never keep stale state silently |
| Referenced existing owners are not reimplemented locally | `parse_effect_sink_external_owners_not_reimplemented_locally` forces external:/none boundaries citing the owning authority (#8619/#8642, #7309, #6729, #9162 train) |
| Compatibility adapters reported with exits, not final authority | `compat.legacy-generic-callback-helper` row + `parse_effect_sink_compat_adapters_have_exit_owner_in_source` (exit owner #7379; adapter present in source) |
| Generated projection deterministic and second-run clean | `parse_effect_sink_projection_second_run_clean` byte-compares regenerated markdown against committed `.spec/11672-parse-effect-sink-contract/inventory.md` |
| No runtime behavior change | Contract module is additive types + static tables only; no production call site modified; focused suite green without touching sink logic |
