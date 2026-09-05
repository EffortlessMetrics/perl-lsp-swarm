# Acceptance: #8099 typed parser metamorphic oracles

Each row binds one stable proposition to its discriminating executable proof.
The intended proof surface is focused unit coverage under
`xtask/src/tasks/metrics/parser_accuracy/metamorphic/`, an integration policy
binary at `xtask/tests/parser_accuracy_metamorphic_oracle.rs`, schema checks,
and deterministic artifact generation. No row is satisfied by the current
aggregate hash alone.

| Row | Proposition | Discriminating proof | Status |
| --- | --- | --- | --- |
| PMO-001 | The legacy trailing-whitespace population is frozen before behavior changes: every live manifest fixture has a retained disposition, including 46 applied legacy observations and every remaining fixture as unclassified legacy applicability | `legacy_whitespace_population_retains_every_live_manifest_fixture` currently proves unique fixture/source identities, 46 applied cases, and denominator closure; the retained NDJSON rows remain to be implemented | partial |
| PMO-002 | Logical case identity is independent of execution order, path movement, and observation revision; source/parser/config changes create a new observation identity without erasing the case | `case_ids_survive_shuffle_and_observation_ids_track_subject_identity` runs ordered and shuffled inputs and mutates source/config identities | planned |
| PMO-003 | Every declared case remains in the denominator with exactly one terminal state; unsupported, not-applicable, transform/instrument failures, and `not_proven` are never filtered away | `terminal_dispositions_reconcile_declared_denominator` plus schema `oneOf`/required-field checks; mutation control deletes an unsupported row and must fail accounting | planned |
| PMO-004 | The legacy projection hash is quarantined as investigation evidence: it cannot establish pass/mismatch, emit a parser failure packet, or become a ratchet floor | status-renderer unit coverage proves the three legacy hash metrics render as `investigation_only`; packet and floor exclusion remain to be proved by `legacy_hash_observations_are_not_proven_and_never_packet_or_floor` | partial |
| PMO-005 | Transformations are independently constructed from exact byte edits; parser spans/AST are not used to select, apply, or validate edits | `validated_edits_reject_overlap_out_of_bounds_and_old_byte_mismatch` and a dependency/policy assertion that the transformer accepts source bytes plus declared edits, not parser nodes | planned |
| PMO-006 | The coordinate map is total for the declared transform and correct at LF, CRLF, bare-CR, EOF, BOM, zero-length endpoints, and multi-line ranges | `coordinate_map_round_trips_all_declared_line_endings_and_boundaries`; mutations lose and double-count CRLF bytes | planned |
| PMO-007 | Payload-bearing and recovery-sensitive regions fail closed unless a fixture or reviewed region authority explicitly admits a safe point; no expanded substring exclusion list becomes authority | `payload_edit_is_transform_failure_not_parser_mismatch`, covering multiline quote/regex, heredoc, format, POD, DATA/END, opaque, and malformed boundaries; policy test forbids a replacement global `contains` gate | planned |
| PMO-008 | Plane comparison is typed, complete for each declared plane, ordered deterministically, and retains all evaluated results while naming one first divergence | `first_divergence_order_is_stable_and_all_plane_results_survive`; mutations alter token order, AST child cardinality, diagnostic order, recovery family, and instrumentation completeness | planned |
| PMO-009 | Exact equality and coordinate-mapped equality are distinct; legitimate mapped movement passes while widened or otherwise wrong ranges fail | `mapped_ranges_accept_expected_shift_and_reject_widening`; controls compare a correct shift without the map and normalize a widened range away | planned |
| PMO-010 | Every required typed mismatch produces one bounded #8031-compatible packet with the exact case, subjects, transformation/map, first divergent plane, expected/observed projections, owner, and reproduce selector | `required_mismatch_has_exactly_one_bounded_failure_packet`; controls suppress and duplicate the packet | planned |
| PMO-011 | Parser mismatch, expected difference, unsupported transform, not-applicable, transformation failure, parser instrument failure, and `not_proven` remain distinguishable in artifacts and status | schema validation plus `terminal_state_round_trip_preserves_reason_and_owner`; no state is represented by an empty/missing row | planned |
| PMO-012 | Case NDJSON and summary JSON are byte-deterministic across repeated and shuffled runs, and summary counts are derived from retained rows | `metamorphic_artifacts_are_byte_stable_and_summary_reconciles_rows`; run generation twice and compare bytes | planned |
| PMO-013 | Parser status labels the legacy ratio `investigation_only`/`legacy_oracle_untrusted`, derives trusted family summaries from typed rows, and reads failure-packet count from the same canonical artifact used for committed packet details | `legacy_metamorphic_hash_rows_render_as_investigation_only` now proves status quarantine and trusted/investigation accounting; canonical packet-count reconciliation remains to be implemented | partial |
| PMO-014 | Repeated-parse cases use the same typed subject model; edit/undo and strategy/work cases consume #7008/#7052 rather than creating a second incremental comparator | `incremental_cases_delegate_to_canonical_differential_subject`; structural policy rejects a parallel edit comparator in the metamorphic module | planned |
| PMO-015 | No invariance floor activates until required curated cases are typed, the oracle catches product and instrument mutations, and all required non-passes packet correctly | `floor_activation_requires_calibrated_typed_population`; a single required mismatch or `not_proven` critical case keeps admission false | planned |
| PMO-016 | Legacy aggregate helpers are removed only after all declared legacy rows have reviewed typed coverage or an explicit terminal disposition | `legacy_retirement_is_consumptive_and_complete` fails while any legacy row lacks a migration owner; final policy forbids `parser_accuracy_projection_signature` and `has_metamorphic_literal_boundary` | planned |

## Required artifact contract

Each case row must contain, at minimum:

```text
schema_version
case_id
observation_id
fixture_id
source path/content digest
transformation family/version/profile and declared point/region
applicability state and reason
ordered exact edits and independently computed final digest
coordinate-map identity and validation result
base/transformed parser subject identities
required planes and every evaluated plane result
first divergent plane, if any
terminal state, reason, criticality, and likely owner
bounded reproduce selector
legacy observation/provenance, when present
```

Applicability, transformation validation, plane results, and terminal results
are separate closed vocabularies. They must not be inferred from a missing row
or overloaded into one enum.

```text
applicability.state:
  admitted
  not_applicable
  unsupported_transformation
  not_proven

transformation_validation.state:
  valid
  failed
  not_run

plane_result.state:
  equal_exact
  equal_via_coordinate_map
  expected_difference
  mismatch
  not_required
  not_evaluated
  instrument_failure
```

A terminal `pass` requires admitted applicability, a valid transformation, and
every required plane to be `equal_exact`, `equal_via_coordinate_map`, or a
declared `expected_difference`. `not_evaluated` and `instrument_failure` can
never be laundered into pass. A transform rejected before parsing retains its
applicability and validation evidence without fabricating parser-plane results.

The schema must reject unknown applicability, validation, plane, or terminal
states; missing reason codes on non-pass rows; a first divergence not present
in evaluated planes; packet policy on an untrusted legacy observation; and a
claimed `pass` when any required plane is unequal or unevaluated.

## Shift-left falsifiers

These controls must fail before the happy runner is accepted:

1. trailing spaces enter a multiline `q{...}`, regex, POD, format, or heredoc
   payload and the case is called eligible;
2. one heredoc marker removes unrelated safe ordinary-code regions without a
   retained disposition;
3. comment insertion after malformed source changes recovery but is reported
   as parser invariance;
4. CRLF mapping loses or double-counts newline bytes, including endpoints on
   the newline itself;
5. a correctly shifted range is compared without the coordinate map;
6. a widened AST or diagnostic range is normalized away;
7. changed token order or AST child/cardinality retains the selected legacy
   projection;
8. changed diagnostic order is hidden by hashing or set comparison;
9. changed recovery family retains the same top-level AST prediction;
10. invalid final bytes are accepted because both parses fail similarly;
11. transform timeout or parser instrument failure disappears from accounting;
12. a raw hash mismatch emits a parser failure packet without a typed divergent
    plane;
13. a required typed mismatch emits no #8031 packet;
14. shuffled case order changes IDs or artifact bytes;
15. edit/undo uses a second comparator rather than #7008/#7052;
16. the current `0.4 / 46` observation is admitted as a floor.

## Close boundary

The specification packet itself does not close #8099. The controller closes
only when all declared metamorphic cases have explicit applicability and
terminal results, final bytes/maps and parser planes are independently proved,
required typed non-passes produce bounded packets, deterministic artifacts and
status projection are current, and the entire legacy investigation population
is fully reclassified or explicitly disposed.
