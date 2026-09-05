# Agent roster

Claude-native agent definitions. Skills own *procedure*; agent definitions own bounded
**authority, tools, model, and useful lifetime**. A brief owns the subject. The main
Claude thread owns orchestration.

The retired persona set (`f99d39d0d`) reached 38 files because it grew one agent per
subject (`scout-parser`, `scout-lsp`, `scout-dap`, `scout-find-*-gaps`) and one per
phase (`red-tdd`, `green-tdd`, `green-refactor`, `green-ci`). Subject belongs in the
brief and phase belongs in skills, so those collapse without losing a capability.

The later `lane-orchestrator` profile made a different category mistake: it duplicated
the main thread's orchestration authority. A claim/lane is now a **logical frame retained
by the main thread**, not an agent role. Add an agent definition only when tools, model,
mutation authority, evidence posture, or useful lifetime differ—not because work belongs
to another claim or SDLC stage.

## What an agent definition gives you that a skill cannot

Tool scope can create a real mechanical boundary. A reviewer without `Edit` and `Write`
cannot use those tools to mutate the candidate; a builder can be isolated to one
candidate worktree; a researcher can stay read-only with no worktree.

Read the limit honestly: `Bash` is a write channel. An agent holding it can edit through
`git`, `sed`, or a heredoc regardless of its tool list. Where Bash is granted, local
mutation restrictions remain partly instructional and must be checked against the actual
sandbox/permission mode.

## Cost is what an agent touches

| Touches | Cost | Practical limit |
| --- | --- | --- |
| GitHub/source reads | low | root attention and provider budget |
| A worktree | disk, plus build/cache footprint | one per genuine mutation claim |
| Builds and tests | CPU, file locks, cache | the real local capacity constraint |

Read-only investigation does not need a worktree. Two writers in two worktrees are fine
when their claims are properly specified and independent; the precondition for a second
writer is a coherent claim and safe admission, not a global slot count.

## Context and lifetime

Warm context is valuable, so avoid both extremes:

- do not respawn once per atomic skill when one coherent programme is still operating on
  the same subject/artifact set;
- do not keep a completed agent alive merely to represent a remote wait;
- batch related work while its loaded context is useful;
- use the main thread for durable synthesis, claim frames, contradictions, route
  selection, and cross-claim judgment;
- let high-volume evidence, mutation, and fixed-subject review live in bounded contexts.

Exact provider cache durations are runtime observations, not repository invariants. The
architectural rule is simpler: **the main thread is the durable orchestration context;
subagents are bounded execution contexts.**

## Lateral communication

Use lateral communication only when it changes the result—for example a premise-
invalidating fact, an actual dependency, or a Team whose specialists must coordinate.
Do not create peer status chatter or a second orchestration graph.

A child may tell another child a premise changed, but decision-changing evidence must
still reach the main thread or its durable GitHub/repository surface. Lateral messaging
does not turn a worker into a subordinate orchestrator.

## The unit is a programme, not a skill

An agent may be dispatched once over one coherent subject and walk an ordered list of
skills against it, loading each when it reaches that step.

```text
one context · one artifact set · one bounded purpose · several ordered skills · one return
```

Forking per skill is usually wasteful. A reviewer that moves from claim review to proof
review should not lose its loaded fixed candidate; a builder moving from construction
to hardening should not rebuild its understanding of the implementation. Atomic skills
change an agent's **attention**, not its identity.

Ownership stays separated:

| Owner | Owns |
| --- | --- |
| main Claude thread | orchestration, logical claim frames, joins, dispositions, continuation |
| public flow | which SDLC concern cluster applies and its normal routes |
| agent definition | bounded authority, tools, model, lifetime, publication/return envelope |
| atomic skill | just-in-time question and method |
| brief | exact subject, ordered programme, established facts, falsifiers, return |

An orchestrator names which programme is needed; it does not rewrite the programme's
method into the brief.

## The roster

| Agent | Lifetime | Writes | Worktree | Programme |
| --- | --- | --- | --- | --- |
| `researcher` | one-shot or standing while continuously useful | GitHub only | no | research, archaeology, external truth, CI/main/issue currency |
| `builder` | one candidate while mutation work is imminent | files | yes | proof → build → harden → simplify → affected proof/repair |
| `reviewer` | one fixed subject, one review programme | GitHub only | no (`git show <sha>`; never mutates caller tree) | differentiated review over a fixed artifact |

The main Claude thread is the orchestrator; it is intentionally not represented by an
agent profile.

Everything else is a mode or execution technique. External oracle and CI triage are
`researcher` assignments; specialist review is a `reviewer` programme; context forks,
Ultracode, and Agent Teams are provider-native execution choices selected by the main
thread when useful.

Read-only agents return evidence, not approval. Independence requires a changed source,
oracle, method, threat model, environment, or meaningful attention surface—not merely a
different agent identity.
