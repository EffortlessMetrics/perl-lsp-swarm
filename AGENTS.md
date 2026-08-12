# Repository operating contract

## Product direction

perl-lsp is becoming a compiler-backed Perl toolchain whose parser, compiler facts,
workspace model, LSP, DAP, packaging, and editor behavior remain honest about source,
freshness, confidence, fallback, and dynamic boundaries.

Optimize for user-visible closure, semantic ownership, deterministic proof, and
maintainable current-main behavior—not local component completion or workflow
compliance.

## Sources of truth

Use the highest applicable current authority:

1. current `origin/main`, live GitHub issues, PRs, reviews, checks, rulesets, and actual
   repository behavior;
2. accepted specifications, ADRs, policies, generated contracts, and independent proof;
3. this file, `.agents/skills/`, and nearest package-local `AGENTS.md` guidance;
4. shared method/reference docs under `docs/agents/`;
5. runtime plans, agents, worktrees, memory, and conversation.

Issue #3949 owns the repository development protocol. Issues #3786, #3985, #3987,
#3988, and #3989 own the staged, pre-push, current-head CI, merge, and reconciliation
boundaries. `$change-graph` is their compiled route for agents; it is not a second
lifecycle authority.

GitHub owns durable live transaction state. Agent topology, liveness, retries, temporary
plans, queue order, proof-token allocation, and runtime frontier remain runtime-local.

Detailed cross-provider contracts remain in
[`docs/agents/DEVELOPMENT_METHOD.md`](docs/agents/DEVELOPMENT_METHOD.md),
[`docs/agents/GITHUB_SURFACES.md`](docs/agents/GITHUB_SURFACES.md),
[`docs/agents/REVIEW_CURRENTNESS.md`](docs/agents/REVIEW_CURRENTNESS.md), and
[`docs/agents/SKILL_CONTRACT.md`](docs/agents/SKILL_CONTRACT.md).

## Orchestrator bootstrap

First determine the context's authority.

- A campaign root or persistent claim lane automatically ingests `$change-graph` once,
  after root and nearest package guidance.
- A campaign root then runs `$deliver-goal` and uses `$orchestrate-work` for independent
  claims and evidence.
- A persistent claim lane runs `$deliver-pr` and retains its context across the skills
  selected by that route.
- A role-specialized reviewer reads the assigned review skills and nearest guidance. It
  does not ingest the full campaign graph unless promoted to claim ownership.
- A focused worker reads only the named skill, bounded question, accepted facts, and
  nearest guidance.

Do not re-ingest the graph at every skill boundary. A skill transition changes the work
inside the current context; it does not require a cold start or another full route read.

## Select and run the route

Choose the narrowest applicable public flow:

- `$deliver-goal` — durable multi-PR outcome or umbrella;
- `$deliver-pr` — one issue, PR, branch, candidate, or coherent claim;
- `$prepare-issue` — problem, owner, scope, proof seam, or plan;
- `$prepare-proof` — discriminating executable proof;
- `$build-candidate` — implementation, test hardening, simplification, candidate
  challenge;
- `$finish-pr` — publication, repair, substantive review, integration, merge, and
  reconciliation.

Enter at the earliest absent or stale useful judgment. Existing coherent work enters
midstream. Selecting a route is not completion: invoke it, follow its useful forward and
backward edges, and do not invent a parallel lifecycle.

## Operating posture

**Default-complete, recovery-forward.** Continue while useful work remains. Stop at a
real remote-owned wait, a named prerequisite, a durable hazard, an external-action
boundary, or a precise `NOT_PROVEN` boundary—not at research, a plan, one agent return,
or green checks that do not complete the claim.

Make reasonable documented engineering decisions and proceed. Missing ceremony,
labels, receipts, or named-agent handoffs is not a reason to discard coherent work.

## Context, role, and skill

Keep three objects separate:

- **context** preserves the durable subject, source map, evidence, and worktree;
- **role** biases attention and default authority, such as claim owner or independent
  reviewer;
- **skill** supplies the executable procedure and typed next route.

The normal claim owner is `.codex/agents/pr-lane.toml`. It keeps one PR or coherent
claim loaded across review, repair, proof, review refresh, live CI, and closeout.

`.codex/agents/pr-reviewer.toml` is a useful role-specialized context. One reviewer may
consume `$review-pr`, `$review-candidate`, `$review-tests`, external-oracle,
production-path, security, compatibility, and re-review skills without rereading the PR
for every angle.

A claim lane or reviewer that finds a bounded candidate-owned defect may fix it in the
same context when the parent grants mutation/publication authority and no other writer
is mutating the candidate. The repair returns through affected proof and review. Where
construction and review would otherwise share one detection surface, add a genuinely
different oracle, method, threat model, environment, or reviewer before merge.

Use a new context when it creates real independence, reaches a different environment,
owns a split claim or prerequisite, or compresses high-output evidence. Do not create one
agent per skill or review lens merely to repeat ingestion.

## Default orchestration mode

For multi-PR campaigns, broad review, queue work, or any substantive goal containing
independent claims, the parent context is a campaign manager by default.

The parent owns:

- goal meaning, claim and PR selection;
- compact context briefing and differentiated evidence questions;
- evidence joins and contradiction resolution;
- mutation, proof, and host-capacity admission;
- proof-debt control;
- dependency, supersession, merge, close, and park decisions;
- durable GitHub closeout.

The parent should not normally become the first deep reviewer, routine implementer,
repetitive proof runner, CI log reader, or worktree janitor merely because direct work is
available. Direct parent leaf work is exceptional: one load-bearing inspection needed
to choose the route, a tiny integration repair after the claim is understood, or an
immediate blocker when useful agent capacity cannot be recovered.

A failed spawn is not by itself permission to absorb the task. First join completed
returns, close completed contexts, reclaim useful capacity, route another decision, or
continue integration work already supported by evidence.

For a large queue, roughly five or six disjoint PR/review contexts may be useful. This is
review fan-out, not a topology, quota, role mix, or occupancy target.

- keep only contexts whose next result can change a decision;
- remove completed, closed, cancelled, or missing handles immediately;
- a context moving from review to repair remains one context and one handle;
- wait only on the current deduplicated live set;
- consume each result as it arrives rather than waiting for a batch;
- keep review breadth wider than mutation and heavy-proof breadth;
- stop starting mutation when proof debt grows; keep review and integration work moving;
- merge or close a converged candidate before opening another speculative repair.

## Durable state and early encoding

`$change-graph` defines the canonical projection. Encode information at the first
boundary where another competent context would otherwise need to rediscover it:

- issue body: current problem, claim, owner, plan, acceptance, non-goals, prerequisites;
- issue comments: research, contradictions, corrected assumptions, decision history;
- `.spec/`, ADR, policy, schema, or contract: settled cross-PR/public invariants;
- test, fixture, oracle, or proof artifact: observed discriminating red and limitations;
- `.changes/unreleased/`: user-visible disposition while context is fresh;
- PR body: cumulative claim, production path, proof, hardening, simplification,
  deviations, risk, rollback, and limitations;
- review threads and submitted review: localized findings/dispositions and cumulative
  judgment;
- GitHub checks: current-head clean-checkout, platform, policy, and integration facts;
- merge/issue closeout: landed effect, residual work, support/proof/Changie state, and
  safe cleanup.

Do not write agent handles, liveness, queue order, retries, ordinary skill transitions,
private reasoning, or temporary frontier state into GitHub or tracked files.

## Skill-directed continuity

Use each skill's `Routes`, `Valid exits`, or equivalent transition table. Common routes
inside the same authorized PR context are:

```text
$review-pr: CHANGES_REQUIRED
→ $address-review-comments / $build-candidate
→ affected proof
→ affected $final-challenge / $review-pr

$review-pr: REVIEW_CURRENT
→ $verify-live-ci

$verify-live-ci: PRODUCT_OR_TEST_FAILURE
→ $build-candidate
→ affected proof and review

$verify-live-ci: INTEGRATION_READY
→ $merge-reconcile when authorized
```

Do not return an intermediate review packet merely so another agent can rediscover the
PR. Split to a new claim lane only when the durable claim itself splits or a separate
prerequisite gains an accountable owner.

When GitHub owns the next transition, return `IN_FLIGHT` with one exact wake event and
continue independent campaign work. Resume the same context when the runtime retains it;
otherwise reconstruct from durable state without creating a rival candidate.

## Local feedback ladder

Place work at the earliest reliable input boundary.

### Before commit

The installed hook runs `cargo xtask precommit` against the exact staged tree. This tier
owns cheap structure, including staged Changie fragment validation, staged-blob rustfmt,
whitespace/conflict markers, executable mode, structured syntax, machine paths, and
size/binary policy.

Do not put Cargo compilation or RIPR in the commit tier. RIPR has no exact staged-index
input and the commit gate has a sub-30-second contract.

### Before push and publication

The affected committed-diff boundary owned by #3985 uses the shared change-set authority
for affected Cargo proof, focused tests, Changie dry rendering, and diff-scoped RIPR
routing. Use the repository hook/xtask surface; do not recreate base selection or package
classification in a skill.

Local proof is candidate evidence, not merge authorization. Missing input identity,
instrument failure, or unavailable environment remains `NOT_PROVEN`.

### In GitHub

CI owns current-head and integration facts: clean checkout, live policy, required checks,
platform/packaging/external environments, merge-group interactions, and other remote-only
proof. It should not be the first place ordinary staged or affected defects are found.

Do not retire a required remote check merely because a local hook exists. Replacement
requires #3987/#3988 parity, provenance, ruleset, merge-group, and alternate-path proof.

## Review and integration

For substantive PRs:

```text
$finish-pr
→ $address-review-comments
→ $final-challenge
→ differentiated $orchestrate-work lenses
→ cumulative $review-pr
→ REVIEW_CURRENT | CHANGES_REQUIRED | NOT_PROVEN |
  BLOCKED_BY_PREREQUISITE | SUPERSEDED_OR_CLOSE
→ only REVIEW_CURRENT enters $verify-live-ci
→ INTEGRATION_READY | PR_IN_FLIGHT | MERGE_BLOCKED | NOT_PROVEN
→ $merge-reconcile
```

Review is not diff reading, green CI, mergeability, zero threads, bot approval, or one
agent verdict. It proportionately challenges proof discrimination, production
reachability, external truth, claim honesty, semantic authority, compatibility, risk,
and rollback. A clean review is valid; do not manufacture findings.

## Worktrees, Git, and currentness

A worktree and branch are operational context, not an exact-head lease.

- keep at most one writer mutating a candidate at a time;
- commit and push normally without force;
- if a push is rejected, fetch and inspect the intervening work;
- integrate compatible changes or resolve an actual conflict;
- do not treat a compatible head change as ownership loss or `CANDIDATE_MOVED`;
- unrelated `main` movement requires no rebase, merge-from-main, empty commit, full CI
  replay, or review restart;
- integrate the base only for an actual conflict, demonstrated combined-tree
  interaction, or named prerequisite now consumed by the candidate;
- refresh only proof and review affected by material candidate change;
- use a SHA for CI attribution and final merge compare-and-swap, not scheduling or
  ownership.

This repository squash-merges. Candidate commits are operational history; the cumulative
PR claim and final squash result are the review and landed objects.

## Useful GitHub handoffs

Publish only information that remains useful after the current context disappears:

- changed claim, authority, plan, proof obligation, prerequisite, support, risk, or
  rollback;
- source-backed evidence that would otherwise be rediscovered;
- localized findings and evidence-backed dispositions;
- a real external wait and wake event;
- cumulative review, integration, merged effect, closeout, or goal synthesis.

Use issues for research, rulings, plans, dependencies, and successor work. Use PR bodies
or comments for candidate-wide proof/limitations, inline review for localized findings,
and submitted review for cumulative judgment. Keep routine progress and unchanged
polling runtime-local.

## Hard stops

Stop only for concrete hazards:

- two writers would mutate the same candidate concurrently;
- destructive cleanup would lose unsalvaged work;
- repository, candidate, claim, or authority cannot be established;
- a secret or unsafe release would be published;
- a durable contract is structurally invalid;
- substantive findings remain unresolved or review is `NOT_PROVEN` at merge;
- current rulesets, required checks, mergeability, or queue state block integration.

A branch-head change, unrelated `main` movement, failed spawn, pending hosted check,
stale handle, or unavailable cleanup operation is not by itself a hard stop. Otherwise
detect, explain, repair, delegate, and continue independent work.

## Repository hygiene and local proof

- read nearest package-local guidance before modifying an owning crate;
- production code must not use `unwrap`, `expect`, `panic!`, `todo!`,
  `unimplemented!`, `abort`, or `dbg!` outside documented narrow exceptions;
- never use `git stash` in worktrees; use scoped restore or a WIP commit;
- stage intended paths explicitly;
- use one worktree per genuine concurrent write claim, not per lifecycle pass;
- retain a PR worktree across near-term review, repair, and proof when the same context
  will continue;
- the child that creates a worktree or process group owns its cleanup after retained work
  is safely published or abandoned and the cache is no longer useful;
- preserve shared targets/caches, locked or ambiguous worktrees, and state owned by
  another agent or tool;
- use short worktree paths on Windows and attribute path-length, shell, timeout, and
  host-saturation failures to the instrument until proven candidate-owned;
- run focused proof, then affected package proof, then broader proof only when risk or
  the merge gate selects it;
- do not run repository-wide Clippy or tests after every edit.

Useful commands:

```bash
just doctor
cargo xtask precommit
cargo fmt -p <package> -- --check
cargo clippy -p <package> --all-targets --locked -- -D warnings
cargo test -p <package> --all-targets --locked
just pr-fast
```

Choose the smallest command that can falsify the claim. Current GitHub protection remains
authoritative at merge.
