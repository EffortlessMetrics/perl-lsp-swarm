---
name: "source-command-wisdom-document"
description: "Wisdom step 3 — write findings to the right place"
---

# source-command-wisdom-document

Use this skill when the user asks to run the migrated source command `wisdom-document`.

## Command Template

# Wisdom: Document

Put your findings where they'll have the most impact.

## Before editing control-plane files

If your findings require editing `.codex/agents/`, `.agents/skills/`, or `AGENTS.md`, acquire the lock first:

```bash
AGENT_ID="wisdom-<issue-number>"   # e.g., wisdom-2566
scripts/control-plane-lock.sh acquire "$AGENT_ID"
```

If acquire fails (another agent holds the lock), do NOT retry in a loop. File your `gh issue comment` and `crates/*/AGENTS.md` updates without the lock — those are safe. Report the contention to the orchestrator for resolution.

Note: Edits to `crates/*/AGENTS.md` do NOT need the lock — each crate is isolated. Only `.codex/agents/`, `.agents/skills/`, and the root `AGENTS.md` require coordination.

## Where findings go

**Process improvements** → comment on the issue or PR:
```bash
gh issue comment <number> --body "## Wisdom Review
<your process findings — what worked, what to change>"
```

**Code patterns** → update the crate's AGENTS.md if relevant:
If you found something about how a crate works that future agents
should know, add it to the crate's AGENTS.md.

**Recurring patterns** → file a swarm improvement issue:
If the same kind of fix keeps coming up, or the same pipeline step
keeps being a bottleneck:
```bash
gh issue create --title "swarm: <pattern observed>" --body "<analysis>" --label "infrastructure"
```

**Agent skill improvements** → suggest updates:
If a step skill is missing guidance that would have helped, note the
specific skill and what to add.

## Rules

- Be specific. "Process could be better" is useless. "The scout's test
  spec didn't account for nested ternary which cost the builder 20 min"
  is actionable.
- Write for the next agent, not for a report. What would help them?
- One finding per location. Don't dump everything into one comment.
- **Close the learning loop.** Process insights should flow back to agent
  definitions and skill files, not just GitHub comments. If you found that
  scouts keep missing X, recommend updating scout-locate to check for X.
