# Checklist: #8099 typed parser metamorphic oracles

Base pin: `main@a9664af790888333efbe50a042fa060f3cc2d171` (2026-08-27).
Controller: #8099 (`IMPLEMENTATION_READY` at the latest current-main review).
This first implementation slice is disjoint from the active guard-taxonomy and
heredoc region-scanning parser PRs. It changes the #8099 spec packet, the parser
status renderer and generated status row, and one focused xtask policy test.

## Evidence captured

- [x] Current denominator disagreement inspected: generated parser status says
      52 fixtures / 30 families; committed fixture inventory says 50 / 29
- [x] Current legacy observation inspected:
      `whitespace_invariance_rate=0.4`, sample count 46
- [x] Current scorer call graph inventoried from variant construction through
      status rendering
- [x] Legacy applicability weakness separated from parser behavior
- [x] Current packet-status disagreement recorded rather than treated as proof
- [x] #8099 controller, #8031 packet route, #4973/#6709 position authority,
      #7038 structural authority, and #7008/#7052 incremental authority kept
      distinct
- [x] No grammar defect inferred from the aggregate hash

## Planned implementation surface

### Stage 0 — specification and claim boundary

- [x] `.spec/8099-parser-metamorphic-oracles/context.md`
- [x] `.spec/8099-parser-metamorphic-oracles/acceptance.md`
- [x] `.spec/8099-parser-metamorphic-oracles/checklist.md`
- [ ] Plan review accepts the case/observation identity split, terminal states,
      artifact accounting, and legacy quarantine before scorer changes

### Stage 1 — freeze the legacy investigation population

- [x] Add `xtask/tests/parser_accuracy_legacy_oracle_policy.rs`
- [x] Retain every live manifest fixture in applied-or-omitted accounting
- [x] Pin the current 46 applied legacy cases without hard-coding a stale total
      from either generated denominator projection
- [ ] Add `.ci/schemas/parser-accuracy-metamorphic-case.schema.json`
- [ ] Add an internal module rooted at
      `xtask/src/tasks/metrics/parser_accuracy/metamorphic/`
- [ ] Emit one sorted NDJSON row for every live manifest fixture under
      `trailing_horizontal_whitespace.legacy.v1`
- [ ] Preserve the 46 applied legacy hash observations as `not_proven` /
      `legacy_hash_oracle_untrusted`
- [ ] Preserve every remaining fixture as `not_proven` /
      `legacy_applicability_unclassified`
- [ ] Emit `parser_accuracy_metamorphic_summary.json` from retained rows
- [ ] Prove legacy rows cannot packet or floor

### Stage 2 — validated transformations and coordinate maps

- [ ] Implement exact ordered edit application with old-byte checks, independent
      final-byte construction/digest, and typed failure states
- [ ] Consume the canonical #4973/#6709 source/position contract for byte,
      line, and UTF-16 mapping; do not create another line-index authority
- [ ] Add fixture-authored safe points/regions for the first narrow profiles
- [ ] Cover LF, CRLF, bare CR, EOF, BOM, zero-length endpoints, and multi-line
      ranges
- [ ] Fail closed on quote/regex/substitution/transliteration, heredoc, format,
      POD, DATA/END, opaque, and malformed regions unless explicitly admitted
- [ ] Prove an accidental payload edit is transformation failure, not parser
      mismatch

### Stage 3 — typed parser subjects and plane comparison

- [ ] Capture exact base/transformed parser subject identities and terminal
      dispositions
- [ ] Compare tokens, AST, structural invariants, diagnostics, recovery, and
      declared semantic facts in the acceptance order
- [ ] Keep coordinate payload and range comparisons separate
- [ ] Retain every evaluated plane result plus one deterministic first
      divergence
- [ ] Calibrate against deliberate token order, AST payload/cardinality/range,
      diagnostic order/range, recovery, transform, map, and instrument mutations
- [ ] Delegate edit/undo strategy/work truth to #7008/#7052

### Stage 4 — packets, status, and admission

- [x] Update `xtask/src/tasks/update_status/parser/accuracy.rs` so the three
      legacy hash rows render as `investigation_only` /
      `legacy_oracle_untrusted`
- [x] Exclude legacy investigations from trusted measured-row accounting and
      retain additional investigation counts separately
- [x] Add unit coverage proving the old `whitespace_invariance_rate=0.4` form
      cannot render
- [x] Regenerate the affected parser status row for the current committed input
- [ ] Route every required typed mismatch into #8031 with one bounded packet
- [ ] Treat a missing required packet as evaluation-integrity failure
- [ ] Derive status and committed packet detail/count from the same artifact
- [ ] Keep all non-pass terminal states visible in denominator summaries
- [ ] Activate no floor until mutation calibration and required packet coverage
      are green

### Stage 5 — consumptive retirement

- [ ] Reclassify every legacy row through a registered typed profile or reviewed
      explicit disposition
- [ ] Remove `parser_accuracy_projection_signature`
- [ ] Remove `has_metamorphic_literal_boundary`
- [ ] Remove aggregate-only whitespace/comment/newline authority
- [ ] Preserve derived family summaries only as views over retained case rows
- [ ] Close #8099 only at the acceptance close boundary

## Focused proof commands for implementation PRs

```bash
cargo fmt --all -- --check
cargo clippy -p xtask --all-targets --locked -- -D warnings
cargo test -p xtask --test parser_accuracy_legacy_oracle_policy --locked
cargo test -p xtask --all-targets --locked
cargo xtask metrics parser-accuracy --json
cp target/metrics/parser_accuracy_metamorphic_cases.ndjson /tmp/cases.first.ndjson
cp target/metrics/parser_accuracy_metamorphic_summary.json /tmp/summary.first.json
cargo xtask metrics parser-accuracy --json
cmp /tmp/cases.first.ndjson target/metrics/parser_accuracy_metamorphic_cases.ndjson
cmp /tmp/summary.first.json target/metrics/parser_accuracy_metamorphic_summary.json
cargo xtask update-status
git diff --check
```

The retained-artifact implementation must also run the focused #7038 structural
fixtures, #7008/#7052 edit rows, schema checks, failure-packet checks,
shuffled-order control, and deliberate mutation controls named in
`acceptance.md`.

## Explicit non-goals for this slice

- [x] No parser grammar or recovery change
- [x] No gold expectation or corpus baseline change
- [x] No claim that arbitrary whitespace/comments/newlines are semantics-free
- [x] No second incremental comparator
- [x] No new invariance floor
- [x] No #8099 close
