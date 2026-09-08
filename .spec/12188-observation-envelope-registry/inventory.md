# compiler_profile_observation model inventory (#12188)

Vocabulary owned by `xtask/src/compiler_profile_observation.rs`. This is a
hand-maintained projection of the type model and falsifier coverage; the
module and its tests are the executable authority.

## Identity types

- `ObservationDigest`, `ObservationIdentity` (64 lowercase hex sha256),
  `ReceiptFamily`, `ReceiptId` (non-empty, private-safe), `SchemaVersion`
  (u32), `AdapterId`, `AdapterVersion` (`v`-prefixed).
- `ProducerAndSchemaIdentity`: producer + family + schema version.
- `CanonicalReceiptReference`: receipt id + content digest + producer/schema
  — a reference, never payload; the source receipt stays canonical.
- `AdapterIdentity`: id + version of the producing adapter.

## Subject identity

- `SubjectDimensionKind` (8): repository_tree, binary_artifact, toolchain,
  compiler_policy, platform, fixture_series, producer_configuration,
  observation_time.
- `SubjectDimension`: proven(value, private-safe) / not_proven.
- `CandidateSubjectIdentity`: absent dimensions read back as explicit
  `not_proven`; canonical form names all eight dimensions.

## Closed dispositions (independent, preserved, never flattened)

- `ObservationClass`: landed `ClaimFamily` × `ProofClass` pair.
- `ObservationDisposition` (8): pass, failed, not_proven, stale,
  unsupported{reason}, not_applicable{justification},
  conditional_not_selected{trigger}, optional_absent.
- `CurrentnessDisposition` (3): current, stale, not_proven.
- `CompletenessDisposition` (3): complete, partial{remainder}, not_proven.
- `WorkDisposition` (4): completed{scope}, zero_work, not_applicable{reason},
  not_proven.
- `LimitationDisposition` (3): none, accepted_debt{scope, reason},
  not_proven.
- `TerminalState` (4): completed, instrument_failed{detail},
  cancelled{reason}, timed_out{detail}; `InstrumentAndTerminalState` pairs
  instrument identity with terminal state.
- `ObservedClaimCeiling`: wrapper over the landed `ClaimCeiling` (3);
  `narrows_or_matches` enforces narrowing-only adaptation.
- `InvalidationEvidence`: non-empty `Vec<InvalidationInput>` (#12186 type).

## Envelope laws (`CompilerProfileObservationV1::validate`)

- every free-text field is non-empty and private-safe (no host paths,
  issue/PR/workflow colour, log prose);
- invalidation non-empty;
- closed non-claiming dispositions and accepted debt cannot carry more than
  observed evidence;
- non-completed instrument or zero work cannot be typed pass/not-applicable;
- `canonical_semantic_text` / `identity`: order-insensitive,
  content-sensitive sha256 identity.

## Registry laws (`ObservationAdapterRegistry`)

- descriptor: schema range non-inverted; unsupported versions inside range;
  emitted classes non-empty; observation ceiling never exceeds source
  ceiling; no self-supersession.
- registration: duplicate ids rejected; same-family accepted-version overlap
  rejected unless a one-directional `supersedes` migration relation selects
  one; migration targets must exist in the same family.
- `select_adapter`: unknown/future/explicitly-unsupported/ambiguous schema
  fails closed; a migration relation deterministically selects the
  superseding adapter.
- `validate_observation`: registered adapter at exact version, owns the
  receipt family, accepts the schema, may emit the class, has every required
  currentness input carried by the observation's invalidation evidence, can
  prove every bound subject dimension, and the observation never strengthens
  the adapter's declared ceiling.
- `semantic_fingerprint`: registration-order independent.

## Synthetic fixtures (representability only, no real adapter)

- `adapter.synthetic-v1` (schema 1..=3, v2 unsupported) and
  `adapter.synthetic-v2` (schema 3..=5, supersedes v1);
  `passing_observation`, `red_but_complete_observation`.

## Falsifier coverage (issue #12188, 12 falsifiers)

| # | Falsifier | Pinning test |
| --- | --- | --- |
| 1 | complete red observation becomes pass | `falsifier_01_complete_red_observation_cannot_become_pass` |
| 2 | accepted source-locked debt becomes general semantic support | `falsifier_02_accepted_debt_cannot_become_general_support` |
| 3 | fixture replay becomes execution/EIR proof | `falsifier_03_fixture_replay_cannot_become_execution_or_eir_proof` |
| 4 | parser/compiler-internal evidence becomes provider/edit/installed-host evidence | `falsifier_04_parser_internal_cannot_become_provider_edit_installed_host` |
| 5 | unknown source schema is accepted | `falsifier_05_unknown_or_future_source_schema_fails_closed` |
| 6 | an adapter strengthens the source claim ceiling | `falsifier_06_adapter_cannot_strengthen_source_claim_ceiling` |
| 7 | two adapters ambiguously own the same source version | `falsifier_07_overlapping_adapters_require_explicit_migration` |
| 8 | issue, PR, workflow colour, log prose, or filename becomes evidence | `falsifier_08_workflow_state_never_becomes_evidence` |
| 9 | missing subject identity reconstructed from another receipt implicitly | `falsifier_09_missing_subject_identity_stays_explicit_not_proven` |
| 10 | instrument_failed/zero_work/cancelled/timed_out collapses into pass/not-applicable | `falsifier_10_instrument_and_zero_work_states_cannot_collapse` |
| 11 | registry ordering changes adapter selection or normalized bytes | `falsifier_11_registry_order_cannot_change_selection_or_bytes` |
| 12 | source/private payload content leaks into the normalized envelope | `falsifier_12_private_payload_cannot_leak_into_envelope` |

Additional closure/acceptance tests:
`closure_identity_is_order_insensitive_and_content_sensitive`,
`registry_required_currentness_inputs_are_enforced`,
`acceptance_source_vocabulary_is_preserved_not_flattened`,
`acceptance_successor_lanes_can_consume_the_public_surface`,
`acceptance_registry_ships_empty_and_no_evaluation_lands`.
