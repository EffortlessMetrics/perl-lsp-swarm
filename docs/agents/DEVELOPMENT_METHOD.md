# Development method

## Purpose

This repository uses a shift-left, review-forward, orchestration-heavy development
loop. The main/root provider thread governs goals and logical claim frames while bounded
researcher, builder, reviewer, fork, worker, workflow, or Team contexts perform useful
investigation, proof, mutation, repair, and review.

Useful passes are controls. Permanent personas, subordinate claim orchestrators,
lifecycle labels, tracked frontiers, agent liveness, completion hooks, exact-head review
receipts, and stage files are not.

```text
current durable artifact
→ selected provider-native route
→ root-held claim frame
→ bounded execution and challenge
→ root joins evidence or candidate delta
→ useful GitHub/repository update
→ next named route
```

## Governing law

**Default-complete, recovery-forward.**

For substantive work, perform every applicable research, planning, proof, hardening,
simplification, review, and reconciliation judgment before the next more expensive
artifact. When an earlier judgment was missed, perform the cheapest version that can
still improve the current artifact and continue. Do not replay history to manufacture
process evidence.

## Queue-only qualification and publication

The October 21 native-queue selection and October 22–23, 2026 clarification in
[#3390](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3390), #17900 and
#17903 supersede the October 17 manual H/S operating sequence. Follow
[qualification-cutover.md](../ci/qualification-cutover.md) for source, enforcement,
workflow enablement, first installation and rollback. A source candidate is not an
activated generation.

```text
cheap commit / push / Draft-or-Ready publication, with zero workflow starts
→ focused/affected proof, substantive review and joined repairs in-session
→ exact-H product/scope judgment and conditional landing authorization
→ verified native enqueue
→ selected integration qualification on actual merge-group G
→ protected native landing
→ cheap landed M read-back and reconciliation
```

Draft, Ready, synchronization, metadata edits, labels, reviews, comments and generated
PR publication are not qualification requests. Skipped jobs and cheap wrappers still
start workflows. Complete applicable proof, hardening, simplification, documentation
and substantive review in the coding session before enqueue. Preserve the useful warm
owner, worktree and toolchain context; a stage does not require a cold replacement.
Independent objectives can proceed within admitted capacity without a global worker cap.

The owner authorizes the reviewed exact H and its product/scope fit before native
enqueue, because native queue admission can lead directly to landing. Do not add a
mandatory post-green approval pause or issue a second direct merge. A new material
finding withdraws eligibility and returns to the specific repair owner. Native queue
reformation produces a new G; old run/attempt evidence never proves the new subject.
Unrelated base movement does not require rebase or broad replay of unchanged proof.

[#17900](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17900) owns actual
admission and trusted H publication; #17903 owns the full direct and indirect event
perimeter; #17080/#10065 own coordinated activation and native canary. Consume the
implemented, qualified operation. `native_queue.py` supplies exact-head enqueue
transport to a trusted in-process authorizer and read-only observation, not a CLI
authorization override, a full G-member acquirer or a complete deployment. Missing
publisher, required G producer, effective protection or workflow-state capability is
`NOT_PROVEN`. Continue useful in-session work and safe source preservation; do not
push, toggle Ready or dispatch ordinary CI as a bootstrap.

Bind H, trusted control, actual G/base/constituents, workflow/run/attempt, selected scope
and landed M separately. Preserve all applicable protected names and application
bindings. Acknowledgment is not queue custody; association is not complete G membership;
empty, skipped, cancelled or wrong-subject evidence is not passing proof. Every selected
candidate/G obligation must succeed independently of main health. The source-only
staged perimeter and finalizer remain separate from native execution and protection.

Retain the same useful owner through ordinary queue waits, result inspection and M
reconciliation. Use supported events or bounded checks, not a polling child or recurring
monitor. A real interruption or exhausted window gets an exact handoff of subject,
run/attempt, proof limits, next action and wake event. Preserve durable evidence so the
work can survive disposal of its VM; warm context is useful execution state, not the
only owner of the result.

Target normally ≤20 minutes for complete active qualification, including setup,
compilation, transfer, verification and finalization; report queue delay, retries and
machine-minutes separately. Keep mandatory and uniquely useful proof. Time or merge
count does not authorize routine main replay. Keep separately admitted release,
security, deep, incident and cache purposes distinct; actual Pages deployment is
publication. No protection, credential or security-permission change is authorized
by this guidance alone.

**Review tooling boundary:** `ub-review.yml` remains paused. Provider-native substantive
review runs in-session; historical job IDs and calibration data do not establish an
active reviewer or a deployed Tower controller.

## Operational authority

```text
Claude Code
→ CLAUDE.md
→ .claude/skills/*

Codex
→ AGENTS.md
→ .agents/skills/*
```

This document states shared invariants. It is not the runtime router.

## Durable graph and ephemeral execution

Durable nodes include goals, issues, accepted contracts, proofs, candidates, findings,
decisions, reviews, checks, merges, and residual claims.

Ephemeral nodes include root claim-frame ordering, workers, subagents, forks, Teams,
worktrees, task lists, wake events, retries, provider/model choices, raw logs, and
provisional reasoning.

Encode the work, not the workers. Persist useful conclusions through their owning
issue, PR, review, check, merge, or contract. Do not persist executor topology.

## Root orchestration

The main/root thread is the accountable orchestrator across both goal and claim scopes.
It owns:

- goal source and current interpretation;
- acceptance predicates and required claims;
- selection and ordering of claims;
- cross-claim dependencies and contradictions;
- runtime-local claim frames and wake events;
- evidence joins and finding disposition;
- candidate writer allocation;
- review sufficiency, integration judgment, and reconciliation.

A claim/lane is a **logical frame retained by the root**:

```text
claim
acceptance predicate
durable subject
current candidate / writer
current route or missing judgment
proof and review state / limitations
external wait
wake event
```

The root may switch among these frames. A claim reaching a GitHub-owned wait remains
`IN_FLIGHT` without a live representative. The frame is reconstructed from durable
artifacts after compaction or replacement.

A substantial claim does not normally create a subordinate orchestrator. The root keeps
orchestration and delegates bounded programmes.

## Bounded programme contexts

### Researcher / read-only worker

Use for source ownership, archaeology, external truth, CI/log triage, broad inventories,
currentness checks, or other high-output-to-answer work.

### Builder / writer

Use for one current candidate or proof mutation programme. One writer owns a candidate
branch/worktree at a time.

### Reviewer

Use for one fixed subject and a differentiated review programme. Reviewers change the
evidence surface through source/oracle/method/threat/environment/attention, not merely
through identity.

### Direct root work

Use for tiny or tightly coupled decisions where delegation changes no evidence surface
and briefing/join cost exceeds its value.

### Provider-native forks, nested agents, workflows, and Teams

These are physical execution techniques. Use them when inherited context, dynamic task
readiness, different tools, or lateral communication materially improves a bounded
programme. They do not acquire logical claim-orchestration authority and are not a
required repository topology.

## Programme continuity

A programme may span several ordered atomic skills while one context remains useful:

```text
researcher
→ research-issue / external truth / related evidence

builder
→ spec-to-test / build-from-proof / improve-test-suite /
  simplify-candidate / affected repair

reviewer
→ issue / plan / proof / candidate lenses over one fixed subject
```

Do not fork once per skill when the same subject and artifact understanding remain
load-bearing. Atomic skills change attention, not identity.

Conversely, use a fresh context when a different source, oracle, threat model,
environment, tool boundary, or independence property is the point.

## Route discipline

The root names the route and then runs it:

```text
deliver-goal
→ select root-held claim frame
→ deliver-pr
→ orchestrate-work
   ├── researcher programme(s), if useful
   ├── one writer / build-candidate programme
   └── reviewer programme(s)
→ root joins evidence
→ finish-pr
→ return to deliver-goal / caller
```

Children receive the parent route, exact durable subject, selected skill, established
facts, authority, read/write boundary, falsifiers, sufficient return, backward routes,
stop conditions, and non-goals.

Do not give children an invented lifecycle when repository skills already define the
method. Do not grant generic claim-orchestration authority merely because a task is
substantial.

## Runtime-local claim frames

The root may keep an in-context table of claim, goal predicate, durable subject,
candidate/writer, current judgment, next material action, external wait, and wake event.

This table is not committed or posted. Reconstruct it from current issues, PRs,
submitted reviews, checks, merges, and repository facts after compaction/replacement.

For an ordinary accepted qualification run, preserve the same-owner continuity
described above. When a real interruption or out-of-window dependency owns the next
transition, retain the frame as `IN_FLIGHT`, release unnecessary contexts and advance
another actionable claim. A wake event is a resumability condition, not a scheduled
monitor. Do not create a timer, cron, polling child or idle watcher for ordinary work.
When no claim is actionable because of real external blockers, report their precise
conditions and continuation instead of keeping a root session alive indefinitely.

## Orchestration economics

Delegate when expected evidence gain, root-context preservation,
dependency-unlocking value, elapsed-time gain, changed detection surface, or avoided CI
cost exceeds cold-start, briefing, duplicated research, resource contention, join, and
correlated-failure costs.

High-output-to-answer work is normally delegated: CI/log triage, corpus/repository
sweeps, dependency/API audits, external-source collection, failure bisection, broad
inventories, proof adversaries, and specialist review.

Maximal fan-out is not sophistication. Stop adding agents when another result cannot
change a decision.

## Graph-delta evidence

Workers return subject/basis, conclusion, direct evidence/authority, scope searched,
contradiction/uncertainty, what is/is not established, affected claim/proof/authority
edge, recommended route, `NOT_PROVEN` boundary, and stable overflow references.

Writers add candidate identity, changed behavior/seams, proof run/not run, repaired
findings, limitations, and typed result.

The root joins evidence rather than votes. Repeated claims from one source remain one
evidence path. Builder self-report is not independent proof. Contradictions remain
visible until resolved.

A dispatched review dimension that never returns remains `NOT_PROVEN`, not
examined-and-clean.

## GitHub-native durable updates

Use GitHub when information is useful later:

- issue correction, current synthesis, plan, decision, route, or dependency;
- material prerequisite, supersession, or actual interaction;
- PR claim/proof/limitation/deviation;
- inline finding, submitted review, or evidence-backed disposition;
- remote wait needed for another operator's handoff;
- merge/closure effect and residual work.

When another context benefits and the intended route is not already obvious, one
compact route declaration may record parent goal, claim, entry flow, current named
transition, reason, durable subject, and wake event. Update it only when the material
route changes. It is a resumability aid, not a stage record or runtime state mirror.

Do not write root claim-frame ordering, agent assignments/liveness, skill completion,
polling, transcripts, provisional reasoning, or duplicate unchanged summaries.

## Public flows

| Flow | Normal root responsibility and outcome |
| --- | --- |
| `deliver-goal` | manage the durable outcome, acceptance predicates, and root-held claim frames |
| `deliver-pr` | focus on one claim frame and carry it through its current SDLC route |
| `prepare-issue` | settle problem/owner/scope/plan with bounded research/review programmes as useful |
| `prepare-proof` | establish/challenge discriminating proof with one proof mutation owner where needed |
| `build-candidate` | drive one candidate writer plus hardening/simplification/challenge programmes |
| `finish-pr` | converge the selected PR through repair, review, integration, merge, closeout |

Public flows are root-facing. Atomic skills may run in the root or in bounded
researcher/writer/reviewer programmes. This is a semantic method, not a fixed physical
executor graph.

## Claim independence and optimistic concurrency

```text
one coherent claim
→ one current candidate
→ one branch/worktree
→ one writer
→ one PR
```

Different claims may touch the same files/crates and proceed concurrently. Coordinate
only duplicate claims, same-candidate writers, explicit prerequisites, destructive
shared runtime state, actual conflicts, or demonstrated combined-tree interactions.
Behind-only movement requires no action.

### The exclusive role is writer, not reader

Mutation is exclusive; reading is not. Arriving at an existing candidate to research,
review, challenge, or comment requires no claim, no check for a current writer, and no
wait. Duplicate review is cheap and buys a second detection surface from
changed attention, which is what substantive review asks for and cannot get from
identity alone. A session that finds another writer already on a candidate may review
it and contribute evidence; it does not push or need to wait to review.

Independent reviewing roots publish useful findings to the durable GitHub surface.
Bounded review workers return findings to their accountable root, which joins the
evidence and publishes the useful result. Evidence left only in a reviewing session
cannot inform the next reviewer; publication follows the existing root/worker authority.

Do not gate reading. A claim ceremony, reservation, or persistent writer-liveness
signal ahead of research or review serializes the cheap half of the work and goes stale.
Mutation is different: immediately before a write, establish the exact candidate,
head, and mutation ownership from live evidence, using typed checks applicable to the
operation. `cargo xtask writer-admission` checks new-candidate creation; its open-PR
collision result is not evidence of another current writer during repair of an
existing candidate. For resume/reuse, consume the applicable guidance and verify the
live head and writer. Missing, stale, or contradictory writer evidence is `NOT_PROVEN`;
an established second writer is a hard stop. This is a just-in-time write check, not
durable coordination state.

If a collision is discovered only after commits from multiple contexts have reached the
branch, first establish one writer. That writer preserves the commits by merging
rather than rewriting history, subject to applicable user and repository authorization.
A second writer does not gain permission to perform recovery mutation merely because
merge is safer than force-push. Writer ownership alone grants no rebase or force-push
authorization.

## Proof ladder

```text
edit
→ exact staged structural proof
→ affected committed proof
→ candidate challenge/review
→ actual merge-group G qualification
→ protected landing and M evidence
```

Run the cheapest discriminating proof first. Never weaken a test, ratchet, support
claim, or required proof merely to obtain green status. Missing or failed instruments
produce `NOT_PROVEN`.

## Review and merge

Substantive review is directed, falsifying, and verified. It establishes, where
applicable, proof discrimination, production reachability, external/semantic truth,
claim honesty, authority/complexity, risk/rollback, and remaining uncertainty.

```text
finish-pr
→ final mutable challenge
→ root selects differentiated review programmes
→ root joins and submits cumulative review-pr
→ REVIEW_CURRENT
→ exact-H conditional landing authorization
→ verify-live-ci / native enqueue and actual G
→ protected native landing
→ merge-reconcile / M read-back
```

Green CI, mergeability, zero threads, bot approval, or author self-certification cannot
create `REVIEW_CURRENT`. Merge requires both current substantive review and current
integration evidence.

Review is semantic and cumulative. Refresh only findings/proof/dimensions materially
changed by repair, claim/authority/risk changes, or actual conflict/combined-tree
repair. Unrelated `main` movement and formatting/editorial/generated-only changes do
not force broad replay.

Generated projections are never repaired by hand: follow their owning writer and
publication policy, and validate any permitted regeneration. In particular, feature
branches do not regenerate or stage `docs/policy/NON_RUST_INVENTORY.md`, the default-branch
publication. [File policy](../FILE_POLICY.md) and
[non-Rust policy](../policy/NON_RUST_POLICY.md) define its current-tree evidence path.
Temporary workflows that execute or push on a branch's behalf are not an acceptable
repair route; the edit is made locally by the candidate writer.

## Self-authored correction and disclosure

When an in-scope defect is reversible and covered by the accepted claim and current
writer envelope, correct it immediately, rerun the
affected proof, and disclose the defect, correction, result, and remaining uncertainty.
The fact that the agent introduced the defect does not create a new approval
transaction. Ask for a decision only when the correction requires a non-derivable
product or policy choice, a separately protected external action, or material scope,
cost, privacy, security, or exposure change. Disclosure and authorization are separate
facts.

Pending candidate work remains a candidate-local wait with one exact wake event. Do not
turn unchanged polling into progress or an umbrella blocker; advance another disjoint
claim when its authority permits. An active writer collision and a higher-precedence
prohibition hold only the affected claim and preserve existing ownership and result
vocabulary. Repository guidance cannot override a higher-precedence runtime
instruction; that provenance remains an explicit uncertainty boundary.

## Hard stops

Stop only for concrete hazards: same-candidate writer collision, destructive loss,
unestablished durable identity/authority, unsafe irreversible action, structurally
invalid contract, unresolved material finding, `NOT_PROVEN` substantive review at
merge, or live GitHub protection blocking integration.

Everything else normally follows:

```text
detect
→ explain
→ route
→ repair
→ continue
```
