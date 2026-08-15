# Claude provider adapter

Claude enters through the root [`CLAUDE.md`](../CLAUDE.md) and loads focused procedures from [`.claude/skills/`](./skills/).

## Active surfaces

- `CLAUDE.md` — complete Claude-native repository router and operating contract;
- `.claude/skills/` — six public flows, focused atomic transformations, reusable lenses, and optional mechanical operations;
- `.claude/agents/` — four programme-executing agent definitions (authority, tools, model, lifetime);
- `.claude/settings.json` — minimal portable shared settings only.

The main Claude thread is normally the warm accountable orchestrator. It may use focused subagents or Agent Teams when a different oracle, context, review direction, or genuinely distinct claim lane materially improves the result. Agent identity is not a lifecycle state, and no fixed lead/worker relay defines the development method.

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

## State boundaries

- GitHub issues, PRs, reviews, threads, checks, rulesets, and merges own live transaction state.
- Repository specs, ADRs, policies, tests, and method documents own durable contracts.
- Claude plans, task lists, teammate liveness, worktrees, model choices, retries, and local helper state remain runtime-local.

Do not recreate stage state through labels, command catalogues, persona rosters, hook telemetry, tracked current-goal pointers, or worktree-slot ownership records.

## Worktrees

Ordinary Git worktrees are candidate isolation, not claim or semantic-surface reservations. One writer mutates each current candidate branch/worktree at a time; distinct claim lanes use optimistic Git concurrency and own their actual merge or combined-tree repair.

The retained `worktree-manager` skill is an optional local reuse/cleanup helper. Its cache is disposable and never outranks Git or GitHub.

## Historical material

The removed `.claude/commands/` catalogue and the retired 38-file persona roster under `.claude/agents/` remain recoverable through Git history and archived research. They are donor history, not active runtime authority. The current four-definition roster in `.claude/agents/` is active.

The current shared method lives under [`docs/agents/`](../docs/agents/).
