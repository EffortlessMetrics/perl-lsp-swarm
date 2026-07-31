# Skill and agent architecture

The repository encodes the **work**, not a permanent organization chart of workers.

The durable architecture is:

```text
current repository and GitHub artifact
→ focused provider-native skill
→ evidence-backed disposition
→ locally named next skill or material backward route
```

Claude, Codex, a focused subagent, a provider-native team, a deterministic checker, or the same warm root session may perform a transformation. Agent identity is runtime choice, not lifecycle state or durable authority.

## Active provider surfaces

```text
AGENTS.md
  complete Codex-facing root router

.agents/skills/
  Codex-native public flows and atomic skills

CLAUDE.md
  complete Claude-native root router

.claude/skills/
  Claude-native public flows and atomic skills

GitHub
  live issues, PRs, reviews, threads, checks, rulesets, and merges
```

The six public flows are:

- `deliver-goal`;
- `deliver-pr`;
- `prepare-issue`;
- `prepare-proof`;
- `build-candidate`;
- `finish-pr`.

Atomic skills provide JIT research, planning, proof, implementation, hardening, simplification, review, repair, merge, and reconciliation guidance. Reusable lenses change the question or oracle without becoming mandatory lifecycle stages.

The canonical shared method and contracts live in:

- [`docs/agents/DEVELOPMENT_METHOD.md`](../agents/DEVELOPMENT_METHOD.md);
- [`docs/agents/SKILL_CONTRACT.md`](../agents/SKILL_CONTRACT.md);
- [`docs/agents/GITHUB_SURFACES.md`](../agents/GITHUB_SURFACES.md);
- [`docs/agents/REVIEW_CURRENTNESS.md`](../agents/REVIEW_CURRENTNESS.md).

## Root and child ownership

The root user session is normally the warm accountable orchestrator. It may research, plan, prove, implement, review, repair, merge, reconcile, and continue through several skills.

An explicitly spawned child owns only its supplied brief. Focused children are useful when a different oracle, context, tool, review direction, or genuinely distinct claim lane improves the result. A fresh identity alone is not an independent control.

The root owns decisions, contradiction-preserving synthesis, durable GitHub updates, and continuation. Child liveness, join order, retries, model routing, and temporary task state remain runtime-local.

## Candidate and worktree boundary

One writer mutates each current candidate branch/worktree at a time.

Distinct claim lanes use ordinary optimistic Git concurrency. They may edit the same files, crates, or nearby semantics and remain active simultaneously. Do not build a file reservation, semantic-surface ownership, overlap map, or sibling-lane surveillance system. If Git reports a conflict, an explicit prerequisite changes, or actual combined-tree proof fails, the affected lane owns its smallest coherent repair and refreshes only affected proof/review.

Use a worktree when separate PR-shaped mutation or independent validation needs isolation. Do not create another worktree merely because attention moved from research to proof or proof to implementation.

The optional `worktree-manager` skill may assist with local reuse and cleanup. Its cache is disposable runtime bookkeeping and never outranks Git or GitHub.

## State boundaries

Repository artifacts own durable product, architecture, method, and proof contracts.

GitHub owns live transaction state:

- issues — problem, research, corrections, current synthesis, plan, dependencies, next action;
- PRs — one coherent acceptance-and-rollback candidate;
- reviews and threads — formal findings and evidence-backed dispositions;
- checks, rulesets, queue, and mergeability — current integration evidence;
- merge/closeout — landed result, residual work, next claim.

Runtime owns temporary agent assignments, tasks, liveness, worktree bookkeeping, raw logs, retries, and model choices.

Do not mirror runtime topology into labels, tracked active-goal files, persona catalogues, command relays, hook telemetry, queue files, or durable worktree-owner records.

## Skills versus deterministic controls

A skill is appropriate when focused context and judgment can improve the next decision. It should name authoritative inputs, procedure, what the pass establishes, what remains unproved, valid exits, and the normal next route.

A deterministic hard gate is appropriate only at a concrete preventable hazard, such as:

- concurrent mutation of the same candidate branch/worktree;
- destructive loss of unsalvaged work;
- unknown candidate or material-claim identity;
- secret or unsafe publication;
- structurally invalid durable contracts;
- unresolved substantive review findings;
- red required checks or repository policy at merge.

Do not use project lifecycle hooks, task-completion gates, agent-role permissions, or magic labels to prove that a useful pass occurred.

## Historical generations

The removed `.claude/commands/`, `.claude/agents/`, `source-command-*`, and `.codex/agents/` catalogues remain recoverable through Git history and archived research. They preserve how the earlier swarm made useful transformations explicit, but they are no longer active discovery or runtime authority.
