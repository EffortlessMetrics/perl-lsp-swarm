# Gate enforcement contract

The repository keeps four facts separate:

| Fact | Question |
| --- | --- |
| policy role | Is the evidence required, advisory, informational, or local? |
| execution result | Did it succeed, fail, remain pending, skip, cancel, time out, or fail as an instrument? |
| applicability | Did policy select the check, select a scoped no-op, or declare it not applicable? |
| GitHub enforcement | Does classic branch protection, a ruleset, both, or neither mechanically require the emitted context? |

An inner gate marked `required: true` in `.ci/gate-policy.yaml` means its failure makes that **runner invocation** fail. It does not prove that the containing workflow job is protected by GitHub. `PR Smoke (Fast Feedback, advisory)` is the concrete example: its selected inner gates can fail the `pr_fast` receipt while the job remains `continue-on-error` and outside live protection.

## Status-context inventory

`.ci/policies/required-checks.toml` is the checked-in status-context inventory. Its header is versioned and authority-bound:

```toml
version = 2
source = "github-enforcement-union"
```

An unsupported version or source is `NOT_PROVEN`; it is never interpreted under the current contract by accident.

Each `[[checks]]` row keeps these fields distinct:

```text
producer
  repository-job | external

workflow_result
  propagate | continue

policy_role
  required | advisory | informational | local

applicability
  always-or-scoped-noop | conditional | planned | not-applicable

enforcement
  github-branch-protection | github-ruleset |
  github-branch-protection+ruleset | neither | local | not-proven
```

A repository-owned producer names its exact tracked workflow file, job ID, static emitted context, expected events, and workflow-result posture. An external producer cannot borrow repository-job semantics. This prevents a Codecov status, repository job, and similarly named helper from being treated as the same authority.

`workflow_result` answers only how the mapped job reports failure:

```text
propagate
  direct job-level continue-on-error is statically false or absent

continue
  direct job-level continue-on-error is statically true
```

This is separate from policy role. An advisory context may propagate failure and show red while remaining non-blocking because GitHub does not protect it. The `Gate Enforcement Contract` itself uses that shape.

## Static contract

`scripts/ci/validate_gate_enforcement_contract.py` rejects:

- an unsupported policy version or authority source;
- disagreement between `policy_role` and `required`;
- a required row naming no protected enforcement;
- unsafe, untracked, symlinked, missing, or repository-escaping workflow paths;
- a repository-owned context without an emitting-job mapping;
- a missing or dynamically named mapped job;
- absent, commented, or dynamic `continue-on-error` where the policy claims `continue`;
- disagreement between `workflow_result` and direct job posture;
- a mapped context name that differs from the emitted job name;
- duplicate static emitters for one status context, including unlisted helper jobs;
- declared events the workflow does not have;
- path-filtered required events;
- unreachable jobs and applicability/condition contradictions;
- duplicate policy identities.

The parser is deliberately bounded. It reads the direct workflow fields needed for this contract rather than pretending to evaluate arbitrary GitHub Actions expressions. A condition that cannot be established at the claimed strength blocks the static contract instead of being guessed.

## Staged qualification-source diagnostic

The same command has an opt-in source-shape diagnostic for #17903. The selected
queue-time target uses this explicit mode:

```bash
python3 scripts/ci/validate_gate_enforcement_contract.py --root <clean-staged-checkout> --json \
  --qualification-entry-event merge_group \
  --qualification-source .github/workflows/ci.yml \
  --qualification-source .github/workflows/ripr.yml \
  --qualification-source .github/workflows/em-ci-routed-rust.yml
```

It retains the normal policy/producer checks and reuses their tracked workflow catalog.
Every selected source must have **only** `merge_group`, with explicit
`types: [checks_requested]`. Ordinary PR events (including Draft/Ready/update),
push, manual dispatch, schedule and unknown entries refuse, even behind a job
condition that currently returns false. No selected source is an error, not a
vacuous pass. The bounded event shape accepts an inline or block activity list;
implicit defaults, flow mappings, aliases, duplicate keys, filters and unsupported
indentation refuse rather than receive a guessed interpretation. This syntax
restriction is not a claim that every rejected shape is invalid GitHub syntax.

The default `--qualification-entry-event workflow_dispatch` retains the historical
dispatch-only **source diagnostic** and its receipt format. In that compatibility
mode, merge-group and ordinary lifecycle events still refuse. Keeping this diagnostic
does not authorize manual product qualification or override the current queue-only
target. Omitting `--qualification-source` retains the ordinary contract mode;
requesting the queue mode without a source fails.

Unresolved job-level reusable calls, job merges, and unsupported event/job mapping
shapes also refuse; this bounded parser does not silently certify omitted edges.
The same catalog now checks incoming local reusable calls, including transitive
callers and cycles. An unselected caller cannot hide behind a clean selected entry:
automatic, manual and `if: false` callers all require request-provenance assessment.
Missing, remote, dynamic and merged reusable targets, and unsupported unselected
job shapes, refuse because they cannot be excluded from the incoming perimeter.
Duplicate job IDs also refuse rather than overwriting a previously parsed edge.

Catalog `workflow_run` entries and literal workflow/API dispatch indications refuse
pending originating request/run/subject/trust assessment. This is a conservative
diagnostic, not a ban on correctly admitted result finalization. Dispatch hints are
not a shell or JavaScript interpreter: aliases, computed endpoints, external actions,
local helper scripts and arbitrary executable behavior remain unresolved. Conditions
and candidate-authored passing statuses are not evidence of admission.

The receipt's `qualification_catalog` includes every catalog path, its events,
reusable edges and dispatch indication lines. Nonselected entries are explicitly
`unresolved-not-selected`; that label does not exempt their product work, classify
them as cheap metadata, or establish that the complete graph is safe. Separately
governed deep/release/platform/incident/cache obligations remain in the denominator.
The selected list is included in both semantic and exact-source receipt identities.
Queue-mode receipts additionally bind `qualification_entry_event = "merge_group"`
into both identities, so a dispatch-only assessment cannot substitute for a queue
assessment of the same files. Default-mode receipt identities remain unchanged.

This is a local static source diagnostic. `qualification_status` remains `NOT_PROVEN`
even when source shape passes. It does not validate complete native group membership,
expected-head admission, dispatch input values, trusted request acceptance,
head-change invalidation, required-check application association,
arbitrary step-script dispatch or the complete indirect graph. #17900 owns actual request/result
behavior; #17903 must close the full event perimeter; #17080/#10065 retain coordinated
source/settings activation. The current live workflows are expected to fail this
staged diagnostic. No trigger, required-context setting, permission or runtime admission
changes merely by invoking it; do not remove live producers before compatible replacements
are qualified. This diagnostic does not authorize a dispatch or merge.

Integration remains staged: #17900 owns actual exact-head admission and native
queue/result behavior; #18255 owns title proof and the unresolved group-membership
or admission-equivalence contract; #16134/#17624 retain shared product workflow
edits. #17080 must observe the complete protection/source/settings generation and
review rollback before #10065 activation. A source-only pass establishes neither
native zero-run behavior nor protected landing, and does not enable the queue.

### This advisory workflow

`.github/workflows/gate-enforcement-contract.yml` now subscribes only to
`merge_group: checks_requested`; it no longer invokes this advisory check on
Draft, Ready, PR updates, main pushes or manual dispatch. Its concurrency key
includes the exact SHA and does not cancel an in-progress check for a different
subject. The existing test list, normal contract validation and receipt artifacts
remain; the list also executes `test_queue_qualification_source.py`. No new
workflow or dependency is introduced. The standard workflow invocation does not
turn on the optional whole-catalog queue diagnostic while its unresolved edges
still require the existing owners' work.

Run the focused source proof in the coding session before admission:

```bash
python3 -B -m unittest scripts/ci/test_queue_qualification_source.py \
  scripts/ci/test_gate_enforcement_app_bindings.py -v
```

Only this advisory root's event list changes in the status inventory. Its required
bit remains false, its policy role remains advisory, and all five protected
contexts and product producers are unchanged. This is not completion of the
repository-wide no-PR-CI cutover. While the native queue is disabled, the advisory
workflow will not run remotely; its source checks remain available locally.
Rollback of this slice restores this workflow and its event row together and
removes the new diagnostic/tests. No protection setting is changed in either
direction; the five required producers must not be disabled by analogy.

## Subject binding

Receipts retain two different identities:

- `subject_sha256` covers the semantic policy header, canonical status-context
  rows, selected source list and nondefault queue mode.
- `exact_source_sha256` covers the clean checked-out repository SHA, tracked
  policy path/content digest, complete workflow path/content-digest catalog,
  selected source list and nondefault queue mode.

A receipt with missing subject identity is not reusable evidence. Changing a
workflow's bytes changes the exact-source identity even when its policy meaning
is unchanged. Changing the interpreted policy or selected entry mode changes the
semantic identity. Neither hash authenticates a producer or proves native execution.

## Live GitHub boundary

The static validator proves checked-in policy/workflow consistency. It does **not** discover the live protected-context union. Live enforcement is additive across classic branch protection and active repository rulesets.

The receipt therefore records live enforcement as `NOT_PROVEN`. An authenticated observer must read both systems and must preserve `NOT_PROVEN` when either source is inaccessible. No checked-in `required = true`, green workflow name, or single accessible GitHub API can substitute for that observation.
