# Merge-ready receipt protocol

`merge-ready` is bound to a receipt for an exact PR head, exact base lineage, and exact gate graph version.

## Receipt

Receipt JSON uses `.ci/receipts/schemas/merge-readiness.schema.json` and includes:

- `check`: `merge-readiness`
- `schema_version`
- `event`
- `pr`
- `head_sha`
- `base_sha`
- `gate_graph_version`
- `required_checks`
- `review_evidence`
- `blocker_labels_absent`
- `verdict`
- `expires_when`

## Required checks source

This repository's `main` branch is gated by GitHub enforcement mechanisms,
and a merge is blocked by their union. Conventional required checks are
read from `.ci/policies/required-checks.toml` first.

Only entries explicitly marked `required = true` are treated as required. The
current proof-floor contexts, by source mechanism, are:

Ruleset `main` (id `16664791`, `GET /repos/{owner}/{repo}/rules/branches/main`):

- `Perl LSP Rust Small Result`
- `ripr+ New Gap Gate`
- `Compile All Targets (bit-rot guard)`
- `Conflict marker check`
- `validate-title`

Classic branch protection (`GET /repos/{owner}/{repo}/branches/main/protection`)
no longer requires any status context. Rulesets bind administrators, so none of
these contexts can be bypassed by a manual admin probe while pending.

This list must match the live branch protection and ruleset state exactly. It
is not self-verifying: nothing compares it against GitHub, so a context added
to either surface without a corresponding `required = true` entry here
silently understates the gate set in every emitted receipt's `required_checks`
inventory and in `gate_graph_version`, which is hashed over this file. When a
required context is added or removed on either surface, update this file in the
same change. See issue #5418 for this gap's discovery. Reading the live
surfaces instead of trusting this checked-in list remains unbuilt, and is the
recurrence risk this leaves open.

`Codecov / Patch 95` is the repo-owned advisory coverage job. `codecov/patch`
is the external Codecov status context posted after Codecov processes an
explicit coverage upload. Both are advisory and must not block normal PR or
merge-queue flow.

### The main-red refusal makes advisory gate shards de-facto required (#16196)

`Perl LSP Rust Small Result` does more than report a candidate's own
aggregate. The `Probe main-red refusal` step of
`.github/workflows/em-ci-routed-rust.yml` classifies exact-SHA check runs for
the eight `CI Gate shard (...)` contexts — `meta`, `foundation`,
`parser_stack`, `analysis`, `lsp`, `support`, `corpus`, `policy` — on both
`main` and the PR/merge-group subject, using
`scripts/ci/main_red_refusal.py` (fetched from `main` itself at probe time so
the classifier cannot be altered by the candidate):

- a shard that is green, missing, or not completed on `main` never blocks
  here;
- while a shard is recorded red on `main` (`failure`, `timed_out`,
  `startup_failure`, `action_required`), the required check refuses to return
  a verdict until the candidate supplies its own completed, green exact-SHA
  result for that shard; candidate reds block immediately, and candidate
  evidence that stays missing or non-terminal through the bounded poll
  (50 × 30 s) is converted to a blocking refusal by the final fail-closed
  pass;
- the probe degrades to a non-blocking warning when its own inputs are
  unreliable: incomplete or failed API evidence, an unreadable canonical
  `ci.yml` on `main`, or `main` moving between the before/after SHA reads.

A candidate whose `ci.yml` differs from canonical `main`'s is treated as
having no comparable shard evidence: it is warned, then fail-closed after the
bounded wait. So while `main` is red, a PR that also modifies `ci.yml` cannot
clear the refusal with its own shard result; `main` has to be repaired first.

Consequence: no `CI Gate shard` context is registered as required, but a
shard red on `main` blocks the required lane for every candidate that cannot
present its own green exact-SHA shard result. "Advisory" means a context is
not registered as a required check; it does not mean nothing reads its
result. Before treating an advisory red as harmless, check whether a required
lane probes it — this one does. Triage the failing step, not the check name:
the required check can fail naming a workflow the diff never touched
(observed on #16172 during the #16011 ledger incident, where a single
un-ratcheted ledger entry took the required lane down via
`CI Gate shard (meta)`).

Operator remedy: fix `main` (or get the red shard rerun green there). The
only candidate-side escape is a completed, green exact-SHA result for the red
shard under a `ci.yml` identical to `main`'s. The refusal has no override
flag; it is fail-closed by design so repairs reach `main` unimpeded while
everything else waits.

## Draft pull requests

A draft PR is work in progress, not merge-ready. Every draft push still gets
exact-head `Rust formatting` and `Conflict marker check` results. The remaining
ready-tier jobs are intentionally deferred until `ready_for_review` to preserve
CI budget.

GitHub does not allow a pull request to merge while it remains draft, but its
skipped jobs can still look successful in the check list. Treat only completed
checks as evidence: a skipped job is not verification. Marking the PR ready
triggers the full workflow against the current candidate. Do not emit or accept
a `merge-ready` receipt while the PR is draft or while a required ready-tier
context has only a skipped result.

## Gate graph versioning

`gate_graph_version` is a deterministic hash over:

- `.ci/policies/required-checks.toml`
- `.ci/policies/**`
- `.ci/gates.d/**` (when present)
- required-style workflow files under `.github/workflows/**`

Inputs are normalized for line endings and sorted to exclude nondeterministic ordering.

## xtask commands

```bash
cargo xtask merge-ready emit --pr <N> --receipt target/receipts/merge-readiness.json
cargo xtask merge-ready verify --pr <N>
cargo xtask merge-ready verify --fixture xtask/tests/fixtures/merge-ready/valid.json
```

There is no `merge-ready reconcile` command. Readiness is a receipt and live
GitHub fact, not a lifecycle-label projection.

Verification statuses:

- `valid`
- `stale_head`
- `stale_base`
- `stale_gate_graph`
- `blocked`
- `missing`
- `not_proven` (the receipt itself records an instrument-incomplete verdict;
  this is non-ready, not an unknown success)

## Fan-in subject binding (#15343)

The fan-in snapshot (schema version 2) requires every `RequiredCheckEvidence`
row to declare its subject class:

- `candidate_head` — evaluated against the raw PR head only. This is
  candidate evidence. It is acceptable only while the snapshot declares no
  merge group; a raw-head result can never satisfy a declared
  merge-group integration subject.
- `pull_request_integration` — evaluated against the exact base + head
  integration tree. The row records its `base_sha` and is stale the moment
  the snapshot's current base differs, so advancing `main` (B → B2)
  invalidates the old B+H proof even when the head and results are unchanged.
- `merge_group` — evaluated against a queue-generated integration subject.
  The row records its `merge_group_sha` and is stale unless it binds the
  snapshot's current merge group.

A row whose declared subject does not match the snapshot's current subject is
a blocking `STALE` finding, never merge authorization. Snapshot rows without
a subject (schema version 1) fail deserialization: old head-only snapshots
cannot re-enter admission.

## Current operation

Use `merge-ready emit`/`verify` for receipt validation and the protected
GitHub preflight for the live candidate, review, required-check, and
mergeability snapshot. There is no apply-mode reconciler: operators do not
repair readiness through lifecycle labels.

See also: [Merge-train protocol](./merge-train-protocol.md).
