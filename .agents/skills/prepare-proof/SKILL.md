---
name: prepare-proof
description: Use when intent is settled but executable proof is absent, weak, circular, too expensive, or no longer aligned with the current claim or production seam.
---

# Prepare proof

## Purpose

Turn the accepted issue plan or durable contract into the cheapest executable proof that discriminates the intended behavior from realistic incorrect implementations.

## Inputs

- controlling issue and current plan;
- governing specification or ADR where applicable;
- current semantic owner and production consumer path;
- existing tests, fixtures, receipts, and known regressions;
- independent external authority where the claim depends on Perl, LSP, DAP, packaging, or dependency behavior.

## Orchestration affordances

### Lane-root decisions

The lane root retains:

- accepted behavior and semantic owner;
- the production seam the proof must reach;
- what a sufficient proof must discriminate;
- acceptable proof cost and which evidence may remain remote;
- whether the claim genuinely has no executable proof subject.

### Delegable read-only questions

Run independently where useful:

- retrieve a competent external oracle;
- construct realistic wrong implementations that should fail;
- identify opposite-direction, stale, failure, refusal, and recovery controls;
- trace the production route from real caller to changed seam;
- challenge denominator, fixture, schema, receipt, or instrument integrity;
- compare cheaper proof layers and hosted-CI cost.

### Mutation owner

One proof writer mutates tests, fixtures, schemas, or proof receipts in the selected
claim's current branch/worktree. Read-only adversaries and oracles return evidence to
that writer; they do not create competing proof candidates.

### Join predicate

The proof boundary is ready only when:

- the instrument executed and its identity/result are observable;
- a realistic wrong implementation or current defect fails for the intended reason;
- the intended behavior can pass;
- relevant controls prove the test is non-vacuous;
- the named production seam is exercised or the limitation is explicit;
- the proof's exclusions and `NOT_PROVEN` boundaries are named.

### Return packet and proof budget

Return proof identity, fixture/subject, command or instrument, observed result,
realistic falsifiers, controls, production-route evidence, proof deliberately not run,
cost/remote boundary, limitations, and typed result.

Prefer the smallest command that can falsify the claim. Run focused proof first,
affected package/semantic proof next, and broad or platform proof only when the claim or
integration policy requires it.

## Procedure

1. Resolve current inputs and establish or reuse the current proof candidate/writer when proof artifacts require mutation.
2. Run `$spec-to-test` to design, locate, materialize, and execute the proof.
3. Run `$review-tests` against the observed execution and realistic wrong implementations.
4. Strengthen and re-execute the proof until adequate or a material premise changes.
5. Continue to `$build-candidate`.

## GitHub boundary

Publish when the proof changes the accepted behavior, owner, obligation, route, or
support boundary; when a reusable oracle/falsifier will help later work; or when a
material `NOT_PROVEN` remote/platform boundary must survive handoff. Link stable logs or
artifacts rather than copying them.

Keep proof-agent identity, topology, retries, temporary experiments, raw output,
intermediate mutations, and routine reruns runtime-local. Do not write proof workflow
state to tracked files or post one comment per run.

## What this establishes

A proof boundary adequate to guide or protect the next candidate within its stated claim.

## What this does not establish

Implementation correctness, production reachability unless directly exercised, formal review, or merge readiness.

## Valid exits

- `PROOF_READY` → `$build-candidate`
- `WEAK_PROOF` → `$spec-to-test`, then repeat `$review-tests`
- `WRITER_COLLISION` / `UNSAFE_WORKTREE` → preserve the same-candidate mutation hazard
- `PLAN_CHANGED` / `MATERIAL_PREMISE_CHANGED` → `$prepare-issue`
- `MORE_ORACLE_RESEARCH` → research the named authority, then repeat this flow
- `NO_EXECUTABLE_PROOF_SUBJECT` → return to the invoking public flow for proportional candidate/claim review
- `ALREADY_PROVEN` → reference current proof and continue to `$build-candidate`
- `NOT_PROVEN` → name the unavailable or contradictory evidence
