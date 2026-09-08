# Acceptance: #12188

| Obligation | Evidence |
| --- | --- |
| One versioned normalized observation envelope exists | `xtask/src/compiler_profile_observation.rs`: `CompilerProfileObservationV1` with `identity()` (sha256 over `canonical_semantic_text`) |
| Product, instrument, currentness, completeness, work, limitation, and claim-ceiling states remain independent | Independent closed fields on the envelope; `acceptance_source_vocabulary_is_preserved_not_flattened`, `falsifier_01_complete_red_observation_cannot_become_pass`, `falsifier_10_instrument_and_zero_work_states_cannot_collapse` |
| One deterministic registry owns adapter identity, source schema ranges, lossiness, and allowed observation classes | `ObservationAdapterRegistry` + `ObservationAdapterDescriptor`; `falsifier_11_registry_order_cannot_change_selection_or_bytes` |
| Unknown/ambiguous/overlapping adapters fail closed | `select_adapter` fails closed; `register` rejects ambiguous overlap without `supersedes`; `falsifier_05_unknown_or_future_source_schema_fails_closed`, `falsifier_07_overlapping_adapters_require_explicit_migration` |
| Source receipt semantics can only be preserved or narrowed | `ObservedClaimCeiling::narrows_or_matches` enforced in descriptor and registry validation; `falsifier_06_adapter_cannot_strengthen_source_claim_ceiling` |
| Exact candidate/producer/subject dimensions remain explicit and private-safe | `ensure_private_safe` on every free-text field; `CandidateSubjectIdentity` with explicit `not_proven`; `falsifier_09_missing_subject_identity_stays_explicit_not_proven`, `falsifier_12_private_payload_cannot_leak_into_envelope` |
| No concrete receipt adapter, manifest evaluation, proof execution, status, compiler, provider, support, or release behavior lands | Registry ships empty; synthetic fixtures only; no serde, no CLI, no evaluation function; `acceptance_registry_ships_empty_and_no_evaluation_lands` |
| The landed #12186 vocabulary is composed, not duplicated | `ObservationClass` wraps `ClaimFamily`/`ProofClass`; `ObservedClaimCeiling` wraps `ClaimCeiling`; `InvalidationEvidence` reuses `InvalidationInput`; `acceptance_successor_lanes_can_consume_the_public_surface` |
