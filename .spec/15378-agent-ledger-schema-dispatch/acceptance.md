# Acceptance — agent-ledger schema dispatch (#15378)

## A1 — the validator is true of the directory it reads

`cargo xtask agent ledgers validate` exits `0` on the committed
`docs/agents/ledgers/` tree.

Negative control: on pristine `origin/main` the same command reports 63 errors over
all 11 rows and exits 1.
A test asserts the real committed ledgers validate clean, so the validator can never
again drift away from the artifacts it governs without a red test.

## A2 — schema selection is explicit and fails closed

| Input | Required outcome |
|---|---|
| file declaring a registered schema id | rows validated against that schema |
| file with no `#!ledger-schema:` directive | error naming the directive and the registered ids |
| file declaring an unregistered id | error naming the unregistered id and the registered ids |
| directive not on the first non-blank line | error — the directive is a header, not a floating comment |
| two directives in one file | error — one schema per file |

A `.jsonl` of an unanticipated shape must never be silently judged by another
ledger's contract. This is the F3 defect and its negative control.

## A3 — rows are typed, not probed

Each registered schema decodes into a `#[serde(deny_unknown_fields)]` struct.

| Input | Required outcome |
|---|---|
| unknown field added to a row | rejected, naming the field |
| `"pr": 42` where the schema declares a string | rejected as a type error |
| `"title": []` | rejected as a type error |
| missing required field | rejected, naming the field |
| optional field absent | accepted |

The F8 catch-all (`_ => {}`) must not survive: no wrong-typed value may pass as
"present and non-empty".

## A4 — existing support contracts are preserved, not weakened

- `close_proof` remains **required** when a pr-triage row is classified
  `close-superseded` or `duplicate-of-merged`. This rule exists to prevent the 29-PR
  false-close class recorded in `workflow-outcomes.jsonl` row 2 and
  `CLOSE_PROOF_POLICY.md`; it must keep its own passing negative control.
- The 13 pr-triage `classification` values and 3 `confidence` values remain enforced.
- Blank lines and `#` comment lines remain skipped.
- A missing ledger directory remains a non-error.
- Human and `--format json` output modes, the `ValidateOutput` shape, and the non-zero
  exit on any error are unchanged.

## A5 — conformance to the existing schema authority

`workflow-outcome.schema.json` is the authority for workflow-outcome rows. The Rust
struct must accept exactly its 15 required properties plus optional `notes`, and
reject additions (`additionalProperties: false`).

A test decodes the schema's own `examples[0]` and requires it to validate clean, so
the Rust type and the JSON Schema cannot drift apart silently.

## A6 — caller-side version pin

`--expected-schema <id>` succeeds when every file resolves to that id and fails,
naming the mismatch, when any file does not. Absent the flag, each file is validated
against its own declared schema.

## Verification

```bash
cargo fmt -p xtask -- --check
cargo clippy -p xtask --all-targets --locked -- -D warnings
cargo test -p xtask --locked agent_ledgers
cargo run -p xtask --quiet -- agent ledgers validate --format json
git diff --check
```

## Limitations

- This claim does not wire the validator into any CI surface; unwired-validator
  remains #15380.
- `ub-review-calibration.v1` is derived from the five committed rows and the prose in
  `WORKFLOW_TEMPLATES.md` Workflow 6 and `docs/ci/ub-review-adoption-notes.md`. No
  JSON Schema file is authored for it here; the Rust struct is its first machine
  contract.
- `pr-triage.v1` keeps the shape `pr_ledger.rs` emits. Whether that generator should
  write `.jsonl` into a validated directory at all is a separate question and is not
  decided here.
