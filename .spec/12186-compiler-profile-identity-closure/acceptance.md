# Acceptance: #12186

| Obligation | Evidence |
| --- | --- |
| One stable in-memory profile/row model exists | `xtask/src/compiler_profile_contract.rs`: `CompilerProfileDefinition`, `CompilerProfileRow`, identity newtypes with validating constructors |
| Required/conditional/optional/unsupported/not-applicable semantics are exhaustive | `RowDisposition` closed 5-variant enum; `closure_required_rows_are_conjunctive_and_dispositions_closed` |
| Imports preserve exact lower-profile identity, rows, limitations and ceilings | `CompilerProfileImport` (id + version + sha256 semantic digest) and `CompilerProfileDefinition::verify_import_closure`; `falsifier_11_import_closure_preserves_rows_and_limitations` |
| Independent compiler/product/evidence/stage/operational dimensions cannot cross-satisfy through constructors or validation | `ClaimFamily` (13), `ProofClass` (4), `SourceTier` (5) closed enums; `falsifier_04_claim_families_cannot_cross_satisfy`, `falsifier_05_oracle_agreement_cannot_satisfy_eir_mechanism`, `falsifier_07_stages_cannot_collapse`, `falsifier_10_oracle_cold_work_is_not_production_work`, `legacy_exit_axes_are_independent` |
| Every row can carry exact subject, evidence, completeness, work, limitation, exit, owner, invalidation and claim-ceiling data | `CompilerProfileRow` non-optional fields + `CompilerProfileRow::validate`; `falsifier_14_ceiling_exit_owner_and_invalidation_are_mandatory` |
| Source/process/package/install/client and gold/oracle/EIR distinctions are representable without string conventions | `SourceTier` 5-variant and `ProofClass` 4-variant closed enums used by fixtures; `falsifier_07`, `falsifier_05` |
| Deterministic semantic fingerprints are independent of insertion order | `canonical_semantic_text` sorts all order-insensitive collections; `falsifier_12_row_order_cannot_change_identity` |
| The four initial profile shapes are representable without implying live status or hard-coding the final inventory | `shape_fixtures::{compiler_local_lexical_v1, compiler_static_project_v1, compiler_bounded_execution_v1, compiler_maintained_code_intelligence_v1}`; `shape_fixtures_are_representable_and_form_a_closed_import_chain` |
| The successor initial-row inventory can instantiate the model without a second type vocabulary | `successor_inventory_can_instantiate_without_a_second_vocabulary` builds and closes a profile from public constructors only |
| No repository manifest parser, receipt adapter, evaluator, command, status, support or release behavior lands | Module exports types, constructors, validators, fingerprints, fixtures only; no serde derives, no CLI wiring, no evaluation or satisfaction function; `falsifier_13_no_scalar_aggregate_figure_exists`, `falsifier_15_profile_evidence_confers_no_support_release_authority` |
