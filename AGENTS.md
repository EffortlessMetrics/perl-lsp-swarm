# Repository operating contract

## Product direction

perl-lsp is becoming a compiler-backed Perl toolchain whose parser, compiler facts, workspace model, LSP, DAP, packaging, and editor behavior remain honest about source, freshness, confidence, fallback, and dynamic boundaries.

Optimize for real user-visible closure, semantic ownership, deterministic proof, and maintainable current-main behavior—not local component completion or workflow compliance.

## Sources of truth

Use the highest applicable current authority:

1. current `origin/main`, live GitHub issues/PRs/reviews/checks/rulesets, and actual repository behavior;
2. accepted specifications, ADRs, policies, generated contracts, and independent proof;
3. this file and package-local `AGENTS.md` guidance;
4. runtime plans, subagents, worktrees, memory, and conversation.

GitHub owns live transaction state. The repository owns durable product, architecture, method, and proof contracts. Runtime agent topology, task lists, liveness, and temporary plans are not repository authority.

Do not use labels, dashboards, tracked active-goal pointers, task completion, agent identity, or conversational self-report as proof that work is ready.

## Select the public flow

Use the narrowest applicable Codex skill under `.agents/skills/`:

- `$deliver-goal` — a durable multi-PR outcome or umbrella;
- `$deliver-pr` — one issue, PR, branch, candidate, or coherent claim;
- `$prepare-issue` — the problem, owner, scope, proof seam, or plan is unsettled;
- `$prepare-proof` — intent is settled but proof is absent or weak;
- `$build-candidate` — reviewed proof or a coherent candidate needs implementation, hardening, simplification, or mutable review;
- `$finish-pr` — publication, GitHub feedback, formal review, live integration, merge, or reconciliation.

Enter at the earliest absent or stale useful judgment. Existing coherent work enters midstream. Do not replay completed stages merely to manufacture process evidence.

Follow each skill's locally named normal or material backward route. Do not run a lifecycle locator between skills.

## Operating posture

**Default-complete, recovery-forward.** Normally perform every applicable research, vision, planning, proof, hardening, simplification, review, and reconciliation pass before creating the next more expensive artifact.

If an earlier pass was missed, perform the cheapest version that can still improve the current artifact and continue. Missing historical ceremony, labels, or receipts is not a reason to discard coherent work.

Make reasonable documented engineering decisions and proceed. Return only for a genuine product/semantic decision, a concrete safety hazard, or honest `NOT_PROVEN` evidence.

## Orchestration

The root session is the warm accountable orchestrator unless it was explicitly spawned with a bounded brief. Naming one issue or PR does not convert the root into a disposable worker.

For substantive work, the orchestrator may dynamically use:

- direct root execution;
- one whole-flow operation agent;
- parallel read-heavy source, external-oracle, proof, or review agents;
- provider-native multi-agent or team coordination;
- separate writers on distinct claims, each with its own candidate branch/worktree, even when eventual Git integration may require repair.

One writer mutates each current candidate branch/worktree at a time. Distinct claim lanes use ordinary optimistic Git concurrency and may touch the same files, crates, or nearby semantics; each affected lane owns its actual merge conflict or combined-tree repair. Do not proactively inspect sibling implementations merely to predict overlap. The orchestrator owns decisions, synthesis, GitHub updates, and continuation. Persist joined durable results; keep executor liveness and topology runtime-local.

Detailed method and contracts: [`docs/agents/DEVELOPMENT_METHOD.md`](docs/agents/DEVELOPMENT_METHOD.md), [`docs/agents/GITHUB_SURFACES.md`](docs/agents/GITHUB_SURFACES.md), [`docs/agents/REVIEW_CURRENTNESS.md`](docs/agents/REVIEW_CURRENTNESS.md), and [`docs/agents/SKILL_CONTRACT.md`](docs/agents/SKILL_CONTRACT.md).

## GitHub-native work

- issues hold research, corrections, current synthesis, plan, dependencies, and next action;
- pull requests hold one coherent acceptance-and-rollback candidate;
- submitted reviews and inline threads hold formal findings and evidence-backed dispositions;
- checks and rulesets hold current machine/integration evidence;
- merge closeout records what landed, what remains, and the next coherent claim.

Use labels for stable area, kind, risk, release, blocker, or requested-attention classification only.

Publish locally complete candidates ready by default. The `publish-pr` skill defines the proof, hardening, simplification, clean-worktree, candidate-identity, writer-collision, and claim thresholds. Draft is an explicit exception for remote-only proof, real collaboration, early visible ownership, or an integration experiment.

A clean review is valid. Never manufacture a finding or edit to prove review effort.

## Proof and currentness

Formal review is bound to the complete review subject:

```text
full candidate head SHA
+ normalized material PR claim/review-index digest
```

- candidate or material claim change → rerun affected supporting proof/specialist review, then obtain a fresh formal-review record for the new review subject;
- actual merge conflict → resolve, rerun affected supporting evidence, then obtain a fresh formal review;
- explicit prerequisite change or actual merge-group/combined-tree failure → targeted analysis and lane-local repair;
- unrelated `main` movement with an unchanged conflict-free candidate and unchanged material claim → no rebase, update-branch, empty commit, full CI replay, or formal-review churn.

Editorial PR-body changes outside the material claim, establishment/non-goal, risk/rollback, and substantive review-index sections do not require review churn.

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

## Repository hygiene

- production code must not use `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, `abort`, or `dbg!` outside documented narrow exceptions;
- never use `git stash` in worktrees; use scoped restore or a WIP commit;
- stage intended paths explicitly;
- use one worktree per genuine concurrent write lane, not per lifecycle pass;
- run focused proof first, then affected package proof, then broader proof at the coherent candidate boundary;
- use current package-local `AGENTS.md` files for domain ownership and commands.

Useful commands:

```bash
just doctor
just pr-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Select proof proportionately; current GitHub protection remains authoritative at merge.
