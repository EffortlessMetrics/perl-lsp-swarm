# Claude repository adapter

Claude uses the repository's durable development method through provider-native surfaces:

- `CLAUDE.md` — complete Claude repository operating contract;
- `.claude/skills/` — public flows, atomic transformations, and focused operations loaded just in time;
- package-local `CLAUDE.md` files — domain ownership, commands, and constraints;
- GitHub issues, PRs, reviews, threads, checks, and merges — live transaction state.

Project `.claude/settings.json` is intentionally minimal. Personal permission posture, command allowlists, model routing, experimental features, and provider-specific convenience settings belong in user or local configuration. The repository does not promise that `gh`, Cargo, or other shell commands are pre-authorized.

The repository no longer uses project hooks, task-completion gates, subagent lifecycle gates, private swarm metrics, fixed pipeline leads, or a tracked current-stage/active-goal surface as development authority. Formatting, proof, review currentness, required checks, merge protection, and reconciliation run at coherent candidate and GitHub boundaries.

## Operating model

```text
current repository and GitHub state
→ narrowest public flow
→ focused JIT skill
→ evidence-backed result and local route
→ one integrating writer for contested mutation
→ protected merge and reconciliation
```

The main Claude thread is normally the warm accountable orchestrator. Focused subagents, context forks, Agent Teams, and worktrees are runtime choices used when they improve evidence or permit genuinely disjoint work; they are not lifecycle nodes.

Legacy command and persona catalogues remain temporary migration donors until the provider-native skill and router cutover removes them from active discovery. Historical versions remain available through Git history and repository archives.
