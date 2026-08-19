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
