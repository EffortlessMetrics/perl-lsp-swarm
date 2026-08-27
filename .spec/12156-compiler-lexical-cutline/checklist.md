# Checklist: #12156 — validator check inventory

What `cargo xtask compiler-lexical-cutline validate` checks, and the test
that proves each check fires.

## Shape and determinism

- schema/manifest/status/issue constants; unknown top-level fields rejected —
  `static_invalid_fixture_wrong_schema_version`
- JSON schema file parses — covered by `canonical_manifest_validates`
- deterministic canonical bytes (sorted keys, two-space indent, single
  trailing LF) — `rejects_noncanonical_bytes`
- fixture SHA-256 digests pin source bytes — `rejects_fixture_digest_drift`

## Protocol correctness (#12358 / LSP 3.18)

- exact prepare result shapes, exact rename params fields, continuation token
  forbidden — `rejects_protocol_continuation_token`,
  `static_invalid_fixture_continuation_token`
- nine-way correlation outcome vocabulary — `rejects_unknown_correlation_outcome`
- `old_plan_reuse` always `forbidden` — `rejects_old_plan_reuse`
- rename authorization always `current-subject-#10650`, never a prior
  observation or returned range — `rejects_prior_preparation_authorization`
- every preparation scenario has at least one row (no-prepare, matching,
  stale/fresh-success, stale/current-refusal, close-reopen, cache-miss,
  malformed-foreign stay distinct) — `rejects_missing_preparation_scenario`

## Denominators

- required admitted positive coverage tags all present —
  `rejects_missing_positive_denominator_coverage`
- required exclusion/refusal/lifecycle coverage tags all present —
  `rejects_missing_exclusion_denominator_coverage`
- admitted references rows use exactly `includeDeclaration=false` —
  `rejects_include_declaration_true_on_admitted_row`
- `exact_empty` only from complete facts and with empty locations;
  `exact_nonempty` requires non-empty locations and `exact` completeness —
  `rejects_exact_empty_with_locations`
- admitted rows never invoke fallback — `rejects_fallback_on_admitted_row`

## Independent expectation integrity

- anchor byte ranges select exactly the binding's sigil+name text —
  `rejects_forged_anchor_text`
- recorded UTF-16 positions match byte-derived positions (astral + CRLF
  fixtures included) — `rejects_utf16_position_drift`
- declaration anchor has role `declaration` and never appears in
  `reference_locations`
- no duplicate occurrence range inflates an exact denominator —
  `rejects_duplicate_occurrence_in_denominator`
- request subject lands on the binding's declaration or occurrences —
  `rejects_subject_off_binding`

## Set identity (authorization / plan / projection / application)

- authorized occurrence IDs == plan edit IDs == projected edit IDs == edit IDs
  on success — `rejects_plan_subset_of_authorized_ids`,
  `rejects_projection_superset_of_plan_ids`
- applied set equals projected set exactly when `applied-verified`; empty
  otherwise
- refusal/instrument-failure rows emit no edits and no partial ID sets —
  `rejects_refusal_with_partial_edits`
- applying the declared edits reproduces the recorded postcondition source —
  `rejects_broken_postcondition`

## Work assertions

- zero/false/identity assertions must name an instrument —
  `rejects_zero_without_instrument`
- pending reserved for final #4306 old-work zeroes and must name #4306 —
  `rejects_pending_without_4306_note`, `rejects_pending_nonzero_assertion`

## Mutation ownership

- exactly 37 mutations, contiguous IDs LX-MUT-01..LX-MUT-37 —
  `rejects_missing_mutation`
- every mutation maps to at least one existing stable row —
  `rejects_mutation_without_existing_row`
- bidirectional ownership: mutation fails_rows and case mutations lists agree —
  `rejects_one_directional_mutation_mapping`,
  `rejects_case_listing_unknown_mutation`

## Test topology

- exactly the five named targets; the active manifest target names an existing
  proof path — `rejects_missing_test_target`
