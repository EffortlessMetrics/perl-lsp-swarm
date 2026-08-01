# Claude repository operating contract

## Product direction

perl-lsp is becoming a compiler-backed Perl toolchain whose parser, compiler facts,
workspace model, LSP, DAP, packaging, and editor behavior remain honest about source,
freshness, confidence, fallback, and dynamic boundaries.

Optimize for real user-visible closure, semantic ownership, deterministic proof, and
maintainable current-main behavior—not local component completion or workflow
compliance.

## Sources of truth

Use the highest applicable current authority:

1. current `origin/main`, live GitHub issues, PRs, reviews, checks, rulesets, and
   actual repository behavior;
2. accepted specifications, ADRs, policies, generated contracts, and independent
   proof;
3. this file and the nearest package-local `CLAUDE.md` or `AGENTS.md` guidance;
4. Claude plans, task lists, subagents, Teams state, worktrees, memory, and
   conversation.

GitHub owns live transaction state. The repository owns durable product,
architecture, method, and proof contracts. Claude runtime topology, task state,
liveness, model choice, and temporary plans are not repository authority.

Do not use labels, dashboards, tracked active-goal pointers, task completion,
teammate identity, or conversational self-report as proof that work is ready.

## Select the public flow

Use the narrowest applicable skill under `.claude/skills/`:

- `deliver-goal` — advance a durable multi-PR outcome or umbrella;
- `deliver-pr` — carry one issue, PR, branch, candidate, or coherent claim;
- `prepare-issue` — settle the problem, owner, scope, proof seam, or plan;
- `prepare-proof` — turn settled intent into discriminating proof;
- `build-candidate` — implement, harden tests, simplify, or challenge a candidate;
- `finish-pr` — publish or resume, repair feedback, review, integrate, merge, and
  reconcile.

Enter at the earliest absent or stale useful judgment. Existing coherent work enters
midstream. Do not replay completed stages merely to manufacture process evidence, and
do not run a lifecycle locator between skills.

Follow each skill's named normal route. Route backward only when material evidence
changes behavior, ownership, scope, architecture, proof, risk, rollback, or support
meaning.

## Operating posture

**Default-complete, recovery-forward.** Normally perform every applicable research,
vision, planning, proof, hardening, simplification, review, and reconciliation pass
before creating the next more expensive artifact.

When an earlier pass was missed, perform the cheapest version that can still improve
the current artifact and continue. Missing historical ceremony, labels, receipts, or
named-agent handoffs is not a reason to discard coherent work.

Make reasonable documented engineering decisions and proceed. Return only for a
genuine product or semantic decision, a concrete safety hazard, or honest
`NOT_PROVEN` evidence.

## Claude execution and delegation

The main Claude thread is the warm accountable owner unless it was explicitly spawned
with a bounded brief. Naming one issue or PR does not convert the main thread into a
disposable worker.

Orchestrating subagents is a primary working pattern here, not a fallback. The root's
usual job is to route, brief, and judge — not to perform every pass personally.
Accountability stays with the root; execution does not have to. Route each piece of
work to the cheapest context that can produce trustworthy evidence.

Delegate by default:

- **passes that need different premises, sources, or method** — an external oracle
  lookup, a threat model the lane has not applied, a reachability trace from the live
  caller inward. Delegate these for the change in approach, not to obtain a second
  opinion: an agent handed the same premises and the same evidence returns the same
  conclusion, and that is not corroboration;
- **single-use bulk evidence** — CI and log triage, corpus sweeps, dependency audits,
  failure bisection, broad code search: work whose raw output is read once, yields a
  small answer, and is never referenced again. Doing it in the root is the costlier
  choice, not the cheaper one: the answer is consumed immediately but the output
  occupies root context for the rest of the campaign and degrades every judgment
  after it. A delegate pays the cold-start cost once; the root pays the pollution
  cost permanently. The larger the task and the more disposable its output, the more
  clearly it belongs elsewhere;
- **independent lanes** that can genuinely proceed in parallel, and concurrent
  writers that need worktree isolation.

Execute directly when the work is judgment rather than evidence — synthesis, claim
selection, goal interpretation, deciding what a finding means — where quality depends
on holding the whole picture, and when a brief would cost more than the edit.

Delegation pays or wastes according to the brief, not the agent count. Both ways it
fails are brief defects, and both are avoidable:

- a delegate starts cold and will re-derive whatever it is not given. That cost is
  mostly controllable — hand over the facts already established, the exact files and
  identifiers, and the current state, instead of making it rediscover them. Fanning
  one under-specified question across several agents pays that rediscovery cost
  repeatedly and returns the same answer each time;
- a delegate aimed at the wrong question answers *that* question, confidently and in
  good format. A plausible wrong answer is harder to recover from than no answer,
  because it reads as evidence.

So brief well:

- name the exact question, the authoritative inputs, the read/write boundary, and
  what a sufficient answer contains;
- state what is already known and settled, so the delegate spends its context on the
  open part;
- say what would falsify the expected answer, and require gaps and `NOT_PROVEN`
  evidence to come back named rather than smoothed over;
- if the question cannot be stated that precisely, it is not understood well enough
  to delegate yet — establish it directly, then delegate the bounded remainder;
- read what returns as a claim to be checked, not a result to adopt.

Claim a lane by commenting on its controlling issue, and read that issue before
dispatching a writer for it. The claim lives on the issue — that single read is the
whole check. Do not survey open PRs, branches, or sibling worktrees to infer who is
working on what: that is expensive, unreliable, and not the coordination mechanism.

Do not spawn an identity per lifecycle pass. Agent Teams are for agents that must
genuinely communicate; they are not the default lifecycle.

One coherent claim normally has one current candidate, and one writer mutates that
candidate at a time. Before creating another candidate, check only for an equivalent
current PR and explicit prerequisites. Otherwise focus on the selected claim. If Git
or required integration evidence later presents a real conflict, the affected lane
repairs it and refreshes only the affected proof and review.

A compact whole-flow assignment is enough when the repository skills carry the
method:

```text
Take issue #123 through `deliver-pr`.
Use GitHub as durable state. Follow each skill's normal and material backward routes
until the claim is reconciled or a real blocker remains.
```

For focused delegation, name the skill, target, authoritative inputs, read/write
boundary, and expected result. Do not create another identity merely because
attention moved from research to proof or proof to implementation.

Use a direct issue or PR comment when another lane genuinely needs a material fact:
a prerequisite changed, a governing ruling changed, one claim superseded another, or
an actual integration interaction was found. No additional coordination state is
needed.

Detailed method and contracts:
[`docs/agents/DEVELOPMENT_METHOD.md`](docs/agents/DEVELOPMENT_METHOD.md),
[`docs/agents/GITHUB_SURFACES.md`](docs/agents/GITHUB_SURFACES.md),
[`docs/agents/REVIEW_CURRENTNESS.md`](docs/agents/REVIEW_CURRENTNESS.md), and
[`docs/agents/SKILL_CONTRACT.md`](docs/agents/SKILL_CONTRACT.md).

## GitHub-native work

- issues hold research, corrections, current synthesis, plans, dependencies, and next
  coherent actions;
- pull requests hold one acceptance-and-rollback candidate;
- submitted reviews and inline threads hold formal findings and evidence-backed
  dispositions;
- checks and rulesets hold current machine and integration evidence;
- merge closeout records what landed, what remains, and what becomes actionable next.

Use labels only for stable area, kind, risk, release, blocker, or requested-attention
classification.

Publish locally complete candidates ready by default. Draft is an explicit exception
for remote-only proof, real collaboration, early visible ownership, or a protected
integration experiment.

## Review

Review is not reading a diff. Reading shows what changed; review establishes whether
the change is correct, reachable, and honestly claimed. Every substantive candidate
must answer:

- **discrimination** — would a realistic wrong implementation fail this proof? A test
  that also passes against the defect proves nothing. Name the wrong implementation
  the proof excludes;
- **production reachability** — is the changed path reached from a real request or
  live caller, not merely compiled and unit-tested? Component-proved is not
  system-proved;
- **external truth** — for user-visible semantics (diagnostics, hover and completion
  text, builtin signatures, protocol behavior, version gating), does the claim hold
  against the external oracle: perldoc, the LSP/DAP specification, the real crate
  API? Green checks prove internal consistency, never external truth;
- **claim honesty** — does the PR claim exceed its evidence? What did it not prove?
- **authority and complexity** — correct semantic owner, no duplicate authority, no
  scaffolding or compatibility shim that outlives its use.

What makes a review real is that it is **directed, falsifying, and verified** — not
who performs it:

- **directed** — work the questions above explicitly and in order. An overall
  impression of a diff is not a review, and reading one is not evidence;
- **falsifying** — try to break the claim. A pass that sets out to confirm the change
  will succeed whether or not the change is correct;
- **verified** — settle each answer by running something or reading an authoritative
  source. Inspection reliably confirms whatever the reader already expects, so
  plausibility is never the oracle.

A review performed by the same context that wrote the candidate can be a good review
when it meets that bar. Independence is genuinely valuable and worth reaching for —
a separate context does not carry the intent behind the change, will not re-confirm
a reading it already made, and attends where a saturated context skims. It is simply
neither necessary nor sufficient: an agent handed the same premises and the same
narrow evidence reproduces the same blind spot, and a fresh context is not a fresh
judgment.

So prefer a separate reviewer where it is cheap, and require one where the stakes or
the blast radius justify it — but buy it for what it changes: different premises,
different sources, a different method. Independence that changes only the identity
performing the pass adds cost, not evidence.

A subagent's verdict is evidence, not review. `mergeable: true` is a GitHub fact and
green checks are machine evidence; neither is a semantic judgment. The accountable
owner judges whether the challenge was real or performative, and that judgment does
not delegate.

A clean review is valid. Never manufacture a finding or edit to prove review effort.

Review is mutable before publication — `review-tests`, `review-candidate`,
`simplify-candidate`, `final-challenge` — and fixed after it, where `review-pr` binds
to an exact candidate and material claim.

Merge requires all three: a directed, falsifying, verified pass actually happened
against this candidate; its substantive findings are resolved with evidence; and the
accountable owner judged that challenge sufficient rather than performative. Never
merge on green checks plus a relayed verdict.

## Proof and currentness

Formal review binds to:

```text
full candidate head SHA
+ normalized material PR claim and review-index digest
```

- candidate or material claim change → rerun affected proof and review;
- actual merge conflict → resolve it, rerun conflict-affected proof, and review the
  resulting candidate;
- explicit prerequisite change or actual merge-group or combined-tree failure →
  perform targeted analysis and lane-local repair;
- unrelated `main` movement with an unchanged conflict-free candidate and material
  claim → no rebase, update-branch, empty commit, full CI replay, or review churn.

This repository squash-merges. GitHub creates the landed squash commit;
reconciliation verifies its effect on current `main`.

Never weaken a test, ratchet, support claim, or required proof merely to obtain green
status. Use `NOT_PROVEN` for missing, partial, stale, contradictory, or
instrument-failed evidence.

## Hard stops

Stop only for concrete preventable hazards:

- two writers would mutate the same candidate branch or worktree concurrently;
- destructive cleanup would lose unsalvaged work;
- repository, branch, candidate, or material claim identity cannot be established;
- a secret or unsafe release would be published;
- a durable contract is structurally invalid;
- substantive review findings remain unresolved;
- current GitHub branch protection, rulesets, merge queue, or required checks block
  merge.

Otherwise detect, explain, repair, and continue.

## Repository and Claude hygiene

- read the nearest package-local owner guidance before modifying an owning crate;
- production code must not use `unwrap`, `expect`, `panic!`, `todo!`,
  `unimplemented!`, `abort`, or `dbg!` outside documented narrow exceptions;
- never use `git stash` in worktrees; use scoped restore or a WIP commit;
- stage intended paths explicitly;
- use one worktree per genuine concurrent write claim, not per lifecycle pass;
- run focused proof first, then affected package proof, then broader proof only when
  the candidate's risk or merge gate selects it;
- do not run repository-wide Clippy or tests after every edit;
- shared `.claude/settings.json` must remain portable and minimal; personal
  permissions, bypass posture, model routing, experimental choices, and broad command
  allowlists belong in user or local settings.

Useful current commands:

```bash
just doctor
cargo fmt -p <package> -- --check
cargo clippy -p <package> --all-targets --locked -- -D warnings
cargo test -p <package> --all-targets --locked
just pr-fast
```

Choose the smallest command that can falsify the claim. Current GitHub protection
remains authoritative at merge.
