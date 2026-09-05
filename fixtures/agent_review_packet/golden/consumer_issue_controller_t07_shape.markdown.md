# Review packet: R_controller_leaf

## Review subject

- packet: issue-controllers/R_controller_leaf/N_controller_bounded/2026-08-24T00:00:00Z
- repository: perl-lsp-swarm base 06a6af4f0 head c44e0d1b7 (diff sha256:3ac90f12)
- programme: issue-controllers / stage R_controller_leaf / proposition P_controller_bounded / profile read_only_reviewer
- owning issue: #11775
- builder packet: sha256:8d22 (agent_implementation_packet.v1)
- candidate state: not_observed (sha256:offline-frontier)
- changed authority: xtask/src/tasks/issue_controller_leaf.rs — bounded controller-leaf adapter
- changed authority: .spec/11775-controller-spec/acceptance.md — checked spec disposition
- evidence: #11774 checked spec disposition@06a6af4f0 (spec_disposition_receipt)
- evidence: cargo test -p xtask --bin xtask issue_controller_leaf --locked@c44e0d1b7 (focused_test_receipt)

## Stage falsifiers under audit

- falsifier F_no_independent_readiness [unit]: The adapter must fail closed when a required node-graph, spec, or frontier input is missing instead of deciding readiness itself.
- falsifier F_dry_run_not_live [unit]: An R03 dry-run route must never mutate live issue metadata; only the R05 route may, and only with an exact expected-old-state guard.

## Primary proposition and falsification questions

Primary proposition: The controller adapter composes #10872 builder packets for one bounded controller leaf without acquiring any independent readiness, GitHub-write, or merge authority.

- stage question Q_controller_as_leaf: Could a domain controller be misclassified as a bounded leaf, letting controller scope hide inside leaf delivery?
- stage question Q_readonly_write_escape: Does any read-only observer route (R03/R06) acquire a write path under refactor, retry, or error recovery?
- stage question Q_expected_old_state: Does the live mutation route preserve unrelated labels/body text by comparing expected old state before writing?

Shared seed questions (immutable):

- Q_seed_proposition: What exact proposition would this PR establish?
- Q_seed_wrong_impl: What realistic wrong implementation could pass a weak test?
- Q_seed_substrate: What distinguishes substrate/mechanism from external behavior?
- Q_seed_currentness: Which subject/currentness mismatch could create a false green?
- Q_seed_duplicate: Which existing authority might have been duplicated?
- Q_seed_cleanup: Which failure, cleanup, or retention path is easiest to omit?
- Q_seed_widening: Which claim could be accidentally widened?

## Review lenses

- [required] semantic_correctness
  - refinement R_packet_fidelity: Composed packets preserve every builder-packet semantic anchor; wrapper syntax changes nothing.
- [required] architecture_authority_duplication
  - refinement R_no_second_packet_schema: The adapter adds fields only; it never creates another packet schema beside #10872/#10881.
  - refinement R_registry_authority: Registry/directory/label authority substitution is challenged as duplicate authority.
- [required] subject_evidence_identity
  - refinement R_exact_tree_binding: Packets bind the exact current tree/head; a packet for another head cannot satisfy composition.
- [required] lifecycle_currentness_concurrency
  - refinement R_api_pagination: API pagination, permission, and instrument failure must surface as blocked, never as no-drift/no-candidate.
- [required] security_trust_boundary
- [not_applicable] resource_retention_cleanup — The adapter holds no persistent resources; packet instances are runtime-local outputs.
- [not_applicable] platform_runtime_portability — The adapter runs inside the repository-owned xtask binary; no platform surface is claimed.
- [required] spec_test_docs_consistency
  - refinement R_bdd_receipts: BDD/spec-ledger identities and receipts stay complete for the composed programme.
- [not_applicable] release_external_boundary — No release or external stage is claimed; the adapter is read-only tooling.

## Negative-control audit

- falsifier F_no_independent_readiness:
  - exists: established (Focused test removes the node-graph input and asserts the adapter fails closed.)
  - red_before_or_mutation_evidence: established (Removing the missing-input guard makes the focused test red against plausible-prose output.)
  - passes_only_intended_implementation: established (The test asserts the typed missing-input error, not a non-zero exit alone.)
  - correct_subject_and_generation: established (The missing-input fixture is hand-authored; it is not generated from the adapter.)
  - independent_expectation_source: established (Expected failure text is pinned in the fixture before implementation.)
  - alternate_subject_exclusion: established (The test binds the exact node identity, so an unrelated node's packet cannot satisfy it.)
- falsifier F_dry_run_not_live:
  - exists: established (Focused test drives the R03 dry-run route against a recorded API fixture and asserts no mutation call is issued.)
  - red_before_or_mutation_evidence: established (Routing the dry-run path through the live mutation client turns the test red.)
  - passes_only_intended_implementation: established (The test asserts the exact zero-mutation call log, not a summary count.)
  - correct_subject_and_generation: established (The recorded API fixture is authored from the documented response shape, not captured from the code under test.)
  - independent_expectation_source: established (Expected zero-mutation outcome is derived from the R03/R05 ruling in #11775, independent of the implementation.)
  - alternate_subject_exclusion: established (The fixture binds the exact controller and route; another route's recording cannot satisfy the test.)

## Old-path disposition

- seam old_title_body_heuristic: compatibility_projection
  - owner: #11775
  - exit: removed after D-series dogfood issues close

## Spec/test/docs/generated obligations

- spec_ledger_ids: .spec/11775-controller-spec/acceptance.md (#11774 checked disposition@06a6af4f0)
- fixture_expectation_manifests: fixtures/issue_controller_leaf/api_recordings.v1.json (sha256:5f2a)
- tests_mutations: xtask issue_controller_leaf focused tests (cargo test -p xtask --bin xtask issue_controller_leaf --locked@c44e0d1b7)
- generated_artifacts: docs/policy/NON_RUST_INVENTORY.md (second-run no-diff@c44e0d1b7)
- docs_projections: docs/agents/ISSUE_CONTROLLERS.md (docs check@c44e0d1b7)

## Review roles

- [required] builder_self_review: The builder states the composed proposition and falsifiers before independent review.
- [required] adversarial_challenger: An independent challenger attempts the false-positive-controller, registry-substitution, and dry-run-becomes-live escapes.
- [required] specialist: A GitHub-surface specialist challenges API pagination/permission failure handling and expected-old-state preservation.
- [optional] evidence_worker: Bounded evidence workers may fetch recorded API fixtures; they never review or mutate.

## Lifecycle discrimination

- graceful cleanup claimed: false
