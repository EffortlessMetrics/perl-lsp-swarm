# Claude repository operating contract

## Product direction

perl-lsp is becoming a compiler-backed Perl toolchain whose parser, compiler facts, workspace model, LSP, DAP, packaging, and editor behavior remain honest about source, freshness, confidence, fallback, and dynamic boundaries.

Optimize for real user-visible closure, semantic ownership, deterministic proof, and maintainable current-main behavior—not local component completion or workflow compliance.

## Sources of truth

Use the highest applicable current authority:

1. current `origin/main`, live GitHub issues/PRs/reviews/checks/rulesets, and actual repository behavior;
2. accepted specifications, ADRs, policies, generated contracts, and independent proof;
3. this file and package-local `CLAUDE.md` guidance;
4. Claude plans, task lists, subagents, Teams state, worktrees, memory, and conversation.

GitHub owns live transaction state. The repository owns durable product, architecture, method, and proof contracts. Claude runtime topology, task state, liveness, model choice, and temporary plans are not repository authority.

Do not use labels, dashboards, tracked active-goal pointers, task completion, teammate identity, or conversational self-report as proof that work is ready.

## Select the public flow

Use the narrowest applicable skill under `.claude/skills/`:

- `deliver-goal` — a durable multi-PR outcome or umbrella;
- `deliver-pr` — one issue, PR, branch, candidate, or coherent claim;
- `prepare-issue` — the problem, owner, scope, proof seam, or plan is unsettled;
- `prepare-proof` — intent is settled but proof is absent or weak;
- `build-candidate` — reviewed proof or a coherent candidate needs implementation, hardening, simplification, or mutable review;
- `finish-pr` — publication, GitHub feedback, formal review, live integration, merge, or reconciliation.

Enter at the earliest absent or stale useful judgment. Existing coherent work enters midstream. Do not replay completed stages merely to manufacture process evidence, and do not run a lifecycle locator between skills.

## Operating posture

**Default-complete, recovery-forward.** Normally perform every applicable research, vision, planning, proof, hardening, simplification, review, and reconciliation pass before creating the next more expensive artifact.

When an earlier pass was missed, perform the cheapest version that can still improve the current artifact and continue. Missing historical ceremony, labels, receipts, or named-agent handoffs is not a reason to discard coherent work.

Make reasonable documented engineering decisions and proceed. Return only for a genuine product or semantic decision, a concrete safety hazard, or honest `NOT_PROVEN` evidence.

## Claude orchestration

The main Claude thread is the warm accountable orchestrator unless it was explicitly spawned with a bounded brief. Naming one issue or PR does not convert the main thread into a disposable worker.

For substantive work, the main thread may use:

- direct execution;
- one whole-flow operation agent;
- ordinary subagents or context forks for independent read-heavy source, oracle, proof, or review questions;
- Agent Teams when communication between focused readers/reviewers or distinct claim lanes materially improves the result;
- separate writers on distinct claims, each with its own candidate branch/worktree, even when eventual Git integration may require repair.

One writer mutates each current candidate branch/worktree at a time. Distinct claim lanes use ordinary optimistic Git concurrency and may touch the same files, crates, or nearby semantics; each affected lane owns its actual merge conflict or combined-tree repair. Do not proactively inspect sibling implementations merely to predict overlap. The main thread owns decisions, contradiction-preserving synthesis, GitHub updates, and continuation. Persist joined durable results; keep teammate liveness, join order, retries, model routing, and temporary worktree bookkeeping runtime-local.

A compact whole-flow assignment is sufficient when repository skills carry the method:

```text
Take issue #123 through `deliver-pr`.
Use GitHub as durable state. Follow each skill's normal and material backward
routes until the claim is reconciled or a real blocker remains.
```

For focused delegation, name the skill, target, authoritative inputs, read/write boundary, and expected result. Do not create another identity merely because attention moved from research to proof or proof to implementation.

Detailed method and contracts: [`docs/agents/DEVELOPMENT_METHOD.md`](docs/agents/DEVELOPMENT_METHOD.md), [`docs/agents/GITHUB_SURFACES.md`](docs/agents/GITHUB_SURFACES.md), [`docs/agents/REVIEW_CURRENTNESS.md`](docs/agents/REVIEW_CURRENTNESS.md), and [`docs/agents/SKILL_CONTRACT.md`](docs/agents/SKILL_CONTRACT.md).

## GitHub-native work

- issues hold research, corrections, current synthesis, plan, dependencies, and next action;
- pull requests hold one coherent acceptance-and-rollback candidate;
- submitted reviews and inline threads hold formal findings and evidence-backed dispositions;
- checks and rulesets hold current machine and integration evidence;
- merge closeout records what landed, what remains, and the next coherent claim.

Use labels only for stable area, kind, risk, release, blocker, or requested-attention classification.

Publish locally complete candidates ready by default. `publish-pr` defines the proof, hardening, simplification, clean-worktree, candidate-identity, writer-collision, and claim thresholds. Draft is an explicit exception for remote-only proof, real collaboration, early visible ownership, or a protected integration experiment.

A clean review is valid. Never manufacture a finding or edit to prove review effort.

## Proof and currentness

Formal review is bound to:

```text
full candidate head SHA
+ normalized material PR claim/review-index digest
```

- candidate or material claim change → rerun affected supporting proof and specialist review, then obtain a fresh formal-review record;
- actual merge conflict → resolve, rerun affected evidence, then formally review the resulting subject;
- explicit prerequisite change or actual merge-group/combined-tree failure → targeted analysis and lane-local repair;
- unrelated `main` movement with unchanged conflict-free candidate and material claim → no rebase, update-branch, empty commit, full CI replay, or formal-review churn.

Editorial PR-body changes outside material claim, establishment/non-goal, risk/rollback, and substantive review-index sections do not require review churn.

This repository squash-merges. GitHub creates the landed squash commit; reconciliation verifies its effect on current `main`.

Never weaken a test, ratchet, support claim, or required proof merely to obtain green status. Use `NOT_PROVEN` for missing, partial, stale, contradictory, or instrument-failed evidence.

## Hard stops

Stop only for concrete preventable hazards:

- two writers would mutate the same candidate branch/worktree concurrently;
- destructive cleanup would lose unsalvaged work;
- repository, branch, candidate, or material claim identity cannot be established;
- a secret or unsafe release would be published;
- a durable contract is structurally invalid;
- substantive review findings remain unresolved;
- current GitHub branch protection, rulesets, merge queue, or required checks block merge.

Otherwise detect, explain, repair, and continue.

## Repository and Claude hygiene

- production code must not use `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, `abort`, or `dbg!` outside documented narrow exceptions;
- never use `git stash` in worktrees; use scoped restore or a WIP commit;
- stage intended paths explicitly;
- use one worktree per genuine concurrent write lane, not per lifecycle pass;
- run focused proof first, then affected package proof, then broader proof at the coherent candidate boundary;
- use package-local `CLAUDE.md` files for domain ownership and commands;
- shared `.claude/settings.json` must remain portable and minimal; personal permissions, bypass posture, model routing, experimental choices, and broad command allowlists belong in user or local settings.

Useful commands:

```bash
just doctor
just pr-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Select proof proportionately; current GitHub protection remains authoritative at merge.
