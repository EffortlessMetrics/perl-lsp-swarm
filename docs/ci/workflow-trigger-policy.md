# Workflow trigger policy (conventional required checks)

`scripts/cargo-admitted run -p xtask --locked -- workflow-trigger-lint` validates trigger and concurrency requirements for workflows listed in `.ci/policies/required-checks.toml`.

## Why this exists

The source policy names six desired required contexts across five workflows:
Perl LSP Rust Small Result, ripr+ New Gap Gate, Compile All Targets
(bit-rot guard), Conflict marker check, validate-title, and Gate Enforcement
Contract. The November 11, 2026 read of main ruleset 16664791 still contained
only the first five. The queue ruleset remained disabled and the complete
classic/ruleset protection union was not established. Source policy is not
proof of live enforcement; see [qualification activation](qualification-cutover.md).

Codecov / Patch 95 and codecov/patch remain advisory coverage contexts.

## Two tables, and which one this lint reads

`.ci/policies/required-checks.toml` holds two tables that answer different
questions, and they are easy to confuse because they differ by one letter:

- `[[checks]]` — the status-context inventory. It records the desired required
  contexts, producers, events, and separately claimed enforcement bindings.
  These source fields do not read or install GitHub protection.
- `[[check]]` — this lint's **governance list**. A workflow is evaluated
  against the clauses below only if it has a `[[check]]` row with
  `required = true`.

Between the two, a required context could name a workflow with no governance
row, and nothing said so: the lint deserialized `[[check]]` alone, and
`toml::from_str` drops unknown keys silently. Three of the five required
contexts were in that state (#16164).

The lint now reads both and refuses the mismatch. Every `[[checks]]` row with
`required = true` **that this repository produces** must name a workflow that
some `[[check]]` row governs, and the receipt carries the counts:

```
governance: 6/6 ruleset-required contexts governed, 0 produced outside this repository, 0 ungoverned, 13 accepted exemptions, 0 remainders
```

This is the observed source-policy result for the #17903 cutover composition.
The output's historical "ruleset-required" label counts source rows, not a live
GitHub read. All six desired contexts are repository jobs. Governed, external,
and ungoverned counts partition that source set; none proves an installed App,
native run, required-check union, or protected landing.

A row with `required = false` governs nothing — `evaluate_required_entry`
returns no violations for one — so it does not satisfy the requirement.

### The external-producer exclusion

Not every required context is produced by a workflow in this repository.
`codecov/patch`, for instance, is published by an installed integration and
names no workflow here. It is advisory today and so not in the required set the
counts above cover, but the classification has to exist before such a context
becomes required: demanding a governance row for it would be a rule nobody could
satisfy, and a lint with an unsatisfiable rule gets suppressed rather than fixed.
The inventory says which is which, and the lint reads that field rather than
inferring it:

| `producer` | Meaning | Effect on the count |
| --- | --- | --- |
| `repository-job` | A job in this repository publishes it | Must name a workflow a `[[check]]` row governs, or it is **reported** |
| `external` | An installed integration publishes it | Counted **out of scope**, not governed |
| absent | The row does not say | **Reported** — the lint does not guess |

The middle column is the whole point of the field: a missing `workflow` key is
legitimate only for an external producer. Reading *any* missing workflow as
external is what let a `repository-job` row that lost its `workflow` key during
an edit pass silently (#16172 review), so a stated `repository-job` must name
one and an unstated producer fails closed.

## Required-workflow lint rules

For each required governance row, the conventional clauses below apply unless
that exact clause has an explicit accepted exemption. #17903 deliberately
retires PR and routine push roots; those two exemptions do not waive G
reachability, unfiltered execution, or failure propagation.

- define `pull_request`
- define `merge_group`
- define `push` with `branches: [master]`
- avoid top-level or event-level `paths` / `paths-ignore`
- define event-aware concurrency:
  - `cancel-in-progress: ${{ github.event_name == 'pull_request' && github.event.action == 'synchronize' }}`
- not declare `pull_request` `types: [labeled]` or `[unlabeled]` —
  required workflows must run on code events only; label events should
  not re-trigger required CI (defense-in-depth on top of the
  synchronize-only `cancel-in-progress` rule above)
- exist at the configured path

## Exemptions

CI Gate, Rust Small, and RIPR carry only the two #17903 retirement
exemptions for pull-request-trigger and push-targets-master. Their actual
merge-group trigger and no-path-filter checks remain mandatory. RIPR also
retains its existing cancellation exception from #16141.

The title producer runs only on actual merge-group G. It reads native ordered
membership and App-authenticated H intake, checks B/H tree composition, then
validates current per-member title metadata and rechecks freshness. Commit-to-PR
associations do not prove membership. The source is read-only and its App
binding remains disabled. Its event-B checkout is not independent proof that
B is protected: speculative B trust remains an activation blocker.

Gate Enforcement Contract uses its own explicit G-only retirement exemptions.
No ordinary Draft, Ready, publication, title edit, or label change is a hosted
qualification request. A passing source lint does not prove the native lifecycle.

A `[[check]]` row may record such a divergence per clause:

```toml
[[check.exemption]]
clause = "event-aware-concurrency"
status = "accepted"
reason = "cancel-in-progress is false so a cancelled run cannot become a false red on a required context; the queueing cost is tracked in #16141."
tracking = "16141"
```

`clause` must be one of `workflow-exists`, `pull-request-trigger`,
`merge-group-trigger`, `push-targets-master`, `no-path-filters`,
`event-aware-concurrency`, `no-label-triggers`. An unrecognized clause is a
violation, so a typo cannot silently exempt nothing.

`status` is `accepted` when the clause is the wrong requirement for that
workflow, or `remainder` when it is a real gap that has not been closed yet. A
`remainder` must carry `tracking`.

Three rules keep the list from becoming a place to put things:

1. An exemption with an empty `reason` is a violation.
2. A `remainder` with no tracking issue is a violation.
3. **A stale exemption is a violation.** If the workflow comes to satisfy the
   clause, the exemption must be deleted. Without that rule an exemption list
   only grows, and a list that only grows stops meaning anything.

Exemptions are printed for passing rows as well as failing ones. An exemption
only visible in a failing run is an exemption nobody re-reads.

## Commands

- Policy run with receipt:
  - `scripts/cargo-admitted run -p xtask --locked -- workflow-trigger-lint --policy .ci/policies/required-checks.toml --receipt target/receipts/workflow-trigger-lint.json`
- Fixture validation:
  - `scripts/cargo-admitted run -p xtask --locked -- workflow-trigger-lint --fixture xtask/tests/fixtures/workflows/valid-required.yml`
- JSON output:
  - `scripts/cargo-admitted run -p xtask --locked -- workflow-trigger-lint --format json`

## CI workflow

`.github/workflows/workflow-trigger-lint.yml` subscribes only to
`merge_group: checks_requested`, without path filters. Its source does not
request ordinary PR or push runs. This page and the local receipt establish
source consistency only, not native execution or live check enforcement.
