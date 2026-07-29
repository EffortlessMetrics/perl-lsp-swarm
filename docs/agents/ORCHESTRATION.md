# Orchestration

## Preferred control pattern

For substantive work, one warm orchestrator normally owns the target, decisions, synthesis, GitHub state, and continuation. It dynamically assembles the smallest executor subgraph that improves elapsed time, evidence quality, context efficiency, steering, or recovery.

This does not mean one model personally performs every pass. It means one accountable control context integrates the work.

## Four separate graphs

### Artifact and authority graph — durable

```text
vision and product direction
→ issue and research
→ current synthesis and plan
→ durable contract where warranted
→ executable proof
→ candidate branch
→ pull request
→ reviews and findings
→ checks
→ merge
→ reconciliation
→ remaining claim
```

It also records semantic owners, consumers, dependencies, proof obligations, support boundaries, and acceptance-and-rollback claims.

### Flow and skill graph — durable

```text
deliver-goal
→ deliver-pr
→ prepare-issue
→ prepare-proof
→ build-candidate
→ finish-pr
```

Atomic skills define focused questions, evidence, local successors, and material backward routes.

### Evidence and currentness graph — durable or reconstructable

Evidence identifies:

- the semantic subject and claim;
- the candidate where applicable;
- the oracle or source;
- what the evidence establishes and does not establish;
- what later change invalidates it.

### Executor graph — ephemeral

```text
orchestrator
├── source mapper
├── related-work researcher
├── external oracle
├── plan challenger
├── proof designer
├── test adversary
├── implementation worker
├── risk or domain reviewer
├── CI triager
└── merge reconciler
```

Agents, models, task lists, liveness, retries, worktrees, and provider topology are runtime choices. Do not make them durable lifecycle authority.

## Proportional execution shapes

### Tiny or mechanical work

The root or one whole-flow worker performs the relevant skill directly.

### Ordinary substantive work

```text
warm orchestrator
├── parallel read-heavy research, oracle, proof, or review questions where useful
├── one integrating writer for the contested candidate
└── differentiated candidate and formal review
```

### Broad coherent epic

The orchestrator prepares and bounds one coherent acceptance-and-rollback claim, delegates the implementation to one whole-flow executor or dynamic workflow, then resumes proof hardening, simplification, review, and GitHub convergence.

### Multi-PR programme

The orchestrator reads the selected umbrella and current GitHub state, chooses one coherent claim, carries it through `deliver-pr`, reconciles the umbrella, and continues while another useful claim remains.

There is no repository-global current-goal pointer and no build-all-eligible scheduler.

## Orchestrator algorithm

1. Resolve the user-selected issue, PR, or durable outcome.
2. Reconstruct only the relevant current artifact graph.
3. Select the narrowest public flow and current atomic skill.
4. Identify independent questions, the join decision, and the contested write surface.
5. Choose the cheapest effective execution topology.
6. Launch or continue agents with the target, starting skill, authorities, and result boundary.
7. Guide, redirect, retry, or replace executors as evidence changes.
8. Preserve contradictions until evidence resolves them; do not count votes.
9. Update the durable issue, proof, PR, review, check, or closeout surface.
10. Follow the local successor or material backward route.
11. Continue until reconciliation or a real blocker remains.

## Mutation and concurrency

- one integrating writer owns each contested branch, worktree, or semantic authority;
- many independent readers and reviewers may fan out;
- genuinely disjoint PR claims may have separate writers and worktrees;
- a reviewer who applies a repair becomes an author of the resulting candidate;
- waiting CI or review does not freeze unrelated work;
- actual collision, not conceptual proximity, determines serialization.

## Externalization boundary

Persist what another session, provider, reviewer, or integrator needs:

- material issue research and corrected assumptions;
- current synthesis and plan;
- durable contract decisions;
- proof and limitations;
- candidate claim and review index;
- formal findings and dispositions;
- check and instrument state;
- merge closeout and remaining work.

Keep runtime-local:

- active agent liveness and task assignment;
- expected join order;
- model routing and token economics;
- retries and raw logs;
- provisional reasoning;
- temporary worktree bookkeeping.
