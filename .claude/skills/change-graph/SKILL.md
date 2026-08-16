---
name: change-graph
description: Orchestrator-only compiled route and durable-state projection for taking one repository change from issue through proof, review, GitHub integration, merge, and reconciliation.
user-invocable: false
---

# Change graph

This is an orientation skill for a campaign root or persistent claim lane. Ingest it once
per context, then execute the named public flow and atomic skills. It is not a stage,
tracked frontier, receipt, scheduler, or replacement authority.

Focused subagents and review lenses do not ingest this graph by default. They read root
and nearest package guidance, then consume the exact skill named in their brief. A child
loads this graph only when it becomes the accountable orchestrator for a durable goal or
claim.

## Authority

This skill compiles the settled repository protocol rather than creating a second one:

- issue #3949 owns the repository development protocol and artifact boundaries;
- issue #3786 owns the exact staged-tree commit gate;
- issue #3985 owns affected committed-diff and pre-push proof routing;
- issue #7365 owns completion of the executable local diff-scoped RIPR path;
- issue #3987 owns thin current-head CI;
- issue #3988 owns merge fan-in and protected integration;
- issue #3989 owns post-merge reconciliation;
- live code, current GitHub state, accepted specs/policies, and the invoked skill remain
  authoritative for the current decision.

If this compiled route conflicts with a current accepted authority, follow the authority
and repair this skill.

## The graph

Enter at the earliest absent or stale useful judgment. Existing coherent work enters
midstream; do not replay nodes to manufacture chronology.

```text
concern or request
→ find-or-create-issue
→ research-issue
→ review-issue                 # should this work exist; owner, scope, vision
→ issue-to-plan
→ research-plan
→ review-plan                  # alternatives, acceptance, risk, vision
→ compile-spec                 # conditional: durable/cross-PR contract only
→ prepare-proof
   → spec-to-test
   → observed discriminating red
   → review-tests
→ build-candidate
   → build-from-proof
   → review-candidate          # implementation, reachability, product vision
   → improve-test-suite        # stronger and cheaper discrimination
   → simplify-candidate
→ prove-before-push            # affected proof, Changie, diff RIPR or honest boundary
→ publish-pr
→ address-review-comments      # human, bot, and candidate-owned CI findings
→ affected proof and affected review
→ final-challenge              # post-repair vision, reachability, residue, risk
→ review-pr                    # cumulative substantive judgment
→ verify-live-ci               # current-head integration facts
→ merge-reconcile              # protected merge/closeout and landed reconciliation
```

The graph is intentionally cyclic:

- material premise, owner, or claim change returns to `prepare-issue`;
- weak, circular, stale, or missing proof returns to `prepare-proof`;
- candidate-owned implementation or test failure returns to `build-candidate`;
- pre-push product/test failure or RIPR observation gap returns to candidate/proof work;
- accepted review findings route through `address-review-comments` and affected proof;
- substantive candidate mutation refreshes only affected review dimensions;
- a named prerequisite becomes a separate claim lane only when it gains an accountable
  owner and independently reviewable acceptance boundary;
- a GitHub-owned wait returns `IN_FLIGHT` with one exact wake event while the campaign
  advances independent work.

Use each invoked skill's routes, valid exits, or equivalent transition table. Skills own
procedure and next routing; the orchestrator owns the joined result.

## Encode durable state at the first useful boundary

Write information when another competent context would otherwise need to rediscover it.
Do not write runtime activity merely because a stage occurred.

| Durable surface | What it owns | Earliest useful write |
| --- | --- | --- |
| GitHub issue body | current problem, claim, owner, scope, non-goals, acceptance, plan, prerequisites, rollback | when research and plan have converged enough to guide proof/build work |
| GitHub issue comments | source-backed research, competing explanations, corrected assumptions, decision history | as soon as the evidence changes the plan or will matter later |
| `.spec/`, ADR, policy, schema, or contract | settled cross-PR/public behavior, semantic ownership, invariants, migration, proof obligations | only when the decision must outlive one PR or govern several consumers |
| Test, fixture, oracle, or proof artifact | executable discrimination and its limitations | once the intended wrong behavior has been observed red |
| `.changes/unreleased/` | user-visible/release-note disposition | when the user-visible effect is settled, while context is still fresh |
| Branch/worktree | one coherent candidate under one writer | when mutation begins |
| Local candidate result | exact committed range, selected/deferred affected proof, Changie, diff RIPR disposition, limitations | after `prove-before-push` executes or honestly classifies the boundary |
| PR body | cumulative claim, changed production path, proof, hardening, simplification, deviations, risk, limitations | when the candidate is coherent enough to publish |
| Inline review thread | localized finding and evidence-backed disposition | when the finding is stable and addressable |
| Submitted review | cumulative substantive judgment | after applicable lenses are joined against the current semantic candidate |
| GitHub checks and artifacts | clean-checkout, platform, policy, and integration facts for the evaluated head | when GitHub runs the selected current-head proof |
| Merge/issue closeout | landed effect, residual claims, support/proof/Changie reconciliation, safe cleanup | immediately after merge or deliberate closure |

Keep subagent handles, roles, Teams/Ultracode topology, queue order, retries, temporary
todo lists, proof-token allocation, polling, worktree liveness, and private reasoning
runtime-local. GitHub and repository artifacts reconstruct the route after compaction or
provider replacement; they do not mirror the runtime.

## Context and role continuity

Context, role, and skill are separate:

- **context** preserves the durable subject, source map, evidence, and worktree;
- **role** biases attention and default authority, such as claim owner or independent
  reviewer;
- **skill** supplies the executable procedure and typed next route.

A skill transition does not require a new subagent. A PR reviewer may consume several
review skills in one loaded context and may repair an accepted bounded finding in place
when write authority is granted and no same-candidate writer exists. The mutation still
returns through affected proof and review. Add another reviewer only when a different
source, oracle, method, threat model, environment, or attention surface can change the
judgment.

## Local feedback ladder

Place work at the earliest reliable input boundary:

```text
editing
→ exact staged-tree structural proof
→ affected committed-diff behavioral proof
→ late PR publication
→ current-head remote integration
→ protected merge
```

### Before commit

The installed pre-commit hook runs `cargo xtask precommit` against the exact staged tree.
The commit tier owns cheap deterministic structure: staged Changie fragment validation,
staged-blob rustfmt, whitespace/conflict markers, executable mode, structured syntax,
forbidden machine paths, oversized/binary policy, and other in-budget checks.

#9112 is the accepted authority for RIPR placement: diff-scoped new-gap enforcement moves
to this exact staged-tree boundary, over the one `git write-tree` candidate OID that #3786
established. The commit tier keeps its warm-median/p95/30s budget; promotion to blocking
requires measured exact-tree results, which is a budget to prove against rather than a
reason RIPR cannot belong here.

Until #9112's promotion and cutover conditions are met, the staged gate is non-blocking
and the remote `ripr+ New Gap Gate` stays required. Treat that as current migration state,
not a repository invariant. Do not reintroduce the retired categorical ruling that the
commit tier lacks an exact staged subject for this analysis, or that new-gap detection
structurally belongs after push. Cargo compilation stays out of the commit tier.

### Before push and publication

`prove-before-push` owns the candidate-facing skill boundary. The committed-diff policy
from #3985 owns the resolved base/head, affected Cargo closure, focused behavioral proof,
and Changie dry rendering. Use the repository planner, hook, and change-set resolver; do
not recreate base selection or package classification inside the skill.

New-gap RIPR placement belongs to #9112 at the staged-tree boundary. This boundary may
consume or disclose staged RIPR evidence where the accepted implementation supports it,
but must not re-establish a competing "RIPR belongs before push" authority. #7365 is
scoped to the pre-push affected-proof and Changie execution path on that basis.

The current `pre-push-plan` is planning-only. Until the accepted implementation lands, run
and validate the local receipts available at this boundary and preserve an exact
`NOT_PROVEN` or named remote-only boundary otherwise.

A local result is candidate evidence, not merge authorization. `NOT_PROVEN` remains
visible when the instrument, base/head identity, or required environment is unavailable.

### In GitHub

CI should confirm current-head and integration facts, not be the first place ordinary
staged or affected defects are discovered. Keep remote work for clean-checkout identity,
required policy, platform/packaging/external environments, merge-group interactions, and
other facts unavailable or untrusted locally.

Do not retire a remote required check merely because a local hook exists. Replacement
requires the #3987/#3988 parity, provenance, ruleset, merge-group, and alternate-path
proof. Until then, local proof shifts discovery left while GitHub remains the protected
backstop.

## Orchestrator use

After ingesting this graph:

- campaign root: invoke `deliver-goal`, then `orchestrate-work` for independent claims;
- persistent claim lane: invoke `deliver-pr`, retain context across skill transitions,
  and use `orchestrate-work` only for independent evidence or split claims;
- dedicated reviewer: consume the assigned review skills without loading the full graph
  unless promoted to claim ownership;
- focused subagent: execute only the named bounded skill/question;
- use Agent Teams only when lateral communication changes the result and Ultracode only
  inside one coherent claim.

Return from a claim lane only at a real remote wait, named prerequisite, terminal
reconciliation, durable hazard, external-action boundary, or precise `NOT_PROVEN`
boundary. Do not stop at an intermediate packet when the next skill is authorized and
useful.
