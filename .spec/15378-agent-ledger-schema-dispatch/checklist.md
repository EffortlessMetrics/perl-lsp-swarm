# Checklist — agent-ledger schema dispatch (#15378)

## Production

- [ ] `LedgerSchemaId` registry with `pr-triage.v1`, `workflow-outcome.v1`,
      `ub-review-calibration.v1`; parse from directive text; render back for errors.
- [ ] `#!ledger-schema: <id>` header directive parsing — first non-blank line only,
      exactly once per file.
- [ ] Typed rows, all `#[serde(deny_unknown_fields)]`:
      - [ ] `PrTriageRow` — preserves the current field set and semantic rules.
      - [ ] `WorkflowOutcomeRow` — conforms to `docs/agents/workflow-outcome.schema.json`.
      - [ ] `UbReviewCalibrationRow` — conforms to the five committed rows.
- [ ] Per-schema semantic checks after decode (enum values, non-empty strings,
      `close_proof` conditional, date pattern).
- [ ] `ValidateConfig.expected_schema: Option<String>` plumbed from a new
      `--expected-schema` CLI flag; mismatch is an error.
- [ ] Errors keep `file` / `line` / `message` and the existing `ValidateOutput` shape.
- [ ] Module doc corrected — it must not claim authority from `ORCHESTRATION_ROLES.md`
      for a shape that document does not declare.

## Ledger files

- [ ] `#!ledger-schema: workflow-outcome.v1` header added to `workflow-outcomes.jsonl`.
- [ ] `#!ledger-schema: ub-review-calibration.v1` header added to
      `ub-review-calibration.jsonl`.
- [ ] No existing row byte is modified (append-only contract preserved).

## Proof

- [ ] A1 committed-ledgers-validate-clean test (binds the validator to the real files).
- [ ] A2 five dispatch negative controls.
- [ ] A3 unknown-field and wrong-type rejections per schema.
- [ ] A4 `close_proof` conditional, classification/confidence enums, comment/blank
      skipping, missing-directory success — all still green.
- [ ] A5 `workflow-outcome.schema.json` `examples[0]` decodes clean.
- [ ] A6 `--expected-schema` match and mismatch.
- [ ] Existing tests kept unless a test asserted the defect itself; any removal
      justified in the PR body.

## Verification

- [ ] `cargo fmt -p xtask -- --check`
- [ ] `cargo clippy -p xtask --all-targets --locked -- -D warnings`
- [ ] `cargo test -p xtask --locked agent_ledgers`
- [ ] `cargo run -p xtask --quiet -- agent ledgers validate --format json` exits 0
- [ ] `git diff --check`

## Boundaries

- [ ] No CI surface wiring (#15380 keeps that claim).
- [ ] No production `unwrap` / `expect` / `panic!` / `todo!` / `dbg!`.
- [ ] `docs/policy/NON_RUST_INVENTORY.md` not regenerated on this branch.
- [ ] No change to `pr_ledger.rs` output location or format.
