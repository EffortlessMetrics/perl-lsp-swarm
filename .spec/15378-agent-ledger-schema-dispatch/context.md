# Context — agent-ledger schema dispatch (#15378)

Controlling issue: #15378. Reconciles the F6 defect tracked separately as #15379.
Out of scope: #15380 (wiring the validator into a CI surface).

## Current behavior

`cargo xtask agent ledgers validate` (`xtask/src/tasks/agent_ledgers.rs`) globs
`docs/agents/ledgers/*.jsonl` and judges **every** row of **every** file against one
hardcoded field set:

```text
pr, title, classification, confidence, evidence, cleanup_done, known_gaps
```

Rows are decoded as untyped `serde_json::Value` and probed with `obj.get(..)` walks
(`agent_ledgers.rs:184-290`).

## What is actually on disk

`docs/agents/ledgers/` holds two committed ledgers, neither of which uses that shape:

| File | Rows | Governing contract | Shares with validator |
|---|---|---|---|
| `workflow-outcomes.jsonl` | 6 | `docs/agents/workflow-outcome.schema.json` | `cleanup_done`, `known_gaps` |
| `ub-review-calibration.jsonl` | 5 | none — prose only (`WORKFLOW_TEMPLATES.md` Workflow 6, `docs/ci/ub-review-adoption-notes.md`) | `classification`, `evidence` (different type) |

Measured on pristine `origin/main` (bbdd163) with the shipped binary, all 11 rows
fail — **63 errors** (33 in `ub-review-calibration.jsonl`, 30 in
`workflow-outcomes.jsonl`):

```console
$ cargo run -p xtask --quiet -- agent ledgers validate --format json
{ "ok": false, "files_checked": 2, "lines_checked": 11, "errors": [ ... 63 ... ] }
```

The claim in #15378 F6 / #15379 is true on current `main`.

## Root cause

The hardcoded contract is not the contract of anything in the directory it reads.

- It matches the skeleton row emitted by `xtask/src/tasks/pr_ledger.rs:70-95`
  (`LedgerRow`, whose doc comment says `pr: String` "matches validator schema").
- `pr_ledger.rs:261-266` writes those rows as a pretty-printed JSON **array** to
  `<slug>.json` under `target/reconciliation/` — never `.jsonl`, never
  `docs/agents/ledgers/`, and `pr-ledger.schema.json:5` states the output is
  "never committed".

So the validator's contract governs **no file that exists**, while the two files it
does read are governed by contracts it ignores. The module doc claims authority from
`docs/agents/ORCHESTRATION_ROLES.md`, but no role schema in that document declares the
`pr`/`title`/`classification`/`confidence` row shape (Scout `:86-94`, Builder
`:143-152`, Closer `:233-241` all differ).

The defect is not a wrong field set. It is that **schema selection is implicit and
universal** where the directory is in fact heterogeneous.

## Semantic owner

Per-ledger schema identity belongs to the ledger file. `docs/agents/workflow-outcome.schema.json`
remains the authority for workflow-outcome rows; the validator must conform to it
rather than restate it loosely.

## Why per-row `schema_version` is rejected

#15378 F2 proposes a per-entry `schema_version` matching the `agent_lease.rs` /
`agent_receipt.rs` family. That family pattern does not transfer here:

1. `workflow-outcome.schema.json:110` sets `"additionalProperties": false`. Adding
   `schema_version` to a row makes every existing row violate its own schema.
2. The same schema (`:5`) declares the ledger **append-only**: "rows are added after
   each workflow run; existing rows are never edited." Retrofitting a field to 11
   historical observation rows edits them.
3. `agent_lease`/`agent_receipt` records are single machine-generated JSON documents.
   A ledger file is an append-only sequence of dated observations; its schema identity
   is a property of the file, not of each historical row.

The envelope is therefore declared **once per file**, on a directive line, which adds
no bytes to any row and preserves both contracts above.

## Change

Replace implicit universal validation with an explicit typed registry:

- each ledger file declares `#!ledger-schema: <id>` on a directive line;
- each registered id decodes rows into a `#[serde(deny_unknown_fields)]` struct;
- an undeclared or unregistered `.jsonl` is a hard error, so a future ledger of a new
  shape fails closed instead of being silently judged by the wrong contract;
- `--expected-schema` pins the caller's expectation.
