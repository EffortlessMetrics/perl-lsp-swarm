# compiler_profile_initial_rows inventory (#12330)

Checked inventory owned by `xtask/src/compiler_profile_initial_rows.rs`. This
is a hand-maintained projection; the module and its tests are the executable
authority. All rows instantiate the #12186 vocabulary in
`xtask/src/compiler_profile_contract.rs` — there is no second type system.

## Profiles and import chain

- `compiler_local_lexical.v1` — 22 rows, no imports, no #8722 prerequisite.
- `compiler_static_project.v1` — imports local verbatim (id+version+digest);
  41 rows (22 imported + 19 own).
- `compiler_bounded_execution.v1` — imports project verbatim; 60 rows
  (41 imported + 19 own).
- `compiler_maintained_code_intelligence.v1` — imports execution verbatim;
  79 rows (60 imported + 19 own, one of them explicitly unsupported).

`initial_profiles()` returns the four checked definitions in import order and
is the single authority #12187 consumes without transcription.

## Own row IDs per profile

### compiler_local_lexical.v1 (22)

`lexical.candidate-toolchain-identity`, `lexical.observation-base-parse`,
`lexical.observation-base-compile`, `lexical.observation-comp-parse`,
`lexical.observation-comp-compile`, `lexical.observation-run-parse`,
`lexical.observation-run-compile`, `lexical.invocation-process-validity`,
`lexical.semantic-debt-retirement-accepted`,
`lexical.parser-generation-accepted`, `lexical.semantic-snapshot-accepted`,
`lexical.pir-lexical-contribution`,
`lexical.external-references-compiler-backed`,
`lexical.occurrence-denominator-complete`, `lexical.rename-authorization`,
`lexical.workspace-edit-application`,
`lexical.zero-request-time-compiler-construction`,
`lexical.zero-legacy-source-scan-work`,
`lexical.lifecycle-currentness-cleanup`,
`lexical.mutation-execution-required`, `lexical.exact-perllsp-process`,
`lexical.claim-ceiling`.

The six `lexical.observation-*` rows are owned by the #12291/#12139–#12141
bounded packet and name the bounded denominator in their coverage boundary,
including the rule that #8722 may later widen publication but cannot redefine
series, subject, or denominator.

### compiler_static_project.v1 (19 own)

`project.import-local-profile-exact`, `project.world-snapshot-accepted`,
`project.root-project-profile-source-generation-closure`,
`project.module-dependency-graph`, `project.scc-schedule`,
`project.private-implementation-transition`,
`project.public-interface-transition`,
`project.reverse-dependency-invalidation-closure`,
`project.stale-publication-rejection`,
`project.multi-root-close-reopen-currentness`,
`project.cross-file-definition`, `project.cross-file-references`,
`project.cross-file-rename-complete-or-refuse`,
`project.cross-file-edit-application`,
`project.representative-project-lifecycle`,
`project.cold-equivalence-correctness`, `project.reuse-recompute-work`,
`project.performance-resource-envelope`, `project.claim-ceiling`.

### compiler_bounded_execution.v1 (19 own)

`execution.executable-profile-identity`,
`execution.unsupported-fact-catalog-identity`,
`execution.package-subtable-effect-denominator`,
`execution.compiler-fact-eir-lowering`, `execution.eir-verification`,
`execution.bounded-deterministic-evaluation`,
`execution.hard-limit-resource-policy`, `execution.curated-gold-reviewed`,
`execution.hermetic-real-perl-oracle`, `execution.eir-gold-agreement`,
`execution.eir-oracle-agreement`,
`execution.selected-upstream-row-denominator`, `execution.nonzero-eir-work`,
`execution.nonzero-tap-work`, `execution.zero-legacy-scaffold-calls`,
`execution.no-project-execution-from-editor-requests`,
`execution.editor-runtime-dependency-false`,
`execution.dynamic-boundaries-explicit`, `execution.claim-ceiling`.

### compiler_maintained_code_intelligence.v1 (19 own)

`intelligence.import-lower-profile-identities-exact`,
`intelligence.upstream-series-denominator`,
`intelligence.selected-provider-refactor-rows`,
`intelligence.release-shaped-package-identity`,
`intelligence.contained-binary-process-identity`,
`intelligence.packaged-semantic-cells`,
`intelligence.manifest-selected-client-platform-identity`,
`intelligence.client-launches-exact-packaged-bytes`,
`intelligence.selected-client-application-cells`,
`intelligence.client-lifecycle-restart-currentness-cleanup`,
`intelligence.correctness-bound-work-envelope`,
`intelligence.latency-resource-thresholds-policy`,
`intelligence.target-route-nonzero-test-mutation-work`,
`intelligence.selected-legacy-replacement`,
`intelligence.old-path-absence-recurrence-proof`,
`intelligence.allowed-limitations-expected-failures`,
`intelligence.machine-public-claim-ceiling`,
`intelligence.support-release-authority-false`,
`intelligence.integrated-publication-8722` (explicitly **unsupported**:
#8722 is a later separately-owned row source, not a current accepted row).

## Pinned semantic digests

The tests pin each profile's `semantic_fingerprint` (sha256 over canonical
semantic text). Any semantic row change — proposition, subject, evidence,
work law, limitation, legacy exit, ceiling, owner, invalidation — fails the
pin and requires the #12186 version transition; row order and formatting
cannot change identity (`falsifier_12`).

- `compiler_local_lexical.v1`: `3436949225dfe7bdff85c480fd54eff0c1fb34abe52fd01fd430fccf2e2609a0`
- `compiler_static_project.v1`: `1beea837946b3e3a3a1a63db75488e049f58f24e8f8033f62be3fbdc173ebec6`
- `compiler_bounded_execution.v1`: `11a103ebeb44bf17f9eb59df8fc971df244a937c7eabdc0d0254f75a2a72202a`
- `compiler_maintained_code_intelligence.v1`: `db944c490af945348e99c352d4d2d894d2b66d2e1196c319980f18058bbb6ce0`

## Falsifier coverage (issue #12330, 14 falsifiers)

| # | Falsifier | Pinning test |
| --- | --- | --- |
| 1 | local lexical propositions collapse into #12079 pass | `falsifier_01_local_lexical_propositions_do_not_collapse` |
| 2 | #8722 becomes prerequisite to bounded local rows | `falsifier_02_integrated_publication_is_not_a_local_prerequisite` |
| 3 | #8722 redefines #12291 series/subject/denominator or fills a bounded field | `falsifier_03_integrated_publication_cannot_redefine_bounded_observations` |
| 4 | compiler-world/navigation/refactor collapse into one static row | `falsifier_04_world_navigation_and_refactor_stay_distinct` |
| 5 | gold/oracle/replay stands in for EIR mechanism or work | `falsifier_05_gold_oracle_cannot_stand_in_for_eir_mechanism` |
| 6 | package/install/client stages collapse | `falsifier_06_package_install_and_client_stages_stay_distinct` |
| 7 | performance or aggregate score replaces correctness | `falsifier_07_performance_cannot_replace_correctness` |
| 8 | failed/unsupported/not-proven denominator row omitted | `falsifier_08_denominator_and_unsupported_rows_are_explicit` |
| 9 | optional/required/limitation semantics change without version movement | `falsifier_09_disposition_semantics_require_version_movement` |
| 10 | lower profile imported by name only | `falsifier_10_import_by_name_only_fails_closure` |
| 11 | two propositions share one stable row ID | `falsifier_11_one_row_id_cannot_carry_two_propositions` |
| 12 | identity changes under ordering/formatting | `falsifier_12_ordering_cannot_change_inventory_identity` |
| 13 | issue/PR/workflow state used as evidence | `falsifier_13_workflow_state_is_never_evidence` |
| 14 | overbroad claim ceiling on a narrow subject | `falsifier_14_claim_ceilings_match_subject_breadth` |

Additional acceptance/closure tests:
`initial_rows_match_the_required_inventory_and_close_the_import_chain`,
`initial_profile_digests_are_pinned`,
`every_row_carries_the_full_field_set_and_a_canonical_owner`,
`initial_profiles_are_the_single_authority_for_the_manifest`,
`inventory_performs_no_evaluation_or_product_behavior`,
`imports_preserve_every_row_and_limitation_verbatim`,
`required_rows_are_conjunctive_across_the_inventory`.
