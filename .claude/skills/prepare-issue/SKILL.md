---
name: prepare-issue
description: Research, challenge, and plan a substantive concern before proof or implementation, or revisit a material premise discovered later.
argument-hint: "[issue number, PR number, or concern]"
---

# Prepare issue

Use this flow when the problem, owner, scope, proof seam, or implementation direction is not settled.

## Inputs

Use current source, live GitHub issues and PRs, accepted repository contracts, package guidance, and primary external authority where relevant.

## Orchestration affordances

### Root decisions

The main Claude thread retains the accepted problem, whether the work should exist,
semantic owner and consumers, one coherent claim, scope/non-goals, risk/rollback,
contradiction resolution, current plan, and whether a durable cross-PR specification is
warranted.

### Useful subagent work

Use focused subagents or an Agent Team only where useful for:

- source-owner and production-consumer mapping;
- related issue, PR, and landed-work search;
- reproduction and current proof inventory;
- external language/protocol/dependency truth;
- vision, duplication, slice, risk, and rollback challenge.

Give each child settled facts, exact authorities and scope, one question, falsifiers,
and a bounded return. Children do not choose the claim or update the issue independently.

### Mutation owner and join

One issue-body integrator owns durable synthesis and plan updates under the root-held
claim frame. Join evidence and contradictions in the main thread rather than counting
agents.

The join is ready when the problem, owner, consumers, scope/non-goals, proof obligations,
prerequisites, and plan are current; material contradictions are resolved or explicitly
`NOT_PROVEN`; and the result supports proof design or an honest no-proof route.

### Return packet

Return the current claim, owner/consumers, accepted plan, evidence references,
contradictions and dispositions, proof obligations, limitations, material return-to-
issue conditions, and typed route result.

## Flow

1. Invoke `find-or-create-issue`.
2. Invoke `research-issue`.
3. Invoke `review-issue`.
4. Invoke `issue-to-plan`.
5. Invoke `research-plan` and `review-plan`.
6. Invoke `compile-spec` only when the decision needs durable cross-PR authority.
7. Continue to `prepare-proof` without routine sign-off.

## GitHub

Use comments for durable research, corrected assumptions, external truth, dependencies,
alternatives, and contradictions. Update the issue body when one current synthesis or
plan helps builders and later contexts. Record a route change only when the material
next judgment changes.

### Canonical mutation readback

For every issue title/body, issue comment, or PR title/body mutation performed by this flow:

1. Bind the intended repository, issue or PR number, optional comment ID, field, and complete outgoing text before writing.
2. Reject outgoing text containing a control character other than ordinary tab or newline.
3. Before creating an issue comment, read the target issue's existing comments and search for the intended content identity. Treat an existing match as `APPLIED_AND_VERIFIED` and do not write a duplicate.
4. Submit the mutation once. A successful mutation response alone is not proof that the intended canonical state exists.
5. Re-read the exact GitHub target after the mutation and compare the observed identity and field against the intended bytes or a deterministic normalized digest.
6. Classify the result as exactly one of `APPLIED_AND_VERIFIED`, `WRITE_REJECTED`, `UNKNOWN_OUTCOME`, or `READBACK_MISMATCH`.
7. On timeout, transport failure, or any other unknown write outcome, do not retry until the exact target has been read back and searched for the intended content identity.
8. Treat an observed matching comment as the completed write; never post a duplicate comment to obtain a cleaner response.
9. Never report an issue, comment, or PR field as canonical from the local request alone. Repair a mismatch through a bounded new mutation and fresh readback, or return `NOT_PROVEN` with the observed mismatch.

### Runtime-local state

Keep subagent/Team topology, task state, retries, raw logs, provisional reasoning, and
routine skill transitions runtime-local. Labels classify stable area/risk/attention;
they do not prove lifecycle completion or lane ownership.

## Establishes

A current researched and vision-aligned plan adequate for proof design or a proportional no-proof route.

## Does not establish

Implementation, production reachability, formal review, or merge readiness.

## Routes

- `PLAN_READY` → `prepare-proof`
- `SPEC_REQUIRED` → `compile-spec`, then `prepare-proof`
- `MORE_RESEARCH_NEEDED` → `research-issue` or `research-plan`
- `MATERIAL_PREMISE_CHANGED` → repeat this flow
- `ALREADY_SATISFIED` / `SUPERSEDED` → return to `deliver-pr`
- `BLOCKED` / `NOT_PROVEN` → preserve the exact unresolved boundary
