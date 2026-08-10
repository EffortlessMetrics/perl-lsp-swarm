# ADR-0032: Skill Scoping and Hook Enforcement for Swarm Orchestration

**Status**: Superseded
**Date**: 2026-03-16
**Superseded**: 2026-07-29 by the provider-native GitHub-first skill architecture (#5199, #5204, #4203)
**Related**: PR #1707, [SKILL_AND_AGENT_DESIGN.md](../reference/SKILL_AND_AGENT_DESIGN.md)

---

## Historical context

This ADR responded to three real swarm-cycle failures:

1. long-lived orchestrator context was polluted with worker-specific procedure;
2. prompt requests for private swarm metrics were routinely skipped;
3. repeated command boilerplate increased prompt cost and still failed to load the intended standards reliably.

It therefore separated orchestrator and worker skills, proposed `TaskCompleted`, `SubagentStart`, and `TeammateIdle` hooks, and used skill frontmatter as role-oriented access control.

## Why it is superseded

The historical diagnosis was useful, but the chosen enforcement boundary was not durable.

- Task, subagent, teammate, model, and executor state are private provider runtime state, not repository authority.
- The original hook promises were only partially implemented; later divergence notes already recorded that metrics enforcement never matched the decision and `TeammateIdle` was unsafe.
- Hook failures can block useful work after the economically valuable decision point and encourage compliance with recorded choreography rather than improving the artifact.
- Fixed orchestrator/worker identities and slash-command catalogues have been replaced by one warm accountable root, provider-native JIT skills, and optional differentiated help.
- Formatting, proof, formal review currentness, required checks, merge protection, and reconciliation belong at coherent candidate and GitHub boundaries.
- Personal permission posture and command authorization belong to user/provider configuration, not shared repository settings.

## Current decision

The repository uses this control model:

```text
current repository and GitHub state
→ narrowest public flow
→ focused JIT transformation or review lens
→ evidence-backed local route
→ one integrating writer for contested mutation
→ protected merge and reconciliation
```

### Skills

Skills remain the procedure surface, but their scope is semantic rather than persona-based. Public flows and atomic skills declare their trigger, authoritative inputs, result, evidence boundary, and normal/backward routes. The same warm root may perform several transformations; another agent is useful when it changes the evidence or detection surface.

### Hooks

No project-level Claude or Codex lifecycle or command-policy hook is authoritative. The repository does not use hooks to:

- authorize task completion;
- enforce issue labels or stage markers;
- inject a fixed role catalogue;
- require private metrics;
- decide whether an agent may continue;
- replace candidate proof, GitHub reviews, required checks, or branch protection.

Concrete hazards remain controlled at their native boundary: Git/GitHub protection, expected-head merge, writer-admission/collision checks, secret/release controls, current required checks, unresolved substantive review findings, and durable-contract validation.

### Runtime configuration

Shared project settings remain minimal and portable. Personal permissions, bypass modes, broad command allowlists, experimental provider features, model routing, and local conveniences belong in user or local settings.

## Consequences

**Positive**

- useful research, proof, hardening, simplification, and review happen immediately before the decisions they improve;
- missed historical ceremony can be repaired forward without rejecting coherent work;
- GitHub preserves asynchronous state across providers and sessions;
- no task/subagent hook can deadlock ordinary progress;
- provider-native skills can evolve without preserving a fixed organization chart.

**Trade-offs**

- the repository no longer claims mechanical command authorization through project settings;
- provider/user permissions differ by runtime and must be discovered honestly;
- review and proof discipline depend on candidate-bound helpers, GitHub protection, and substantive skill execution rather than lifecycle hooks;
- historical command/persona donors remain useful only until their semantics are absorbed and active discovery is retired.

## Historical record

The original accepted text and implementation divergence remain available through Git history and PR #1707. They are not current operating authority.
