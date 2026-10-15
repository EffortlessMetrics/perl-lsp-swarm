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
  repository-job | reusable-workflow-job | external

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
- a missing mapped job or a dynamic name outside the explicit finite contract below;
- absent, commented, or dynamic `continue-on-error` where the policy claims `continue`;
- disagreement between `workflow_result` and direct job posture;
- a mapped context name that differs from the emitted job name;
- duplicate static emitters for one status context, including unlisted helper jobs;
- declared events the workflow does not have;
- path-filtered required events;
- unreachable jobs and applicability/condition contradictions;
- duplicate policy identities.

The parser is deliberately bounded. It reads the direct workflow fields needed for this contract rather than pretending to evaluate arbitrary GitHub Actions expressions. A condition that cannot be established at the claimed strength blocks the static contract instead of being guessed.

### Optional finite RIPR emission contract

Static names remain the default. A future direct `.github/workflows/ripr.yml: ripr`
row may explicitly declare `name_contract = "pr-material-required-name-v1"` with
`name = "ripr+ New Gap Gate"`, `applicability = "conditional"`,
`events = ["pull_request"]`, required policy role and propagating failure. No active
row or workflow opts in as part of the validator-support change. This versioned
optional field retains policy version 2: old validators reject the unknown field,
and its value is included in the canonical context/subject digest.

The only supported name expression is `RIPR_NAME_EXPRESSION` in the canonical
validator. It emits the protected name on opened, synchronize, reopened, Ready,
draft conversion, the exact merge-ready labeled/unlabeled edge, and a base edit.
Other metadata and non-PR events emit `Ignored metadata / ripr+ New Gap Gate`.
That ignored name must not be registered as a policy context. This is an explicit
emission domain, not an always-or-scoped-noop claim about the entire job.

Acceptance also requires the exact `RIPR_RESULT_CONDITION`: `always()` and the same
material-event predicate. A material event cannot skip the job with its protected
name. A non-material event may skip only the ignored identity. The explicit PR
activity list must be complete, without duplicates or required-event path filters.
The exact `RIPR_CONCURRENCY_GROUP` isolates ignored events by run ID and material
events by workflow/PR; literal cancellation is enabled. A job-level concurrency
override, reusable call, matrix, arbitrary/inverted expression, missing opt-in,
unknown contract or failure suppression is refused.

The finite opt-in accepts unique direct plain single-line name, result-condition
and concurrency scalars, plus a direct inline activity-type sequence with balanced
optional item quotes. Literal/folded blocks, scalar continuations, outer quoted
expressions, a quoted sequence-shaped scalar, malformed activity quotes and duplicate
direct job fields are refused. This narrow source grammar avoids flattening YAML
newlines into a different protected identity; it is not a general YAML validator.
These restrictions apply to the finite opt-in only, preserving existing static-name
and reusable-producer parsing.

Finite source comparisons ignore only YAML's ASCII space/tab separation and split
lines only at normalized CR/LF boundaries. Unicode whitespace and line separators
remain scalar data, including before a `#` that has no ASCII separator. This avoids
accepting an expression with extra name bytes or altered activity/cancellation data.
Ordinary Unicode text after an ASCII-separated comment remains permitted. See
[YAML 1.2.2 whitespace](https://yaml.org/spec/1.2.2/#55-white-space-characters)
and [line breaks](https://yaml.org/spec/1.2.2/#54-line-break-characters).
The legacy static/job projection parser retains its prior behavior; this finite
proof does not establish full YAML validity or decode escaped mapping keys.

Adversarial controls extend the original companion `37dae252c3436a91ae22034fdca0835e05b9cc1f`:
that source flattened literal `ripr+\nNew Gap Gate` into the protected name and
accepted a quoted sequence-shaped scalar or mismatched activity-item quotes.
The focused tests now reject these forms through the production validator, retain
balanced quoted activity items/plain failure propagation as positive controls,
and refuse the finite expression in reusable caller/callee projections. These are
local source-contract controls, not evidence of native Actions name emission.

The producer index includes both finite alternatives even for unlisted emitters,
so a second static or finite protected producer cannot hide behind the opt-in.
Opaque dynamic expressions mentioning the protected identity are refused when
this contract is selected. This bounded parser does not prove that arbitrary
unlisted expressions cannot synthesize a collision indirectly; candidate workflow
trust and hosted check-name/source association remain separate qualification work.

This contract establishes source emission/skip/concurrency consistency only. It
does not validate result-step proof, trusted admission, current PR readiness, actual
runtime cancellation, protection settings, or a 20-minute execution budget. Native
canaries and separately authorized cutover remain necessary. Reusable-producer
projection and its existing callee-source binding are unaffected; a reusable row
cannot borrow this direct-job contract.

The bounded support delta is composed on PR #16134 owner head
`f5e0f96537e8d71f6b592e6e798d073f6ead0e3e` without changing that branch or active
workflows. Coordination is recorded in [the companion comment](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16134#issuecomment-6182682200).
Migration ownership remains #10058/#17900/#17903. This support is not a claim that
the label cutover is active or that the broader trusted-admission work is complete.

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

## Immutable reusable workflow producers

`reusable-workflow-job` is an explicit one-level native producer. Alongside the
ordinary caller workflow/job, events, role and result posture, its row supplies
`callee_workflow` (the exact full-SHA uses reference), `callee_source` (tracked
verbatim YAML), `callee_sha256`, and `callee_job`. Native checks must be bound to
GitHub Actions integration15368. Dynamic names, matrix expansion and nested
reusable emitting jobs are outside this bounded contract.

The validator checks caller/ref agreement and source bytes, then derives the
caller-name / callee-name context and combined condition/failure posture. It
indexes every direct callee job for every caller of the declared immutable pin,
including undeclared duplicate callers. Existing direct-job validation is
unchanged. The source identities join the exact-source receipt, and all callee
fields join canonical context identity.

The parser proves local source consistency. Independent upstream retrieval and
review must establish that the vendored bytes belong to that immutable public
ref; a digest supplied alongside arbitrary bytes is insufficient provenance.
Native qualification must still confirm the real emitted name, app ID, PR head,
tested merge SHA, run and attempt before any protection change.

The proposed owned Rust row is OWNED_RUST_DRAFT_ROW in the existing validator's
test module, following TRUSTED_BASE_DRAFT_ROW's pre-registration pattern. The
assembled candidate declaration includes that identical proposed row and an
explicit pending-activation notice. Neither source asserts live enforcement.
Its changed producer/doc contracts require deliberate final qualification of
this complete candidate before an approved protection transaction.
