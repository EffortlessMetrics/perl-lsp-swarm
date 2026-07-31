# Crate Architecture Guide — retired compatibility pointer

This file was a v0.8.8-era combined crate inventory and Claude swarm-control guide. It is no longer a current architecture authority: the workspace, compiler-backed product model, crate boundaries, and agent runtime have all changed materially since it was written.

Use the current sources instead:

- [`../../CLAUDE.md`](../../CLAUDE.md) — current Claude repository operating contract;
- [`../../AGENTS.md`](../../AGENTS.md) — current provider front door and agent roster;
- [`../../docs/reference/ARCHITECTURE.md`](../../docs/reference/ARCHITECTURE.md) — current system and crate architecture;
- [`../../docs/reference/ORCHESTRATION_DOCTRINE.md`](../../docs/reference/ORCHESTRATION_DOCTRINE.md) — orchestration and routing model;
- [`../../docs/agents/IMPLEMENTATION_WORKER.md`](../../docs/agents/IMPLEMENTATION_WORKER.md) — provider-neutral implementation method;
- package-local `AGENTS.md` / `CLAUDE.md` files — domain ownership, constraints, and focused commands;
- current Cargo manifests and source — actual workspace membership and public crate seams.

Project `.claude/settings.json` does not grant shared shell permissions or enforce the development lifecycle, and the repository has retired project-level Claude/Codex hooks. GitHub issues, PRs, reviews, threads, checks, rulesets, and merges carry live transaction state; provider-native skills provide just-in-time procedure.

The original detailed guide remains available through Git history for archaeology. Do not use its crate metrics, performance claims, role catalogue, command list, hook guarantees, or worktree doctrine as current truth.
