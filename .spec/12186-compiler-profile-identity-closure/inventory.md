# compiler_profile_contract model inventory (#12186)

Vocabulary owned by `xtask/src/compiler_profile_contract.rs`. This is a
hand-maintained projection of the type model and the falsifier coverage; the
module and its tests are the executable authority.

## Identity types

- `CompilerProfileId`, `CompilerProfileVersion` (`v`-prefixed),
  `CompilerProfileDigest` (64 lowercase hex sha256), `CompilerProfileRowId`,
  `SubjectRef`, `WorkScope` (non-empty; zero-work scope unconstructable).

## Closed dimensions

- `ClaimFamily` (13): parser_internal, provider, edit, project_world,
  execution, performance, exact_process, packaged, installed_host,
  actual_client, test_reachability, legacy_exit, public_claim.
- `ProofClass` (4): curated_expectation, real_perl_oracle, eir_mechanism,
  evaluated_work.
- `SourceTier` (5): source, exact_process, packaged, installed_host,
  actual_client.
- `RowDisposition` (5): required, conditional{trigger}, optional,
  unsupported{reason}, not_applicable{justification}.
- `ClaimCeiling` (3): observed_evidence, accepted_compatibility,
  bounded_public_claim; none confers support/release/publication authority.
- `CurrentnessRule` (4): source_locked, project_world_current,
  execution_bounded, host_observed.
- `CoverageRule` (3): exhaustive, bounded{boundary},
  explicitly_partial{remainder}.
- `InvalidationKind` (5): source_change, dependency_change,
  world_model_change, host_environment_change, oracle_change.
- `Obligation` (2): required, not_applicable (legacy-exit axes).

## Row/profile structure

- `CompilerProfileRow`: id, disposition, subject (`SubjectSelector`, 7 exact
  dimensions), evidence (`EvidenceRequirement`: family + tier + conjunctive
  non-empty proof-axis set), completeness, work (`WorkRequirement`:
  correctness / production / oracle-or-cold / performance-resource, scoped
  variants non-zero), limitations, legacy_exit (three independent
  obligations), ceiling, invalidation (non-empty), owner+wake event.
- `CompilerProfileImport`: exact lower profile id + version + semantic digest.
- `CompilerProfileDefinition`: id, version, purpose, change_reason, owner,
  imports, rows (order-insensitive), profile-level limitations.
  - `validate`: unique row/import/limitation ids, no self-import, per-row
    closure (unsupported/not-applicable rows cannot claim more than observed
    evidence; invalidation non-empty; payloads non-empty).
  - `verify_import_closure`: exact identity/version/digest match plus
    verbatim preservation of every imported row and limitation.
  - `canonical_semantic_text` / `semantic_fingerprint`: deterministic,
    insertion-order independent, sensitive to every semantic field.
  - `required_unconditional_row_ids`: the conjunctive unconditional required
    set; `conditional_row_triggers`: conditional rows with their triggers,
    resolved by the downstream evaluator; no aggregate roll-up figure exists.
  - `ClaimCeiling::strongest_claim`: inspectable per-ceiling claim data; no
    variant maps to support, release, or publication authority.

## Shape fixtures (representability only, not the checked inventory)

- `compiler_local_lexical.v1` — no imports; parser-internal + test-reachability rows.
- `compiler_static_project.v1` — imports local_lexical; adds project/world (multi-axis) + provider rows.
- `compiler_bounded_execution.v1` — imports static_project; adds execution (production work) + performance rows at exact-process tier.
- `compiler_maintained_code_intelligence.v1` — imports bounded_execution; adds edit-authorization (full legacy-exit), bounded public-claim, and an explicitly unsupported actual-client row.

## Falsifier coverage (issue #12186, 15 falsifiers)

| # | Falsifier | Pinning test |
| --- | --- | --- |
| 1 | local lexical pass stands in for stronger profile | `falsifier_01_local_lexical_cannot_stand_in_for_stronger_profile` |
| 2 | long-horizon work becomes prerequisite to bounded local profile | `falsifier_02_local_profile_has_no_long_horizon_prerequisites` |
| 3 | issue/PR/workflow state enters the evidence model | `falsifier_03_workflow_state_has_no_place_in_the_evidence_model` |
| 4 | parser proof satisfies provider/edit/installed-host proof | `falsifier_04_claim_families_cannot_cross_satisfy` |
| 5 | fixture replay/oracle agreement satisfies EIR mechanism/evaluation | `falsifier_05_oracle_agreement_cannot_satisfy_eir_mechanism` |
| 6 | source-locked debt typed as general semantic support | `falsifier_06_source_locked_debt_cannot_be_typed_as_general_support` |
| 7 | source/exact-process/package/install/client stages collapse | `falsifier_07_stages_cannot_collapse` |
| 8 | unsupported/not-proven required row disappears by omission | `falsifier_08_required_row_cannot_disappear_by_omission` |
| 9 | zero-work execution satisfies a required work row | `falsifier_09_zero_work_cannot_satisfy_a_work_row` |
| 10 | cold/oracle work typed as production work avoided | `falsifier_10_oracle_cold_work_is_not_production_work` |
| 11 | imported lower profile loses rows or limitations | `falsifier_11_import_closure_preserves_rows_and_limitations` |
| 12 | row ordering changes the profile fingerprint | `falsifier_12_row_order_cannot_change_identity` |
| 13 | scalar figure or aggregate percentage introduced | `falsifier_13_no_scalar_aggregate_figure_exists` |
| 14 | claim ceiling, legacy exit, owner or invalidation fields absent | `falsifier_14_ceiling_exit_owner_and_invalidation_are_mandatory` |
| 15 | support/release authority inferred from a profile result | `falsifier_15_profile_evidence_confers_no_support_release_authority` |

Additional closure tests: `closure_required_rows_are_conjunctive_and_dispositions_closed`,
`closure_identity_changes_with_any_semantic_row_field`,
`shape_fixtures_are_representable_and_form_a_closed_import_chain`,
`successor_inventory_can_instantiate_without_a_second_vocabulary`,
`legacy_exit_axes_are_independent`.
