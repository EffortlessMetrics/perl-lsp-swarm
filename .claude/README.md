# Claude provider adapter

Claude enters through the root [`CLAUDE.md`](../CLAUDE.md) and loads focused procedures from [`.claude/skills/`](./skills/).

## Active surfaces

- `CLAUDE.md` — complete Claude-native repository router and **root orchestration** contract;
- `.claude/skills/` — six public flows, focused atomic transformations, reusable lenses, and optional mechanical operations;
- `.claude/agents/` — three bounded programme-executing agent definitions: researcher, builder, reviewer;
- `.claude/settings.json` — minimal portable shared settings only.

The main Claude thread is the warm accountable orchestrator. It retains the durable goal interpretation and runtime-local logical claim frames, then uses bounded researcher, builder, reviewer, context-fork, Ultracode, or Team execution only where another context or communication pattern materially improves the work. Agent identity is not lifecycle state, and a substantial claim does not normally create another orchestrator.

## Public flows

```text
deliver-goal
deliver-pr
prepare-issue
prepare-proof
build-candidate
finish-pr
```

Each flow enters from live GitHub and repository state, follows locally named skill routes, and continues until the requested outcome is reconciled, left explicitly in flight under GitHub authority, or bounded by a real blocker or `NOT_PROVEN` evidence.

`deliver-goal` and `deliver-pr` run in the main/root orchestration context. A claim/lane is a logical frame held there; bounded agent programmes execute research, mutation, and review work without becoming subordinate orchestration authority.

## State boundaries

- GitHub issues, PRs, reviews, threads, checks, rulesets, and merges own live transaction state.
- Repository specs, ADRs, policies, tests, and method documents own durable contracts.
- Claude claim-frame ordering, plans, task lists, teammate liveness, worktrees, model choices, retries, and local helper state remain runtime-local.

Do not recreate stage state through labels, command catalogues, persona rosters, hook telemetry, tracked current-goal pointers, worktree-slot ownership records, or persistent executor hierarchies.

## Worktrees

Ordinary Git worktrees are candidate isolation, not claim or semantic-surface reservations. One writer mutates each current candidate branch/worktree at a time; distinct claims may use optimistic Git concurrency and own their actual merge or combined-tree repair.

The retained `worktree-manager` skill is an optional local reuse/cleanup helper. Its cache is disposable and never outranks Git or GitHub.

## Historical material

The removed `.claude/commands/` catalogue, retired 38-file persona roster, and retired `lane-orchestrator` profile remain recoverable through Git history and archived research. They are donor history, not active runtime authority. The active roster is researcher + builder + reviewer; orchestration lives in the main Claude thread.

The current shared method lives under [`docs/agents/`](../docs/agents/).
