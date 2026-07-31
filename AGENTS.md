# Repository operating contract

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
3. this file and the nearest package-local `AGENTS.md` guidance;
4. runtime plans, subagents, worktrees, memory, and conversation.

GitHub owns live transaction state. The repository owns durable product,
architecture, method, and proof contracts. Runtime agent topology, task lists,
liveness, and temporary plans are not repository authority.

Do not use labels, dashboards, tracked active-goal pointers, task completion, agent
identity, or conversational self-report as proof that work is ready.

## Select the public flow

Use the narrowest applicable Codex skill under `.agents/skills/`:

- `$deliver-goal` — advance a durable multi-PR outcome or umbrella;
- `$deliver-pr` — carry one issue, PR, branch, candidate, or coherent claim;
- `$prepare-issue` — settle the problem, owner, scope, proof seam, or plan;
- `$prepare-proof` — turn settled intent into discriminating proof;
- `$build-candidate` — implement, harden tests, simplify, or challenge a candidate;
- `$finish-pr` — publish or resume, repair feedback, review, integrate, merge, and
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
the current artifact and continue. Missing historical ceremony, labels, or receipts
is not a reason to discard coherent work.

Make reasonable documented engineering decisions and proceed. Return only for a
genuine product or semantic decision, a concrete safety hazard, or honest
`NOT_PROVEN` evidence.

## Execution and delegation

The root session is the warm accountable owner unless it was explicitly spawned with
a bounded brief. Naming one issue or PR does not convert the root into a disposable
worker.

Direct execution is normal. Use focused subagents, context forks, external oracles,
or whole-flow workers when another context, tool surface, or review method materially
changes the evidence or reduces elapsed work.

One coherent claim normally has one current candidate, and one writer mutates that
candidate at a time. Distinct claims may proceed in parallel even when they edit the
same files, crates, or nearby semantic seams. Same-file or same-crate overlap is not
ownership and is not a reason to serialize work.

Before creating another candidate, check only for an equivalent current PR and
explicit prerequisites. Otherwise focus on the selected claim. If Git or required
integration evidence later presents a real conflict, the affected lane repairs it
and refreshes only the affected proof and review.

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

## Repository hygiene and local proof

- read the nearest package-local owner guidance before modifying an owning crate;
- production code must not use `unwrap`, `expect`, `panic!`, `todo!`,
  `unimplemented!`, `abort`, or `dbg!` outside documented narrow exceptions;
- never use `git stash` in worktrees; use scoped restore or a WIP commit;
- stage intended paths explicitly;
- use one worktree per genuine concurrent write claim, not per lifecycle pass;
- run focused proof first, then affected package proof, then broader proof only when
  the candidate's risk or merge gate selects it;
- do not run repository-wide Clippy or tests after every edit.

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
