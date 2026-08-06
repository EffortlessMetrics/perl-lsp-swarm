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
3. this file, `.claude/skills/`, and the nearest package-local `CLAUDE.md` or
   `AGENTS.md` guidance;
4. shared method/reference docs under `docs/agents/`;
5. Claude plans, task lists, subagents, Teams state, worktrees, memory, and
   conversation.

For Claude Code, this file and `.claude/skills/` are the operational flow authority.
Shared documents explain cross-provider principles and currentness; they do not
replace a provider-native skill or make a review happen merely by being linked.

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
- `finish-pr` — publish or resume, repair feedback, substantively review, integrate,
  merge, and reconcile.

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

Use the internal `orchestrate-work` skill for proportional execution shape:

```text
tiny or tightly coupled work
→ direct root execution is often cheapest

substantive work
→ the root normally orchestrates
→ focused agents consume bounded skills or questions
→ one writer integrates each current candidate

campaign work
→ whole-flow claim lanes may run under the campaign root
→ the root retains goal interpretation and reconciliation
```

Delegation is worth its cost when it preserves root context, changes the source,
oracle, tool, environment, threat model, or review method, compresses high-output
evidence, or reduces elapsed time. The brief is the control: bound the target,
authority, mutation boundary, sufficient result, falsifiers, stop conditions, and
non-goals. An unknown conclusion may be explored when the search boundary is bounded.
Agent Teams are useful only when genuine communication changes the result; they are
not the default lifecycle.

Delegate when the evidence-to-answer compression ratio is high: CI or log triage,
corpus or repository-wide searches, dependency/API audits, external-source collection,
failure bisection, broad inventories, or an independently useful proof adversary. The
child returns bounded evidence and references; the warm root keeps decisions,
contradictions, and integration.

One coherent claim normally has one current candidate, and one writer mutates that
candidate at a time. Before creating another candidate, check only for an equivalent
current PR and explicit prerequisites. If Git or required integration evidence later
presents a real conflict, the affected lane repairs it and refreshes only affected
proof and review.

A compact whole-flow assignment is enough when repository skills carry the method:

```text
Take issue #123 through `deliver-pr`.
Use GitHub as durable state. Follow each skill's normal and material backward routes
until the claim is reconciled or a real blocker remains.
```

For focused delegation, name the skill, target, authoritative inputs, established
facts, read/write boundary, realistic falsifiers, expected evidence, and non-goals.
Do not create another identity merely because attention moved between lifecycle
passes.

Use a direct issue or PR comment when another lane genuinely needs a material fact: a
prerequisite changed, a governing ruling changed, one claim superseded another, or an
actual integration interaction was found. No additional coordination state is needed.

Detailed cross-provider method and contracts:
[`docs/agents/DEVELOPMENT_METHOD.md`](docs/agents/DEVELOPMENT_METHOD.md),
[`docs/agents/GITHUB_SURFACES.md`](docs/agents/GITHUB_SURFACES.md),
[`docs/agents/REVIEW_CURRENTNESS.md`](docs/agents/REVIEW_CURRENTNESS.md), and
[`docs/agents/SKILL_CONTRACT.md`](docs/agents/SKILL_CONTRACT.md). The operator
recovery and handoff sequence is in
[`docs/how-to/SESSION_OPERATIONS.md`](docs/how-to/SESSION_OPERATIONS.md).

## GitHub-native work

- issues hold research, corrections, current synthesis, plans, dependencies, and next
  coherent actions;
- pull requests hold one acceptance-and-rollback candidate;
- submitted reviews and inline threads hold findings, cumulative judgment, and
  evidence-backed dispositions;
- checks and rulesets hold current machine and integration evidence;
- merge closeout records what landed, what remains, and what becomes actionable next.

Use labels only for stable area, kind, risk, release, blocker, or requested-attention
classification.

Publish locally complete candidates ready by default. Draft is an explicit exception
for remote-only proof, real collaboration, early visible ownership, or a protected
integration experiment.

A clean review is valid. Never manufacture a finding or edit to prove review effort.

## Claude-native PR review

Review is an operational flow in `.claude/skills/`, not a shared-document pointer. For
a substantive PR, the normal path is:

```text
`finish-pr`
→ repair existing findings through `address-review-comments`
→ `final-challenge` while mutation remains allowed
→ `orchestrate-work` to select applicable adversarial lenses
→ main thread joins evidence and performs cumulative `review-pr`
→ REVIEW_CURRENT | CHANGES_REQUIRED | NOT_PROVEN |
  BLOCKED_BY_PREREQUISITE | SUPERSEDED_OR_CLOSE
→ only REVIEW_CURRENT enters `verify-live-ci`
→ INTEGRATION_READY | PR_IN_FLIGHT | MERGE_BLOCKED | NOT_PROVEN
→ `merge-reconcile`
```

The main thread normally considers, where applicable:

- `review-tests` for discrimination, historical-defect controls, negative/stale
  directions, schema/validator agreement, and evidence integrity;
- `review-candidate` for implementation correctness, semantic authority, production
  reachability, complexity, compatibility, risk, and rollback;
- a bounded production-path trace from a real caller or consumer;
- a competent external oracle for language, protocol, platform, dependency, or
  release truth;
- focused security, packaging, migration, persistence, or support review.

Focused Claude subagents return evidence, contradictions, falsifiers, uncertainty,
and recommended findings. They do not approve the PR. The main thread joins evidence
rather than votes, inspects load-bearing seams, publishes one useful GitHub review,
and judges whether the challenge was real or performative. One integrating writer
repairs accepted findings.

Review is not reading a diff, relaying CI green, posting a head/claim hash, or repeating
a subagent verdict. For substantive work it is directed, falsifying, and verified:

- **discrimination** — what realistic wrong implementation does the proof reject?
- **production reachability** — what live request, consumer, installer, workflow, or
  runtime path reaches the change?
- **external truth** — what competent authority establishes user-visible, language,
  protocol, platform, dependency, or release semantics?
- **claim honesty** — what does the evidence establish, and what remains unproved?
- **authority and complexity** — is this the semantic owner, and is the result free of
  duplicate authority, residue, or unnecessary API?
- **risk and rollback** — what compatibility, security, persistence, packaging,
  migration, support, or release boundary moved?

The construction context must not be the only detection surface supporting a
substantive merge. Fresh context is useful when it brings a different source, oracle,
threat model, method, environment, or attention surface; identity separation by
itself is neither necessary nor sufficient.

Every substantive PR records one cumulative substantive review result before live
integration:

```text
REVIEW_CURRENT
CHANGES_REQUIRED
NOT_PROVEN
BLOCKED_BY_PREREQUISITE
SUPERSEDED_OR_CLOSE
```

Green checks, `mergeable: true`, zero open threads, bot approval, or author
self-certification cannot independently create `REVIEW_CURRENT`.

Once review is current, live GitHub facts separately produce:

```text
INTEGRATION_READY
PR_IN_FLIGHT
MERGE_BLOCKED
NOT_PROVEN
```

A pending check leaves the substantive review current while integration is in flight.

Review is cumulative and semantic. Submitted reviews, inline findings, replies, and
evidence-backed dispositions are the durable record. Do not post `Review pass (...) at
head ... and claim ...` comments. A later commit does not invalidate review merely
because the SHA changed.

After repair:

- rerun affected proof;
- verify the affected finding and changed seam;
- revisit claim, authority, reachability, proof, risk, rollback, compatibility, or
  integration only when the repair materially changes that dimension;
- do not restart a full deep review for formatting, editorial cleanup, generated
  receipt refresh, or stronger tests unless meaning changed;
- review actual conflict or combined-tree repairs at the affected seam.

A clean review should state what was checked, which realistic wrong behavior was
challenged, what the evidence establishes, and what remains unproved.

## Proof and currentness

Keep candidate, integration, and landed evidence distinct.

- candidate behavior or material claim changes → rerun affected proof and review;
- actual merge conflict → resolve it, rerun conflict-affected proof, and review the
  repaired seam;
- explicit prerequisite change or actual merge-group/combined-tree failure → targeted
  analysis and lane-local repair;
- unrelated `main` movement with a conflict-free candidate → no rebase, update-branch,
  empty commit, full CI replay, or review churn;
- a head SHA change by itself → no review invalidation.

At merge time, the current head SHA may be used as compare-and-swap protection so the
branch cannot move between inspection and merge. That is merge safety, not review
currentness.

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
- substantive review is missing or `NOT_PROVEN` for a candidate that would merge;
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