---
tags: [orchestration, ci, substrate, doctrine, github-rate-limit, agent-atomicity, epistemics, session-retrospective, compiler-foundation]
repos: [perl-lsp-swarm]
related: ["#2076", "#2559", "#2566", "#2567", "#2571", "#2569", "#2570"]
portable: true
article_asset: true
search_terms: [
  "Perl LSP Rust Small Result", "ripr+ New Gap Gate", "Codecov Patch 95",
  "Perl LSP Rust Small on CX43", "em-ci-rust", "self-hosted runner flap",
  "validate-title issue reference", "patch coverage merge throttle",
  "secondary rate limit", "content-creation abuse guard", "GraphQL primary pool watcher",
  "REST vs GraphQL content creation", "agent atomicity self-post",
  "session cwd leak worktree", "verify target repo before launch", "sync before audit",
  "model-tier haiku sonnet opus", "vertical packet provider promotion", "merged verified path win condition",
  "clean-parse not verified truth", "reversibility draft PR worktree idempotent", "map lags territory generated status",
  "PLSP-PROP-0002", "PLSP-ADR-0005", "PLSP-SPEC-0031", "epic 2076", "roadmap 2559"
]
---

# 2026-06-21 — Compiler-foundation + swarm session learnings

**Session**: Compiler-foundation epic landing + swarm operation, multi-agent
**Workstreams**: CI/merge operational calibration, GitHub-as-substrate throughput, orchestration doctrine, epistemic alignment of swarm with product
**Tracking**: epic [#2076](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2076), canonical roadmap [#2559](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2559)

---

This is a session retrospective in the [`docs/forensics/`](README.md) tradition (see
`2026-05-11-autonomous-session-learnings.md`). It captures the **durable operational and
doctrinal learnings** that are *not* already encoded in the merged compiler-foundation
artifacts. Each section: takeaway, what was observed, what to do differently, where it
connects to existing doctrine.

The compiler-specific design and contracts landed this session are **not restated here** —
they are the merged foundation. See:

- Epic [#2076](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2076) — compiler-foundation epic
- Canonical roadmap [#2559](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2559)
- Contracts [#2566](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2566) — PLSP-PROP-0002, PLSP-ADR-0005
- Specs [#2567](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2567) — PLSP-SPEC-0031..0035
- Keystone [#2571](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2571)
- Evidence rails [#2569](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2569) / [#2570](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2570)

This doc references that foundation; it does not duplicate it.

---

## A. CI / merge operational facts (verified this session)

### A.1 The required-check set on `main` is exactly three — judge by aggregate, not by one noisy lane

**Takeaway**: The only checks that gate a merge to `main` are `Perl LSP Rust Small Result`,
`ripr+ New Gap Gate`, and `Codecov / Patch 95` (plus live `mergeable`). The raw
`Perl LSP Rust Small on CX43` lane is **non-blocking infrastructure**, not a quality gate.

**Observed**: The CX43 self-hosted runner lane flaps — it depends on an `em-ci-rust`
image that is sometimes missing, and on a small-disk cold-boot that intermittently fails.
When CX43 flaps, the GitHub-hosted failover lane passes and rolls up into the required
`Perl LSP Rust Small Result`. PRs **#2067, #2069, #2070** merged this session with the raw
CX43 lane showing red, because the required aggregate was green. A stale or noisy
`statusCheckRollup` entry for CX43 is not a merge blocker.

**What to do differently**:
- Gate decisions on the **required-check set + live `mergeable`**, never on the raw CX43 lane.
- When reading PR state, take the **latest run per check name** and read the required
  aggregate — do not act on a stale or superseded `statusCheckRollup` entry. (This is the
  live-signal-first discipline of [LIVE_SIGNALS_VS_LABELS.md](../reference/LIVE_SIGNALS_VS_LABELS.md):
  query live ground truth, filter stale entries, trust the aggregate.)
- A red CX43 lane next to a green required `Result` is **expected infra flap**, not a regression.

**Connects to**: [LIVE_SIGNALS_VS_LABELS.md](../reference/LIVE_SIGNALS_VS_LABELS.md) (latest-per-check
filtering; live aggregate is authoritative), [orchestrator-substrate-model](../concepts/orchestrator-substrate-model.md)
(which checks are required vs advisory is a substrate fact the orchestrator must model correctly).

### A.2 `validate-title` requires an issue reference `#N`, not a conventional-commit type

**Takeaway**: The `validate-title` gate enforces the presence of an issue reference `#N` in
the PR title. It does **not** enforce a conventional-commit type prefix.

**Observed**: PR titles that read as well-formed conventional commits but omitted `#N` failed
`validate-title`; titles carrying `(#N)` passed regardless of type-prefix shape. This is a
recurring agent-generated-PR failure class (see also
[2026-06-validate-title-issue-ref-gap.md](../learnings/2026-06-validate-title-issue-ref-gap.md)).

**What to do differently**: Every agent that opens a PR must put `#N` in the title before
opening — a pre-open guard, not a post-hoc CI retry. The issue reference is the hard contract;
the type prefix is cosmetic.

### A.3 The real merge throttle is patch coverage — the answer is tests, not infra

**Takeaway**: `Codecov / Patch 95` (95% patch coverage on **new** code) is the binding
constraint on most merges this session. When a PR is stuck, the cause is usually under-tested
new lines, not a broken gate.

**Observed**: PRs that were locally correct and passed the Rust result lane still sat red on
`Codecov / Patch 95` because new code paths lacked tests. The instinct to "fix the gate" is
wrong here — the gate is correctly reporting a real coverage gap.

**What to do differently**: When `Codecov / Patch 95` is red, route to **add tests for the
new lines**, not to infra/gate changes. Distinguish this from a genuinely broken gate (cf.
[2026-06-rerunning-broken-gates.md](../learnings/2026-06-rerunning-broken-gates.md)): a gate
that fails on verified-correct content is a bug; a gate that fails on genuinely untested new
code is doing its job.

**Connects to**: [2026-06-merge-funnel-throughput-constraint.md](../learnings/2026-06-merge-funnel-throughput-constraint.md)
— the merge funnel is the binding constraint, but the sub-diagnosis matters: a true coverage
gap is a *build/test* problem, not a *substrate* problem.

---

## B. GitHub is the collaboration substrate — and the throughput bottleneck (not compute)

**Takeaway**: At swarm scale the binding constraint is **GitHub's write capacity**, not model
compute or code difficulty. The secondary *content-creation* abuse guard throttles issue and
comment creation, and it cannot be dodged by switching API surface.

**Observed**:
- The secondary content-creation abuse guard blocks issue/comment creation on **both REST and
  GraphQL** — switching API surface does not evade it. The only correct responses are **back off
  and stagger**.
- The GraphQL **primary** rate-limit pool is **watcher-dominated** (subscription/notification
  traffic eats the pool), so routing content *creation* through GraphQL competes with watcher
  load and is the wrong surface.
- Net: route **content creation through REST**, reserve GraphQL for reads where its query shape
  wins, and **stagger** any workflow that writes to GitHub.

**What to do differently**:
- Route content creation (issues, comments) through **REST** (`gh api repos/.../issues --method POST`),
  not GraphQL (`gh issue create`).
- **Stagger** gh-writing workflows — do not fire N agents that each post simultaneously.
- Make agents **atomic and self-posting** (see C.1): one agent run produces *and posts* its own
  artifact, so writes are naturally spread across agent runtimes rather than bunched at a
  fan-in moment.
- On a `403` secondary-rate-limit: **stop and back off**, do not retry-spam or switch API to
  dodge it.

**Connects to**: [orchestrator-substrate-model](../concepts/orchestrator-substrate-model.md)
— "throughput is gated by the orchestrator's model of the substrate, not by agent quality."
GitHub's write-rate semantics are a substrate fact; modeling them wrong makes the fleet thrash
against an invisible ceiling. See also
[2026-06-merge-funnel-throughput-constraint.md](../learnings/2026-06-merge-funnel-throughput-constraint.md)
(the bottleneck is the funnel/substrate, not discovery or build).

---

## C. Orchestration / agent doctrine

### C.1 Agents must be atomic and self-post their own artifact in the same run

**Takeaway**: An agent must **produce and publish** its artifact within a single run. Do not
split "do the work" from "post the result" across a later resume.

**Observed**: When an agent does work but defers posting to a hoped-for later resume, the
artifact frequently never lands (the resume doesn't happen, or context is lost). The reliable
unit is: research → produce → post, all in one run.

**What to do differently**: Design every agent as atomic: its run is not complete until its
output is committed/posted to the system of record. This also smooths GitHub write load (see B).

### C.2 Agents inherit the session cwd and leak scratch unless confined to worktrees

**Takeaway**: A spawned agent inherits the session's working directory. Without a dedicated
worktree, it writes scratch files into whatever repo the session happens to be in — including
the *wrong* repo.

**Observed**: Agents not confined to an isolated worktree leaked scratch artifacts into the
session cwd. The discipline `Agent(isolation: "worktree", ...)` exists precisely to bound this.

**What to do differently**: Run code-touching and file-creating agents in **worktree
isolation**. Confine all scratch to the worktree. Never let an agent create files outside its
designated output.

**Connects to**: `docs/reference/WORKTREE_PROTOCOL.md`, the worktree-stash prohibition, and the
worktree-isolation routing pattern.

### C.3 Verify the target repo before launching

**Takeaway**: Confirm the agent is pointed at the **correct repository** before spawning. A
mis-targeted agent does confident, wasted (or harmful) work in the wrong place.

**What to do differently**: As a preflight, assert the remote/origin of the target worktree
matches the intended repo. This is cheap and prevents an entire class of cross-repo
contamination.

### C.4 Sync before auditing — a stale base produces confident wrongness

**Takeaway**: Always `git fetch origin` and verify against current `origin/main` before making
any forward claim about code state. A stale checkout yields confident, wrong conclusions.

**Observed**: This is the same failure class documented in the 2026-05-11 retrospective
(stale-checkout misfiled 8 issues). It recurs whenever an audit reads a behind-by-N checkout.

**What to do differently**: Before any audit, issue-filing, or "X currently does Y" claim,
fetch and read from `origin/main`. Classifying question: *am I about to claim something about
code state that another agent or human will act on?* If yes, sync first.

**Connects to**: [verify-the-instrument](../concepts/verify-the-instrument.md),
[2026-06-agent-claims-vs-ground-truth.md](../learnings/2026-06-agent-claims-vs-ground-truth.md)
(agent claims are instrument readings; verify against ground truth before routing).

### C.5 Model-tier deliberately

**Takeaway**: Match model tier to task shape — **haiku** for research/triage/filing, **sonnet**
for design/build, **opus** for the hardest design and synthesis. Spending sonnet on discovery,
or haiku on hard design, both waste the run.

**What to do differently**: Default routing — discovery/scout/accuracy/triage → haiku;
plan-review/build/green-refactor → sonnet; the hardest architecture/synthesis → opus.

**Connects to**: [model-conformance](../concepts/model-conformance.md), the model column in the
CLAUDE.md pipeline table.

### C.6 Build by vertical packets that each CLOSE on a provider promotion

**Takeaway**: Structure build work as **vertical packets** — each one ends with a real,
user-visible **provider promotion** (a capability moved forward in `features.toml` and proven
by tests). Do **not** build "coverage shells" — horizontal scaffolding that raises a metric
without closing a vertical.

**What to do differently**: Define each build unit by the promotion it delivers, not by the
files it touches. A packet is done when a provider's capability is genuinely advanced and proven,
not when a coverage number ticks up.

**Connects to**: [slow-stochastic-compiler](../concepts/slow-stochastic-compiler.md) — the
compiler's output is *emitted* (merged, working code), not partial passes; a vertical packet is
one complete emit.

### C.7 The win condition is a MERGED, verified path — not artifact count

**Takeaway**: Success is a **merged, verified** change, not a pile of issues/PRs/docs. Artifact
count is a vanity metric; a merged verified path is the only thing that moves the product.

**What to do differently**: Measure the swarm by merged-verified throughput, not by open
artifacts. An agent that files 10 issues that never land has produced nothing durable.

**Connects to**: [ORCHESTRATION_DOCTRINE.md](../reference/ORCHESTRATION_DOCTRINE.md) ("the
product isn't code, it's decisions + proof" — and proof requires the change to land and stay
green).

---

## D. The epistemic meta-point: the orchestration's failure modes mirror the product's own problem

**Takeaway**: The swarm's failure modes are the **same shape** as the problem the product (a
Perl analyzer/LSP) exists to solve. Recognizing the isomorphism makes both halves easier to
reason about.

The recurring pattern, stated four ways:

1. **Clean-parse ≠ verified truth.** A parser producing a clean AST has not proven the program
   is correct; an agent producing a confident summary has not proven the action landed. Both
   are *instrument readings*, not *ground truth*. (cf.
   [verify-the-instrument](../concepts/verify-the-instrument.md),
   [2026-06-agent-claims-vs-ground-truth.md](../learnings/2026-06-agent-claims-vs-ground-truth.md).)

2. **Never act on stale signals.** A stale checkout, a stale `statusCheckRollup` entry, a stale
   label — each is a measurement that no longer describes reality. Measure *before* acting, and
   prefer the live signal where one exists (A.1, C.4).

3. **Measure before acting.** The expensive mistakes this session were all *acting on an
   assumed state* (assumed CX43 was a gate; assumed an API switch dodges the rate limit;
   assumed the checkout was fresh). The cheap fix is always a measurement first.

4. **Reversibility beats first-try correctness at swarm scale.** Draft PRs, isolated worktrees,
   and idempotent re-runs make every step *cheaply undoable*. At scale, a stochastic pipeline
   that can re-run a wrong pass cheaply beats one that must be right the first time. (cf.
   [slow-stochastic-compiler](../concepts/slow-stochastic-compiler.md): each pass may be wrong;
   the operator's job is to make wrongness cheap to catch and redo, not to eliminate it.)

And the governing consequence:

> **The map lags the territory.** Issues and roadmaps (the map) drift behind the code (the
> territory). Therefore the **code is the durable truth**, and status is **generated, not
> hand-written**.

This is why this very doc *references* the merged foundation (#2076, #2559, #2566, #2567,
#2571, #2569/#2570) rather than restating its contents: the merged artifacts are the
territory; a hand-copied summary here would be a map that immediately starts to drift. When in
doubt, read the code and the generated status, not a prose snapshot.

**Connects to**:
- [slow-stochastic-compiler](../concepts/slow-stochastic-compiler.md) — the orchestration *is*
  a compiler: stochastic passes, a verifier (CI/coverage), an emit (merge), and an operator who
  makes passes cheap to re-run. Its soundness problem is the product's soundness problem.
- [doctrine-is-a-hypothesis](../concepts/doctrine-is-a-hypothesis.md) — doctrine (the map) is a
  hypothesis tested against the substrate (the territory); when they disagree, the substrate
  wins and the doctrine is updated.
- [ORCHESTRATION_DOCTRINE.md](../reference/ORCHESTRATION_DOCTRINE.md) and
  [LIVE_SIGNALS_VS_LABELS.md](../reference/LIVE_SIGNALS_VS_LABELS.md) — live truth where it
  exists; status is computed, not hand-edited.

---

## Where this connects

- Merged compiler foundation (not restated): [#2076](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2076),
  [#2559](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2559),
  [#2566](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2566) (PLSP-PROP-0002, PLSP-ADR-0005),
  [#2567](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2567) (PLSP-SPEC-0031..0035),
  [#2571](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2571),
  [#2569](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2569) / [#2570](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2570).
- Doctrine: [ORCHESTRATION_DOCTRINE.md](../reference/ORCHESTRATION_DOCTRINE.md),
  [LIVE_SIGNALS_VS_LABELS.md](../reference/LIVE_SIGNALS_VS_LABELS.md),
  [PIPELINE_GATES.md](../reference/PIPELINE_GATES.md).
- Concepts: [slow-stochastic-compiler](../concepts/slow-stochastic-compiler.md),
  [orchestrator-substrate-model](../concepts/orchestrator-substrate-model.md),
  [verify-the-instrument](../concepts/verify-the-instrument.md),
  [doctrine-is-a-hypothesis](../concepts/doctrine-is-a-hypothesis.md),
  [model-conformance](../concepts/model-conformance.md).
- Adjacent incident learnings: [2026-06-merge-funnel-throughput-constraint.md](../learnings/2026-06-merge-funnel-throughput-constraint.md),
  [2026-06-agent-claims-vs-ground-truth.md](../learnings/2026-06-agent-claims-vs-ground-truth.md),
  [2026-06-validate-title-issue-ref-gap.md](../learnings/2026-06-validate-title-issue-ref-gap.md),
  [2026-06-rerunning-broken-gates.md](../learnings/2026-06-rerunning-broken-gates.md).
