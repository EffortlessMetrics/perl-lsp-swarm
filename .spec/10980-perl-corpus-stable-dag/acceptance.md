# Acceptance Criteria: #10980 — encode the stable perl-corpus authority DAG, conflict keys, and legacy exits

## §Behavior

| Input / Condition | Expected Result | Notes |
|---|---|---|
| `cargo xtask perl-corpus-train check` on the landed tree | Green: closed JSON Schema applied, every named law passes, shuffled control canonizes/projects identically, every invalid fixture fails with its named code, projections current | `run_check` |
| A controller, decision, external action, or historical node is marked selectable | `NON_LEAF_SELECTABLE` | falsifier 1 |
| Two selectable nodes own one exclusive conflict key with no hard dependency path between them (an evidence edge orders nothing) | `CONFLICT_KEY_PARALLEL_COLLISION` | falsifiers 2, 5, 11 |
| Two active nodes declare the same `authority_after` | `DUPLICATE_ACTIVE_AUTHORITY` | falsifiers 2, 8 |
| A banned state key, a status word in a lineage row, a commit hash, or a branch/pull coordinate appears in stable bytes | `MUTABLE_STATE_EMBEDDED` | falsifier 3 |
| A pull request is the subject of a non-historical node | `CANDIDATE_AS_ACTIVE_NODE` | falsifier 3 |
| A node ranked before `package_externalization` hard/evidence-depends on a package or publication node | `PUBLICATION_PROMOTED_INTO_FOUNDATION` | falsifier 4 |
| An authorization edge is carried by a coding node, or targets anything other than the declared `#EXPLICIT-AUTHORIZATION` authority | `AUTHORIZATION_ON_CODING_NODE` / `DEPENDENCY_CLASS_COLLAPSED` | falsifier 4 |
| An external action carries no authorization edge | `AUTHORIZATION_MISSING` | acceptance bullet |
| A node carries two retirement dispositions, points a disposition at itself, `superseded_by`/`supersedes` are not mirrored, or dispositions form a cycle | `SUPERSESSION_INCONSISTENT` | law |
| An implementation/cutover leaf has no legacy exit owner or condition | `MISSING_LEGACY_EXIT` | falsifier 6 |
| A node carries `superseded_by`/`duplicate_of`/`transferred_to` but is not historical | `SUPERSEDED_REACTIVATED` | falsifier 7 |
| A selectable leaf lacks proposition, falsifier, ceiling, stop conditions, proofs, negatives, verification, keys, surfaces, or forbidden surfaces | `INCOMPLETE_ONE_PR_CONTRACT` | falsifier 9 |
| A current leaf (#11029/#11030/#11031/#11032/#11034) is omitted | `UNKNOWN_EDGE_TARGET` / `CONSUMED_BY_MISMATCH` from its consumers | falsifier 10 |
| Nodes, dependencies, or arrays are reordered | Canonical digest, canonical form, and all four projections are byte-identical | falsifier 12 |
| A hard/evidence edge targets a controller | `DEPENDENCY_ON_CONTROLLER` | acceptance bullet |
| A hard/evidence cycle exists | `HARD_DEPENDENCY_CYCLE` | acceptance bullet |
| `consumed_by` differs from the derived reverse edge set | `CONSUMED_BY_MISMATCH` | law |
| A title changes without re-fingerprinting | `TITLE_FINGERPRINT_MISMATCH` | law |
| A conflict key is not in the registry | `CONFLICT_KEY_UNKNOWN` | law |
| Horizon ranks drift from the closed order, a role's selectability contradicts the closed role law, or `role_vocabulary`/`dependency_classes` is not the closed set declared once each | `VOCABULARY_DRIFT` | law |
| A `semantic_authority_refs` entry is neither a declared external authority nor a node subject | `UNKNOWN_EDGE_TARGET` | law |
| `cargo xtask perl-corpus-train graph --check` after a manifest edit without regeneration | Fails: projection drift | generated-output freshness |
| `cargo xtask perl-corpus-train explain-static pc_opened_asset_7693` | One bounded static packet ending with an explicit "readiness: not evaluated here" line; an unknown node fails | no readiness claim |

All tests pass: `cargo test -p xtask --locked perl_corpus_train`
No clippy warnings in the new files: `cargo clippy -p xtask --all-targets --locked -- -D warnings`
Formatted: `cargo fmt -p xtask -- --check`

## §Hazards

| Class | Invariant | Surface (specific file/fn this change touches) | Required adversarial test |
|---|---|---|---|
| ID/ref-space collision | Node ids, subject refs, exclusive authorities, and conflict keys are unique; a shared key is legal only under dependency ordering | `perl_corpus_train.rs::node_problems`, `conflict_problems` | `falsifier_2_*`, `falsifier_2b_*`, `falsifier_5_*`, `unknown_conflict_key_is_rejected` |
| Bounds/overflow | N/A — no client-supplied indices; all arithmetic is over small in-memory collections |  |  |
| Protocol-safety | Malformed or unknown manifest input never panics: unknown keys fail the JSON Schema, unknown targets fail closed, missing fields read as empty and fail the wording laws | `schema_failures`, `validate_document` | `every_expected_invalid_fixture_fails_with_named_code` (incl. `unknown_key_schema.json`) |
| Scanner literal/comment blindness | N/A — the only scanner is the lowercase-hex-run detector over string values; it is deliberately blind to nothing (any 32+ hex run anywhere is rejected) | `looks_like_commit_hash` | `falsifier_3_*` |
| Test-encodes-the-bug | No existing test's expected value is modified; every new expectation is a mutation of the landed manifest that must be rejected | `perl_corpus_train_tests.rs` | all `falsifier_*` tests |
| Coverage/measurement integrity | COV-1: `check` fails closed on a missing or empty `expected_errors.json`, a missing projection, or an instrument failure; nothing is reported as zero or complete when absent | `run_check`, `projection_drift` | `gate_command_run_check_is_green_on_the_landed_tree` plus the `graph --check` drift row above |

**Subsystem-specific defaults consulted**: [SUBSYSTEM_HAZARD_DEFAULTS.md — Coverage/CI section](../../docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md) (xtask surface). COV-2..COV-4 do not apply: the change adds no coverage transform, workflow, or gate-policy row.

## §Contracts

| Contract | Source document + section | How this change satisfies or extends it |
|---|---|---|
| `train_edge_contract.v1` typed edge and claim-profile vocabulary | `.spec/10858-train-edge-contract/`, #10858 | Three adaptation rows bind `hard`/`evidence`/`authorization` to `requires_implementation`/`requires_behavior_for_claim`/`external_checkpoint(manual_authorization)`; the bundle is registered in `manifests`; `cargo xtask check-train-edge-contract` adapts all 256 edges |
| Stable-versus-mutable truth | #10980 "Stable versus mutable truth"; #5205 | `stable_versus_mutable` block names owners; `MUTABLE_STATE_EMBEDDED` rejects live state in bytes |
| Dependency classes and release horizons | #10980 "Dependency classes", "Release horizons" | Closed vocabularies in schema and checker; `VOCABULARY_DRIFT` guards rank order |
| Shared-mechanics boundary | #10554 | Digest and canonical form consumed from landed modules; residual overlap recorded in `context.md` |
| Programme controller authority | #8826 body and comments | Every controller/leaf named there is a node; leaf-header dependencies preserved with provenance |
| Spec method | #3983, `docs/reference/SPEC_TEMPLATE.md` | This three-file packet plus the manifest, fixtures, and projections |

## §API-Shape

| Item | Kind | Signature / Range | Dup-risk (grep result) | Caller count |
|---|---|---|---|---|
| `perl_corpus_train.v1` (`train.manifest.json`) | stable graph | 106 nodes, 256 typed edges, 63 conflict keys, 9 horizons, 7 roles, 3 classes | none (`grep perl_corpus_train` empty on main) | consumed by #10992/#11001/#11010/#11017 |
| `schemas/perl_corpus_train.v1.schema.json` | JSON Schema | closed, `additionalProperties: false` at every level | none | applied by `run_check` |
| `perl_corpus_train::validate_document` | function | `fn(&Value) -> Vec<Violation>` | mirrors `native_neovim_train::validate_document` shape, programme-local laws | 1 (`run_check`) + tests |
| `perl_corpus_train::render_projections` | function | `fn(&Value) -> Result<Vec<(&'static str, String)>>` | none | `run_graph`, `projection_drift`, tests |
| `perl_corpus_train::render_explain_static` | function | `fn(&Value, &str) -> Result<String>` | none | `run_explain_static`, tests |
| `perl_corpus_train::title_fingerprint` | function | first 16 uppercase hex of SHA-256 | duplicates `module_train::title_fingerprint` (private there); recorded for #10554 | `node_problems`, tests |
| `cargo xtask perl-corpus-train {check, graph [--check], explain-static <node>}` | CLI | clap subcommand | none | operators, CI |
| Reason codes | closed vocabulary | 21 codes listed in §Behavior | none | `expected_errors.json` |

## §Test-Grid

| Scenario | Kind | Test name | Invariant discharged |
|---|---|---|---|
| Landed manifest validates | positive | `canonical_manifest_is_clean` | every law passes on the landed bytes |
| Shuffled control | positive/determinism | `shuffled_control_canonizes_identically_validates_and_projects_identically` | order invariance of digest, form, projections |
| Two renders identical, no host paths | determinism | `projections_are_deterministic_across_renders` | generated output deterministic and portable |
| Every invalid fixture fails with its code | negative | `every_expected_invalid_fixture_fails_with_named_code` | 20 fixtures, one named law each |
| Fingerprint law | positive | `title_fingerprint_follows_the_shared_law` | shared fingerprint law |
| Falsifier 1 | negative | `falsifier_1_controller_made_selectable_fails` | controllers never selectable |
| Falsifier 2 | negative | `falsifier_2_*`, `falsifier_2b_*` | exclusive authority uniqueness |
| Falsifier 3 | negative | `falsifier_3_*` | issue/PR/commit state never stable state |
| Falsifier 4 | negative | `falsifier_4_*` | publication never an ordinary foundation dependency |
| Falsifier 5 | negative | `falsifier_5_*` | same-authority nodes cannot be parallel |
| Falsifier 6 | negative | `falsifier_6_*` | compatibility exit owner required |
| Falsifier 7 | negative | `falsifier_7_*` | superseded nodes cannot reactivate |
| Falsifier 8 | negative | `falsifier_8_*` | product gold authority cannot move into generic topology |
| Falsifier 9 | negative | `falsifier_9_*` | falsifier/ceiling/stop required |
| Falsifier 10 | negative | `falsifier_10_*` | #11029/#11030/#11031/#11032/#11034 present; omission fails |
| Falsifier 11 | negative | `falsifier_11_*` | disjoint #11580/#11034 not serialized; conflicting #6996/#6999 not parallel |
| Falsifier 12 | determinism | `falsifier_12_*` | reversed arrays render identically |
| Hard cycle | negative | `hard_cycle_is_rejected` | acyclicity |
| Dependency on controller | negative | `dependency_on_a_controller_is_rejected` | umbrellas never gate leaves |
| External action without authorization | negative | `external_action_without_authorization_is_rejected` | authorization never inferred |
| consumed_by law | negative | `consumed_by_must_equal_the_derived_reverse_set` | derived reverse set |
| Fingerprint drift | negative | `title_fingerprint_drift_is_rejected` | retitle detected |
| Unknown conflict key | negative | `unknown_conflict_key_is_rejected` | registry closed |
| Evidence edge as writer ordering | negative | `evidence_edge_does_not_serialize_a_shared_exclusive_key` | only hard paths serialize shared keys |
| Node census | positive | `every_selectable_leaf_has_a_complete_contract_and_every_controller_is_unselectable` | ≥13 controllers, ≥60 selectable leaves, no selectable controller |
| explain-static | positive/negative | `explain_static_renders_a_bounded_packet_without_readiness` | packet bounded, no readiness, unknown node fails |
| Gate command end to end | positive | `gate_command_run_check_is_green_on_the_landed_tree` | schema + laws + fixtures + projections |

## §Blast-Radius

| Consumer | Crate | Dependency type | Impact | Required update |
|---|---|---|---|---|
| `train_edge_contract::run` / `landed_manifests_adapt_into_the_shared_vocabulary` | xtask | reads `adaptations.json` | now also adapts this bundle | adaptation rows and manifest entry added (this PR) |
| `module_train::canonical_digest`, `native_neovim_train::canonical_form` | xtask | direct call | none — read-only reuse of `pub fn`s | none |
| `xtask/src/main.rs` command tree | xtask | new subcommand | additive | wired (this PR) |
| `policy/non-rust-allowlist.toml` | policy | schema file allowlist | new `schemas/*.json` row | added (this PR); `.spec/**` already allowlisted |
| `crates/perl-corpus` | perl-corpus | none | no behavior change | none |
| #10992/#11001/#11010/#11017 | future | consume `train.manifest.json` | first stable input available | none here |

Must-not-touch boundary: `crates/perl-corpus/**` source, any corpus asset, other train bundles' manifests, `xtask/src/tasks/module_train*.rs`, `xtask/src/tasks/native_neovim_train*.rs`, GitHub state.

## Claim boundary

This bundle makes the perl-corpus train's stable topology durable and machine-checked. It
does not prove that any node is implemented, ready, or landed on the current tree
(#10992), that any candidate exists (#11001), that a spec packet or work packet is
correct (#11010/#11017), or that the topology is the semantically correct reading of
every leaf body beyond the header statements it cites (the #8826 revision route owns
corrections). Authority uniqueness is checked as exact `authority_after` text; a
differently worded overlap of product semantics into a generic-topology node is a
review obligation, not a checker law. Those remain `not_proven` here.
