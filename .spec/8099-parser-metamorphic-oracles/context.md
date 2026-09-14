# Context: #8099 — typed parser metamorphic oracles

Base pin: `main@a9664af790888333efbe50a042fa060f3cc2d171` (2026-08-27).
Controller: #8099. Accuracy fan-in and packet routing: #8031. Source/position
authority: #4973 and #6709. Structural comparison authority: #7038.
Incremental edit authority: #7008 and #7052.

## Decision

Replace the current aggregate hash-only “invariance” rows with retained,
typed transformation cases and plane-specific observations. The migration
starts by freezing the current trailing-whitespace investigation population;
it does not treat the reported `0.4 / 46` as parser evidence, a baseline, or a
ratchet floor.

This packet defines the migration boundary. It changes no grammar, gold
expectation, parser recovery policy, or accuracy floor. #8099 remains open
until the typed runner, artifacts, packets, status projection, and legacy
retirement are implemented and calibrated.

## Current evidence

The committed denominator projections disagree. `docs/project/status/parser.md`
renders 52 fixtures across 30 families, while
`docs/project/status/parser_accuracy_fixture_inventory.json` records 50 across
29. The generated status reports
`whitespace_invariance_rate=0.4 (trailing whitespace; n=46)`. The live manifest,
not either stale projection, is therefore the denominator authority for the
migration. The scorer does not retain the 46 applied case identities, the
remaining fixture dispositions, or a first divergent parser plane.

The committed status surfaces also disagree about failure packets:
`docs/project/status/parser.md` says 50 active packets while
`docs/project/status/parser_accuracy_failure_packets.json` records zero. This
is stale projection evidence, not proof that metamorphic mismatches already
have packets. The replacement must derive status and committed packet details
from the same current artifact.

## Current call graph

At the base pin, the operative path is:

```text
score_manifest_determinism
  ├─ whitespace_invariance_variant
  ├─ comment_invariance_variant
  ├─ newline_style_invariance_variant
  ├─ has_metamorphic_literal_boundary
  └─ parser_accuracy_projection_signature
       ├─ extract_line_tags
       ├─ extract_ast_predictions
       └─ parse_determinism_hashes(...).diagnostic_hash
            └─ normalize_offsets
→ DeterminismScore aggregate stable/sample counters
→ parser-accuracy MetricRow values
→ target/metrics/parser_accuracy.json
→ update_status/parser/accuracy.rs::parser_accuracy_metric_summary
→ docs/project/status/parser.md
```

`whitespace_invariance_variant` appends two spaces to every nonblank physical
line unless the source contains `<<`, `__DATA__`, or `__END__`.
`comment_invariance_variant` appends an EOF comment behind the same filter.
`newline_style_invariance_variant` globally replaces LF with CRLF behind the
same filter.

`parser_accuracy_projection_signature` hashes debug-formatted line tags,
selected AST predictions, and a diagnostic hash. `normalize_offsets` removes
every numeric run from diagnostic debug output. These operations can create
both false mismatch and false equality:

- payload or coordinate changes can alter the hash without changing the
  proposition a case intended to test;
- omitted token, AST, diagnostic, recovery, or semantic distinctions can
  preserve the hash despite a material parser change;
- broad numeric normalization can erase a meaningful diagnostic difference;
- whole-fixture substring filtering can admit unsafe payload edits and remove
  safe subregions without a retained disposition.

The `0.4 / 46` value is therefore an observation produced by an untrusted
legacy oracle. It is not evidence that 60% of fixtures expose parser defects.

## Object model

The replacement keeps logical case identity separate from one execution.

### `MetamorphicCase`

A reviewed proposition:

```text
case_id
fixture_id
transformation_profile_id
declared source asset/revision authority
declared safe point or region identity
required comparison planes
expected terminal relationship
allowed coordinate/presentation changes
criticality and packet policy
```

`case_id` is stable across execution order. It is built from the fixture
identity, transformation family/version, and fixture-authored point/region
identity. It is not an iteration index and does not silently change when a
path is moved.

### `MetamorphicObservation`

One execution of one case:

```text
observation_id
case_id
source digest
parser/tool/config/schema identities
validated transformation or typed applicability result
base and transformed subject identities
all evaluated plane results
first divergent plane
terminal result and reason code
bounded reproduce selector
legacy observation, when present
```

`observation_id` includes `case_id`, exact source content, parser/tool/config
identity, and schema version. A source revision therefore produces a new
observation without erasing the logical case.

### Terminal states

The retained terminal state is exactly one of:

```text
pass
mismatch
expected_difference
not_applicable
unsupported_transformation
transformation_failure
parser_instrument_failure
not_proven
```

Every non-pass includes a stable reason code. `not_applicable`,
`unsupported_transformation`, instrument failures, and `not_proven` stay in
the declared case denominator.

## Legacy population freeze

Before changing transformation behavior, emit one deterministic legacy row for
every fixture in the live manifest at the evidence pin under
`trailing_horizontal_whitespace.legacy.v1`.

- The 46 fixtures for which the current function returns a variant retain the
  old hash-equality observation, but their terminal result is `not_proven`
  with reason `legacy_hash_oracle_untrusted`.
- Every remaining fixture retains a row with terminal result `not_proven` and
  reason `legacy_applicability_unclassified`. The old substring/no-change
  decision is evidence about legacy control flow, not a trustworthy
  `not_applicable` ruling.
- No legacy row creates a parser-accuracy failure packet.
- No ratio derived from these rows may become a floor.
- Reclassification is consumptive: each legacy row remains visible until a
  typed registered profile covers it or a reviewed disposition explains why
  it cannot be covered.

If the live manifest or current sample count changes before the retained case
artifact lands, the implementation records the new base pin, every case
identity, and the exact population delta. It does not force a stale total from
one of the disagreeing generated projections onto newer source.

## Transformation authority

Typed transformations use exact byte edits over a content-addressed source.
The parser may consume the result and coordinate map; parser spans and AST
structure cannot define or validate the edits.

The transformer proves before parsing:

```text
edits are ordered, non-overlapping, and in bounds
expected old bytes match
final bytes are independently constructed and digested
line-ending and EOF behavior match the profile
the coordinate map covers unchanged, inserted, and removed regions
map composition and boundary round-trips hold
```

Initial safe profiles are deliberately narrow:

1. one trailing-horizontal-whitespace insertion at a fixture-authored ordinary
   code line boundary, including separate LF, CRLF, bare-CR, and EOF cases;
2. one comment insertion at a fixture-authored point between complete admitted
   statements;
3. one whole-file LF→CRLF case only when the fixture explicitly declares all
   payload regions newline-insensitive for that proposition;
4. repeated parse of identical bytes/configuration;
5. edit/undo cases delegated to #7008/#7052 rather than a second comparator.

Fixture declarations or an independently reviewed source-region authority
admit points and regions. The implementation must not replace the current
three-substring filter with a larger exclusion list or a generic Perl trivia
classifier inside the oracle.

## Comparison order

Compare in one deterministic first-divergence order while retaining every
evaluated plane result:

```text
1. transformation application and coordinate-map validity
2. parser subject identity and terminal disposition
3. token kind, payload, cardinality, and order
4. token ranges through the coordinate map
5. AST kind, non-coordinate payload, field/child identity, and cardinality
6. AST ranges through the coordinate map
7. structural/source invariant result from #7038
8. diagnostic identity, order, and non-range payload
9. diagnostic ranges through the coordinate map
10. recovery topology and declared limitations
11. semantic fact identity where the case requires it
12. strategy/work truth for incremental cases from #7008/#7052
13. instrumentation completeness
```

A complete typed fingerprint may short-circuit an exact-equality plane only
when its schema covers that whole plane. A mismatch still falls through to the
typed comparator and bounded first-divergence packet. Set comparison is
forbidden where order is part of the contract.

## Artifacts and accounting

The implementation adds:

```text
.ci/schemas/parser-accuracy-metamorphic-case.schema.json
target/metrics/parser_accuracy_metamorphic_cases.ndjson
target/metrics/parser_accuracy_metamorphic_summary.json
```

Case NDJSON is sorted by `case_id`, then `observation_id`, with exactly one row
per declared case observation. The summary is derived from those rows and
reconciles:

```text
declared_case_count
= pass
+ mismatch
+ expected_difference
+ not_applicable
+ unsupported_transformation
+ transformation_failure
+ parser_instrument_failure
+ not_proven
```

The summary reports each transformation family and terminal disposition
separately. It may publish rates as derived views, but no single scalar replaces
the retained rows.

Required typed mismatches flow through #8031 into the existing parser-accuracy
failure-packet authority. Packet production is separate from status rendering.
A missing required packet is an evaluation-integrity failure. Legacy hash rows
and unclassified applicability rows never produce parser defect packets.

## Implementation seams

Prefer an internal SRP module under
`xtask/src/tasks/metrics/parser_accuracy/metamorphic/`, with only minimal
wiring in the existing large `parser_accuracy.rs` scorer. Separate model,
transformation/map, comparison, and artifact concerns when the code earns the
split.

The staged migration is:

1. freeze every live legacy row and add schema/accounting checks;
2. implement independently validated edits and coordinate maps;
3. add typed parser snapshots and ordered plane comparisons;
4. route required typed non-passes into #8031;
5. project trusted case/disposition summaries into parser status;
6. retire `parser_accuracy_projection_signature`,
   `has_metamorphic_literal_boundary`, and aggregate-only invariance authority
   after all declared rows are migrated.

Status must label the legacy metric `investigation_only` or
`legacy_oracle_untrusted`; it must not continue rendering `0.4` as an ordinary
measured accuracy score.

## Claim boundary

This work can establish discriminating evidence for registered transformation
profiles. It does not prove arbitrary whitespace, comments, or newline changes
are semantics-free in Perl. It does not fix grammar behavior, bless current
parser output as gold, duplicate the canonical incremental comparator, or
close #8099 from a larger hash projection.

## Rollback

Each stage is additive until the typed artifact and status projection are
accepted. A stage can be reverted without changing parser behavior or gold
expectations. The old aggregate functions are removed only in the final
migration stage, after deterministic typed artifacts and packet routing are
green.
