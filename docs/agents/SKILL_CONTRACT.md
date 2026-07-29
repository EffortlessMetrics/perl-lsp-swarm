# Skill contract

Skills are small, self-navigating artifact transformations. They make the next useful judgment clear without turning the repository into a runtime workflow engine.

## Required shape

Every public flow and substantive atomic skill should state:

```text
Purpose
Use when
Do not use when
Authoritative inputs
Focused questions
Recommended procedure
Optional orchestration / lenses
GitHub and repository inputs
Durable updates
What this establishes
What this does not establish
Valid exits and routes
Actual stop conditions
```

## Local route grammar

Use direct callable skill names in route sections.

```text
PLAN_READY
  → prepare-proof

PROOF_READY
  → build-candidate

CANDIDATE_READY
  → finish-pr

MATERIAL_PREMISE_CHANGED
  → prepare-issue

WEAK_PROOF
  → review-tests

REVIEW_FINDINGS_OPEN
  → address-review-comments

ALREADY_SATISFIED
  → return to deliver-pr for reconciliation

NOT_APPLICABLE
  → return to the public flow and select the next applicable skill

BLOCKED / NOT_PROVEN
  → name the exact dependency, authority, instrument, or evidence gap
```

Do not mix stage identifiers, agent identities, label names, guessed command names, and callable skills in one exit vocabulary.

## Applicability

The normal path runs every applicable pass. A pass is not applicable only when:

- its subject genuinely does not exist;
- current evidence already establishes the same judgment;
- the change is proportionally mechanical and has no corresponding decision;
- the flow entered after that judgment and replay would add no value.

A missed earlier pass causes forward repair, not retrospective punishment.

## Orchestration section

A substantive skill may include a concise `## Orchestration` section describing:

- questions that may run independently;
- the required join or synthesis decision;
- the integrating writer and contested mutation boundary;
- differentiated evidence, oracle, or review lenses;
- the durable result to preserve;
- runtime-only details that must not be persisted.

This is guidance for compiling an executor subgraph. It must not require a provider, model, agent count, team topology, or workflow engine.

## GitHub interaction section

State which native GitHub surfaces the skill reads and may update. Also state which surfaces must not be treated as authority.

A skill may use labels to classify area, kind, risk, release, or requested attention. It must not use lifecycle-mirror or agent-completion labels as proof that work succeeded.

## Structural validation

Maintenance-time validation may check:

- metadata and route targets;
- provider semantic coverage;
- no-proof, midstream, repair, and backward routes;
- one integrating writer per declared contested surface;
- root skill-discovery budget;
- absence of retired active references.

It must not inspect live issue or PR stage, require a named agent, authorize mutation, or run between ordinary skill transitions.
