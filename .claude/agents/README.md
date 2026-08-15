# Agent roster

Claude-native agent definitions. Skills own *procedure*; these own *authority and
lifetime*. A brief owns the subject.

That split is why this roster is small. The retired persona set (`f99d39d0d`) reached 38
files because it grew one agent per subject (`scout-parser`, `scout-lsp`, `scout-dap`,
`scout-find-*-gaps`) and one per phase (`red-tdd`, `green-tdd`, `green-refactor`,
`green-ci`). Subject belongs in the brief and phase belongs in `spec-to-test` and
`build-candidate`, so those collapse without losing a capability. Add a definition only
when an agent needs different **tools**, **model**, or **lifetime** — never because it
works on a different part of the tree.

## What an agent definition gives you that a skill cannot

Tool scope is mechanical. A reviewer without `Edit` and `Write` *cannot* mutate the
candidate, which is what makes "one writer per candidate" true rather than asserted, and
what stops a reviewer quietly repairing what it found and reporting clean.

Read the limit honestly: `Bash` is a write channel. An agent holding it can edit through
`git`, `sed`, or a heredoc regardless of its tool list. The retired `reviewer.md`
illustrated the trap — it declared no `Edit`/`Write` while its prose said "apply trivial
fixes directly," and it held `Bash`. Where the roster below grants `Bash`, the local
mutation prohibition is prose, and only the file-edit prohibition is mechanical.

## Cost is what an agent touches, not that it exists

| Touches | Cost | Practical limit |
| --- | --- | --- |
| GitHub reads/writes, source reads | negligible | attention, not capacity |
| A worktree | disk, plus a cold `target/` | one per genuine mutation claim |
| Builds and tests | CPU, file locks, cache | this is the real constraint |

A hot read-only lane investigating issues all day costs almost nothing. Two writers in
two worktrees is fine **when both claims are properly specified and disjoint** — the
precondition for a second writer is a spec, not a slot. Under-specified parallel writers
are what produce overlap, rework, and diagnostic cascades.

Never set `isolation: worktree` on a read-only agent. The retired roster set it on
`scout`, `reviewer`, and `red-tdd`, so every discovery pass allocated a worktree it never
wrote to.

## Cache economics

Two numbers govern every lifetime decision here:

```text
subagent prompt cache    5 minutes
orchestrator (main)      1 hour
```

Warm context is the expensive thing to rebuild, so the waste is never a live agent — it
is a *re-warmed* one. With a five-minute subagent window, that has sharp consequences:

- **keep a subagent alive only if its next task is under five minutes away.** Past that
  the cache is gone, and a respawn costs the same as re-warming without paying for the
  idle. "Long-running" has to mean *continuously busy*, not merely long-lived;
- **when an agent reports finishing, use it immediately or stop it.** Waiting ten minutes and
  then re-tasking the same agent is the worst available option — you pay the idle and the
  re-warm and gain nothing over a fresh spawn;
- **batch the queue.** Five tasks sent at once stay inside one warm window; five tasks
  sent over an hour pay five cold starts;
- **a long local build inside a subagent will outlive its own cache.** A cold workspace
  compile here runs about twelve minutes, so a proof agent that triggers one returns
  cold. Reuse a warm `target/`, or run the long build from the main thread where the
  one-hour window covers it;
- **the orchestrator's hour is the only durable warm context in the system.** Keep synthesis,
  claim state, and cross-lane judgment there, and spend subagents in bursts.

The state to avoid is **alive and unattended**. A steered lane is cheap; a lane idling
through a CI wait with nothing queued is paying rent on a cache that has already expired.

## Lateral communication

Agents can message each other and be messaged mid-flight. Use it for facts that
**invalidate a premise** — a merged PR, a corrected policy reading, a superseded claim —
not for status.

This removes the orchestrator-as-message-bus bottleneck. Where a fact would otherwise be
rediscovered independently by several lanes, prefer one long-running holder that the others
query, rather than each deriving it separately and disagreeing.

Because a running agent can be asked, silence is no longer opaque: query it before
reasoning about its artifacts, and before concluding anything about ownership. See
`FAILED_NO_RETURN` in `orchestrate-work`.

## The unit is a programme, not a skill

An agent is dispatched once over one coherent subject and walks an **ordered list of
skills** against it, loading each when it reaches that step.

```text
one context · one artifact set · one bounded purpose · several ordered skills · one durable result
```

Forking per skill is the mistake this replaces. A reviewer that moves from architecture
review to proof review should not lose its loaded candidate; a builder moving from
construction to hardening should not rebuild its understanding of the implementation.
Atomic skills change an agent's **attention**, not its identity.

Three owners, and they do not overlap:

| Owner | Owns |
| --- | --- |
| flow skill | which default programmes a situation warrants, and the join point |
| agent definition | authority, tools, model tier, lifetime, publication, return shape |
| atomic skill | the just-in-time question and method for one step |
| brief | the exact subject, the ordered skills, observed basis, falsifiers, return |

An orchestrator names *which* programme, never *how* to perform it. If a brief has to
explain the review method, that method now lives in whoever happened to be orchestrating
— which is the drift agent definitions exist to stop.

Deferred loading is the other half. A reviewer holds only the lens it is currently on;
four lenses' worth of instruction loaded at once compete for attention and sidetrack the
step in front of it.

### Sequence for bounded work, menu for open-ended work

**One-shot agents get an ordered checklist.** The subject does not move under them, so the
programme is knowable at dispatch. Write the skill list to `TodoWrite` on arrival and mark
each step as it completes.

The checklist is not bookkeeping — it makes partial completion *visible*. An agent that
stops after step two and reports reads as finished; a checklist showing two of five ticked
does not. That is the dead-lens problem caught at the agent rather than at the join, and
an unfinished list is what a return should carry as `not examined`.

**Long-running agents get a menu.** A standing researcher or a lane orchestrator meets
work whose shape is unknown at dispatch, so a fixed sequence would be wrong. Their
definition lists candidate skills with the condition that triggers each — names and
triggers only, which is cheap, since the body loads when the trigger fires.

The two compose rather than compete: a long-running agent uses its menu to **select** a
bounded unit of work, then issues itself a checklist to **execute** it.

## The roster

| Agent | Lifetime | Writes | Worktree | Programme |
| --- | --- | --- | --- | --- |
| `researcher` | one-shot or standing | GitHub only | no | research, archaeology, external truth, issue currency |
| `builder` | one candidate, while work is imminent | files | yes | `spec-to-test` → build → harden → simplify → proof → repair |
| `reviewer` | one subject, one programme | GitHub only | no (`git show <sha>` only; never mutates caller tree) | an ordered review programme over a fixed artifact |
| `lane-orchestrator` | one claim, steered | none by default | no | `deliver-pr`; selects programmes, dispositions findings |

The campaign orchestrator is the main session, not a file.

Everything else is a **mode** of one of these — external oracle, CI triage, main-health
holder, and live-policy holder are `researcher` assignments; specialist review is a
`reviewer` programme. Add a definition only for different tools, model, or lifetime,
never for a different subject.

Read-only agents return evidence, not approval. A subagent verdict never constitutes
review; independence requires a different source, oracle, method, threat model, or
environment, not merely a different agent.
