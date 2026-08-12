---
name: prove-before-push
description: Execute or classify the candidate-bound local affected-proof boundary before ordinary PR publication, using the canonical pre-push planner, Changie, and current diff-scoped RIPR surfaces.
---

# Prove before push

Use this skill after a coherent candidate commit exists and before ordinary PR
publication. It is the local candidate-proof transition from #3949/#3985/#3986, not the
proof-design flow owned by `$prepare-proof` and not the remote integration flow owned by
`$verify-live-ci`.

The current claim lane normally executes this skill in place. Do not cold-start a
separate proof agent merely because the route moved from implementation to verification.
A bounded worker is useful only when another environment or high-output instrument
materially changes the evidence.

## Authority and current limitation

- #3786 owns exact staged-tree structural proof through `cargo xtask precommit`.
- #3985 owns affected committed-diff planning and proof routing.
- #7365 owns completion of one executable local path that includes diff-scoped RIPR and
  Changie in the candidate result.
- #3987/#3988 own current-head CI and protected integration.

Today `cargo xtask pre-push-plan --base auto --head HEAD --format json` is a pure planner.
It does not itself execute the selected steps or RIPR. The repository also exposes the
current local RIPR command sequence in `docs/ci/ripr.md` and a committed-diff adapter.
Do not claim #7365 complete or local RIPR proven unless the actual commands ran and their
candidate-bound receipts validated.

## Inputs

Establish:

- controlling issue, accepted claim/non-goals, and governing spec/policy where present;
- clean candidate commit and checked-out `HEAD`;
- canonical base resolved through the repository change-set authority;
- current proof obligations, risk tags, and local proof budget;
- applicable Changie fragment or evidenced exemption;
- current `ripr` tool availability and supported repository command surface;
- known remote-only platform, packaging, external-service, or merge-group proof.

A dirty worktree may contain future edits, but the result must bind to one immutable
candidate commit/range. Do not let unstaged fixes make the committed candidate appear
proven.

## Procedure

1. **Confirm the candidate boundary.** Record repository, base/head identity, claim,
   changed paths, and current worktree state. If the candidate is not coherent or the
   claim materially changed, return to `$build-candidate` or `$prepare-issue`.
2. **Generate the canonical plan.** Run:

   ```bash
   cargo xtask pre-push-plan --base auto --head HEAD --format json
   ```

   Preserve the plan schema, resolved SHAs, change-set digest, selected/deferred steps,
   affected packages, and posture. Do not replace it with hand-selected package logic.
3. **Execute selected affected proof serially.** Run the plan's selected commands through
   the repository's safe build/tool surfaces and host admission. Stop on a product/test
   failure; distinguish tool/config/timeout/capacity failure as `NOT_PROVEN`.
4. **Validate change disposition.** Validate the applicable staged/current Changie
   fragment or exemption and dry rendering. Missing or malformed disposition leaves the
   candidate incomplete before ordinary publication.
5. **Run diff-scoped RIPR when selected and available.** Use the canonical committed
   base/head and current supported commands from `docs/ci/ripr.md`. Validate the
   diff-scoped and review-guidance receipts against this candidate. Do not substitute a
   repo-wide total for the changed production scope.
6. **Preserve the migration boundary.** When the local RIPR executor/tool is unavailable,
   too expensive under the admitted budget, or not yet wired through #7365, return a
   precise local RIPR `NOT_PROVEN` or named remote-only boundary. Do not call the
   candidate locally complete merely because formatting and tests passed.
7. **Join one local candidate result.** Record selected proof run, deferred proof, RIPR
   disposition, Changie disposition, input identity, failures/limitations, and what
   remains remote. Keep raw logs and runtime routing local; link durable artifacts when
   they will be consumed at publication.

## Result classes

```text
LOCAL_CANDIDATE_PROVEN
  selected affected proof ran and passed; applicable local RIPR and Changie obligations
  are current; deferred/remote-only proof is named

CANDIDATE_PRODUCT_OR_TEST_FAILURE
  a valid selected instrument found a candidate-owned defect

RIPR_GAP_REQUIRES_REPAIR
  current diff-scoped analysis found a new actionable observation gap in changed
  production behavior

WEAK_OR_CIRCULAR_PROOF
  the candidate's proof cannot discriminate the intended behavior from a realistic
  wrong implementation

REMOTE_ONLY_PROOF_REQUIRED
  local candidate proof is otherwise sufficient, and an explicitly named platform,
  artifact, service, or protected integration fact can only be established remotely

INSTRUMENT_NOT_PROVEN
  planner, tool, receipt, input identity, timeout, capacity, or environment failure made
  the local result unreliable

RETURN_TO_ISSUE
  scope, authority, acceptance, risk, or semantic owner materially changed
```

`REMOTE_ONLY_PROOF_REQUIRED` is not a generic escape for missing ordinary local proof.
It must name the exact remote instrument and why the local environment cannot establish
that fact. Draft publication may be appropriate under `$publish-pr`.

## Routes

- `LOCAL_CANDIDATE_PROVEN` → `$publish-pr`
- `CANDIDATE_PRODUCT_OR_TEST_FAILURE` → `$build-candidate`, then repeat this skill
- `RIPR_GAP_REQUIRES_REPAIR` → `$improve-test-suite` or `$build-candidate`, then repeat
  affected proof and this skill
- `WEAK_OR_CIRCULAR_PROOF` → `$prepare-proof`, then resume `$build-candidate`
- `REMOTE_ONLY_PROOF_REQUIRED` → `$publish-pr` only through an explicit draft/remote
  proof boundary
- `INSTRUMENT_NOT_PROVEN` → repair/bootstrap the named instrument or preserve
  `NOT_PROVEN`; do not represent the result as green
- `RETURN_TO_ISSUE` → `$prepare-issue`

## Publication packet

Return a bounded packet containing:

- candidate base/head and change-set digest;
- selected and deferred plan steps;
- commands/results and affected scope;
- Changie fragment/exemption identity and render result;
- RIPR tool/version, diff receipt, review-guidance receipt, new-gap disposition, or exact
  `NOT_PROVEN` boundary;
- proof deliberately not run and why;
- result class, next skill, and remote-only wake event where applicable.

`$publish-pr` consumes this packet as an index. It does not copy raw logs into the PR or
reinterpret missing evidence as success.

## Non-goals

- No RIPR in the pre-commit staged-tree tier.
- No full-workspace or repo-wide burn-down proof on every local candidate.
- No automatic retirement of current required GitHub checks.
- No second change classifier, package graph, suppression policy, or receipt authority.
- No claim that static exposure proves mutation kill or general correctness.
