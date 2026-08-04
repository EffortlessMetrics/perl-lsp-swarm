# Provider-native Claude operating model

> The filename is retained for compatibility with existing links. The active Claude front door is [`CLAUDE.md`](../../CLAUDE.md), and the shared method lives under [`docs/agents/`](../agents/).

Claude operates the repository through live GitHub state and focused provider-native skills. It does not consume a tracked current-program manifest, fixed role conveyor, lifecycle-label chain, review-receipt lifecycle, or durable executor topology.

## Truth boundaries

Use the highest applicable current authority:

1. current GitHub default branch or a verified local `origin/main`, plus live issues, PRs, reviews, threads, checks, rulesets, and actual repository behavior;
2. accepted specifications, ADRs, policies, generated contracts, and independent proof;
3. root and package-local `CLAUDE.md` guidance plus provider-native skills;
4. Claude plans, task lists, subagents, Teams state, worktrees, memory, and conversation.

GitHub owns live transaction state. The repository owns durable product, architecture, method, and proof contracts. Claude runtime topology and liveness remain ephemeral.

The retired `.perl-lsp/goals/` manifests are historical artifacts recoverable through Git history. They no longer select work or outrank current GitHub state.

## Three useful scopes

- **durable outcome** — an umbrella issue, release objective, compiler/LSP campaign, or accepted multi-PR end state;
- **coherent claim** — one acceptance-and-rollback result carried by one current candidate and PR;
- **runtime assignment** — the current root or focused child task, which may change without altering durable state.

A root Claude session may carry a durable outcome through several claims. GitHub and repository artifacts preserve the cross-session graph.

## Public flows

```text
deliver-goal    durable multi-PR outcome
deliver-pr      one coherent claim
prepare-issue   unsettled problem, owner, scope, proof seam, or plan
prepare-proof   settled intent with absent or weak proof
build-candidate implementation, hardening, simplification, mutable review
finish-pr       publication, feedback, review, integration, merge, closeout
```

Enter existing work at the earliest absent or stale useful judgment. Do not replay completed stages merely to manufacture process evidence, and do not run a lifecycle locator between skills.

## Root and focused children

The main Claude thread is normally the warm accountable orchestrator. It owns selection, decisions, contradiction-preserving synthesis, durable GitHub updates, and continuation.

Use subagents, context forks, or Agent Teams only when a different oracle, context, tool, review direction, or genuinely distinct claim lane materially improves the result. A separate identity alone is not an independent control.

An explicitly spawned child owns its supplied brief. The root joins and validates material results before they become durable state.

## Candidate and concurrency boundary

One writer mutates each current candidate branch/worktree at a time. Distinct claim lanes use ordinary optimistic Git concurrency and may edit the same files, crates, or nearby semantics.

Do not create semantic-surface reservations, overlap maps, sibling-lane surveillance, or a tracked writer/agent database. If a real conflict or combined-tree interaction appears, the affected lane owns its smallest coherent repair and refreshes only affected proof/review.

Worktrees isolate PR-shaped mutation and independent validation. Do not create a new worktree merely because attention moved between passes.

## Proof and review

Every substantive pass states what its evidence establishes and what remains unproved. Missing, partial, stale, contradictory, or instrument-failed evidence is `NOT_PROVEN`, not an empty green result.

Review is cumulative and semantic:

- useful submitted reviews and inline findings are durable evidence;
- a later commit does not invalidate review merely because the SHA changed;
- a focused repair refreshes the affected finding, proof, and semantic seam;
- a material claim, production-route, authority, security, compatibility, packaging, migration, support, or rollback change refreshes the affected broader conclusions;
- formatting, editorial cleanup, generated receipt refresh, and stronger tests do not trigger a full review by themselves;
- a clean review is valid.

Do not post `Review pass (...) at head ... and claim ...` comments, compute material-claim digests for review currentness, or run the retired `review-start` / `review-done` receipt protocol.

The current head SHA identifies current code and check results. At merge time it may be used as compare-and-swap protection. That is merge safety, not review validity.

## State discipline

Persist only durable results:

- issue research, corrections, current synthesis, plan, dependencies, next action;
- specifications, ADRs, policies, and tests;
- PR claims, proof, review findings, and dispositions;
- required checks and current integration evidence;
- merge/closeout and residual work.

Keep task checkboxes, teammate liveness, retries, model routing, raw logs, provisional reasoning, and local worktree bookkeeping runtime-local.

Labels may classify area, kind, risk, release, blocker, or requested attention. They do not prove build, review, CI, merge, or lifecycle completion.

## Hard stops

Stop only for a concrete preventable hazard:

- concurrent mutation of the same candidate branch/worktree;
- destructive loss of unsalvaged work;
- unknown repository, branch, or candidate identity;
- secret or unsafe publication;
- structurally invalid durable contracts;
- unresolved substantive review findings;
- current GitHub branch protection, rulesets, merge queue, or required checks.

Otherwise detect, explain, repair, and continue.

## Canonical references

- [`CLAUDE.md`](../../CLAUDE.md)
- [Development method](../agents/DEVELOPMENT_METHOD.md)
- [GitHub surfaces](../agents/GITHUB_SURFACES.md)
- [Review and proof currentness](../agents/REVIEW_CURRENTNESS.md)
- [Skill contract](../agents/SKILL_CONTRACT.md)
- [Repository operating model](operating-model.md)
