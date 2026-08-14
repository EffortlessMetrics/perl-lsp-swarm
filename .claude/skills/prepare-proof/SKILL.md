---
name: prepare-proof
description: Turn settled intent into the cheapest discriminating executable proof before implementation or candidate promotion.
argument-hint: "[issue, spec, or candidate]"
---

# Prepare proof

Use the issue plan, governing contract, current semantic owner, production path, existing tests, and independent external authority.

## Orchestration affordances

### Lane-root decisions

The lane root retains accepted behavior and semantic ownership, the production seam the
proof must reach, what sufficient discrimination means, acceptable proof cost, which
evidence may remain remote, and whether the claim genuinely has no executable proof
subject.

### Useful subagent work

Use focused subagents or context forks where useful to:

- retrieve a competent external oracle;
- construct realistic wrong implementations that should fail;
- identify opposite-direction, stale, failure, refusal, and recovery controls;
- trace the real production route;
- challenge denominator, fixture, schema, receipt, or instrument integrity;
- compare cheaper proof layers and hosted-CI cost.

### Mutation owner

One proof writer mutates tests, fixtures, schemas, or proof receipts in the current
claim branch/worktree. Read-only adversaries and oracles return evidence to that writer;
they do not create competing proof candidates.

### Join predicate

The proof is ready only when the instrument executed; a realistic wrong implementation
or current defect fails for the intended reason; the intended behavior can pass;
relevant controls make vacuity visible; the production seam is exercised or the
limitation is explicit; and exclusions plus `NOT_PROVEN` boundaries are named.

### Return packet and proof budget

Return proof identity, fixture/subject, command/instrument, observed result, realistic
falsifiers, controls, production-route evidence, proof deliberately not run, cost/remote
boundary, limitations, and typed result.

Prefer the smallest command that can falsify the claim. Run focused proof first,
affected package/semantic proof next, and broad/platform proof only when the claim or
integration policy requires it.

## Flow

1. Resolve current inputs and establish or reuse the current proof candidate/writer when proof artifacts require mutation.
2. Invoke `spec-to-test` to materialize and execute the proof.
3. Invoke `review-tests` against the observed execution and realistic wrong implementations.
4. Strengthen and re-execute the proof until adequate.
5. Continue to `build-candidate` without routine approval.

## GitHub boundary

Publish when proof changes accepted behavior, owner, obligation, route, or support;
when a reusable oracle/falsifier will help later work; or when a material
`NOT_PROVEN` remote/platform boundary must survive handoff. Link stable logs/artifacts
instead of copying them.

Keep subagent/Team topology, retries, temporary experiments, raw output, intermediate
mutations, and routine reruns runtime-local. Do not write proof workflow state to
tracked files or post one comment per run.

## Routes

- `PROOF_READY` → `build-candidate`
- `WEAK_PROOF` → `spec-to-test`, then `review-tests`
- `WRITER_COLLISION` / `UNSAFE_WORKTREE` → preserve the same-candidate mutation hazard
- `PLAN_CHANGED` / `MATERIAL_PREMISE_CHANGED` → `prepare-issue`
- `MORE_ORACLE_RESEARCH` → research, then repeat
- `NO_EXECUTABLE_PROOF_SUBJECT` → return to the invoking public flow for proportional candidate/claim review
- `ALREADY_PROVEN` → `build-candidate`
- `NOT_PROVEN` → preserve the missing evidence
