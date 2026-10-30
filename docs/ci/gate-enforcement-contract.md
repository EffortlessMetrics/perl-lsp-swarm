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

The same command has an opt-in source-shape diagnostic for #17903:

```bash
python3 scripts/ci/validate_gate_enforcement_contract.py --root <clean-staged-checkout> --json \
  --qualification-source .github/workflows/ci.yml \
  --qualification-source .github/workflows/ripr.yml \
  --qualification-source .github/workflows/em-ci-routed-rust.yml
```

It retains the normal policy/producer checks and reuses their tracked workflow catalog.
Each selected source must have only `workflow_dispatch` as its entry event. Ordinary
PR (including Draft/Ready/update), push, merge-group and unknown entry events refuse.
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
Omitting the option retains the ordinary contract mode.

This is a local static source diagnostic. `qualification_status` remains `NOT_PROVEN`
even when source shape passes. It does not validate dispatch input values, trusted
request acceptance, head-change invalidation, required-check application association,
arbitrary step-script dispatch or the complete indirect graph. #17900 owns actual request/result
behavior; #17903 must close the full event perimeter; #17080/#10065 retain coordinated
source/settings activation. The current live workflows are expected to fail this
staged diagnostic. No trigger, required-context setting, permission or runtime admission
changes merely by invoking it; do not remove live producers before compatible replacements
are qualified. This diagnostic does not authorize a dispatch or merge.

Integration remains staged: #17900 must supply executable exact H/B/request/run
provenance and H2 invalidation; the result owner must demonstrate correctly linked
finalization without admission-only success; #16134/#17624 retain shared workflow
edits and no-product runner ordering; #17080 must observe the complete current
source/settings/protection generation and reviewed rollback before #10065 activation.
The ordinary PR/main triggers have not been removed. No native zero-run behavior,
warm-session savings, source-review eligibility, final gate or live enforcement
is established by this source-only leaf. Rollback removes this extension together
with its tests; it changes no hosted routing or required context.

## Subject binding

A successful receipt is bound to:

- the clean checked-out repository SHA;
- the tracked policy path, SHA-256 digest, version, and source;
- the complete tracked workflow catalog and each workflow digest;
- the canonical status-context rows interpreted by the validator;
- one aggregate `subject_sha256` over those identities.

A receipt with missing subject identity is not reusable evidence. Changing any governed workflow, producer mapping, event declaration, policy role, applicability, or enforcement claim changes the subject digest.

## Live GitHub boundary

The static validator proves checked-in policy/workflow consistency. It does **not** discover the live protected-context union. Live enforcement is additive across classic branch protection and active repository rulesets.

The receipt therefore records live enforcement as `NOT_PROVEN`. An authenticated observer must read both systems and must preserve `NOT_PROVEN` when either source is inaccessible. No checked-in `required = true`, green workflow name, or single accessible GitHub API can substitute for that observation.
