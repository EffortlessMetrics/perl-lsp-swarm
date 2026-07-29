# Crate Architecture Guide — retired compatibility pointer

This file was a v0.8.8-era combined crate inventory and Claude swarm-control guide. It is no longer a current architecture authority: the workspace, compiler-backed product model, crate boundaries, and agent runtime have all changed materially since it was written.

Use the current sources instead:

- [`../../ARCHITECTURE.md`](../../ARCHITECTURE.md) — current system and crate architecture;
- [`../../START_HERE.md`](../../START_HERE.md) — repository orientation and reading order;
- [`../../AGENTS.md`](../../AGENTS.md) and [`../../CLAUDE.md`](../../CLAUDE.md) — current provider front doors;
- [`../agents/DEVELOPMENT_METHOD.md`](../agents/DEVELOPMENT_METHOD.md) — provider-neutral development method;
- [`../agents/ORCHESTRATION.md`](../agents/ORCHESTRATION.md) — within-claim orchestration and one-writer control;
- package-local `AGENTS.md` / `CLAUDE.md` files — domain ownership, constraints, and focused commands;
- current Cargo manifests and source — actual workspace membership and public crate seams.

Project `.claude/settings.json` does not grant shared shell permissions or enforce the development lifecycle, and the repository has retired project-level Claude/Codex hooks. GitHub issues, PRs, reviews, threads, checks, rulesets, and merges carry live transaction state; provider-native skills provide just-in-time procedure.

The original detailed guide remains available through Git history for archaeology. Do not use its crate metrics, performance claims, role catalogue, command list, hook guarantees, or worktree doctrine as current truth.
