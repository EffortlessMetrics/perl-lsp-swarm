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
oracle, tool, environment, or review method, compresses high-output evidence, or
reduces elapsed time. The brief is the control: bound the target, authority,
mutation boundary, sufficient result, falsifiers, stop conditions, and non-goals.
An unknown conclusion may be explored when the search boundary is bounded. Agent
Teams are useful only when genuine communication changes the result; they are not
the default lifecycle.

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

A clean review is valid. Never manufacture a finding or edit to prove review effort.

## Review

Review is not reading a diff, relaying CI green, or repeating a subagent verdict.
For substantive work it is a directed, falsifying, and verified judgment:

- **discrimination** — what realistic wrong implementation does the proof reject?
- **production reachability** — what live request, consumer, or protocol path reaches the change?
- **external truth** — what competent authority establishes user-visible or protocol semantics?
- **claim honesty** — what does the evidence establish, and what remains unproved?
- **authority and complexity** — is this the semantic owner, and is the result free of duplicate authority, residue, or unnecessary API?
- **risk and rollback** — what compatibility, security, packaging, migration, or support boundary moved?

The construction context must not be the only detection surface supporting merge.
Fresh context is useful when it brings a different source, oracle, threat model,
method, or attention surface; identity separation by itself is neither necessary
nor sufficient. For a substantive fixed or refuted disposition, enforced review
convergence still requires a passing verification receipt from a verifier outside
the writer set. A clean review is a valid result when that applicable protocol is
satisfied.

The review sequence is:

```text
writer self-check
→ proof challenge
→ mutable candidate challenge
→ repair and simplification
→ final mutable challenge
→ fixed-candidate formal review
→ accountable integration judgment
```

“Mutable” describes the challenge method, not a pre-publication time window.
`final-challenge` may run after publication or after accepted repair. `review-pr`
is fixed-candidate formal judgment; CI and reviewer/subagent conclusions are
evidence for the accountable owner, not approval by themselves.

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
