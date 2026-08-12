# Claude repository operating contract

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
3. this file, `.claude/skills/`, and nearest package-local `CLAUDE.md`/`AGENTS.md`;
4. shared method/reference docs under `docs/agents/`;
5. Claude plans, subagents, Teams state, worktrees, memory, and conversation.

This file is Claude Code's route map. `.claude/skills/` contains executable
provider-native procedures. Shared docs define invariants and GitHub surface ownership;
they do not replace a named skill.

GitHub owns durable live transaction state. Subagent topology, liveness, retries,
temporary plans, queue order, and runtime frontier remain runtime-local.

Detailed cross-provider contracts remain in
[`docs/agents/DEVELOPMENT_METHOD.md`](docs/agents/DEVELOPMENT_METHOD.md),
[`docs/agents/GITHUB_SURFACES.md`](docs/agents/GITHUB_SURFACES.md),
[`docs/agents/REVIEW_CURRENTNESS.md`](docs/agents/REVIEW_CURRENTNESS.md), and
[`docs/agents/SKILL_CONTRACT.md`](docs/agents/SKILL_CONTRACT.md).

## Select and run the route

Choose the narrowest applicable public flow:

- `deliver-goal` — durable multi-PR outcome or umbrella;
- `deliver-pr` — one issue, PR, branch, candidate, or coherent claim;
- `prepare-issue` — problem, owner, scope, proof seam, or plan;
- `prepare-proof` — discriminating executable proof;
- `build-candidate` — implementation, test hardening, simplification, candidate
  challenge;
- `finish-pr` — publication, repair, substantive review, integration, merge, and
  reconciliation.

Enter at the earliest absent or stale useful judgment. Existing coherent work enters
midstream. Selecting a route is not completion: invoke it, follow its useful forward and
backward edges, and do not invent a parallel lifecycle.

## Operating posture

**Default-complete, recovery-forward.** Continue while useful work remains. Stop at a
real remote-owned wait, a named prerequisite, a durable hazard, or a precise
`NOT_PROVEN` boundary—not at research, a plan, one subagent return, or green checks that
do not complete the claim.

Make reasonable documented engineering decisions and proceed. Missing ceremony,
labels, receipts, or named-agent handoffs is not a reason to discard coherent work.

## Default orchestration mode

For multi-PR campaigns, broad review, queue work, or any substantive goal containing
independent claims, the parent context is a campaign manager by default.

The parent owns:

- claim and PR selection;
- compact context briefing and differentiated evidence questions;
- evidence joins and contradiction resolution;
- mutation and proof admission;
- proof-debt control;
- dependency, supersession, merge, close, and park decisions;
- durable GitHub closeout.

The parent should not normally become the first deep reviewer, routine implementer,
repetitive proof runner, CI log reader, or worktree janitor merely because direct work is
available. Direct parent leaf work is exceptional: one load-bearing inspection needed
to choose the route, a tiny integration repair after the claim is understood, or an
immediate blocker when useful subagent capacity cannot be recovered.

A failed dispatch is not by itself permission to absorb the task. First join completed
returns, close completed subagents, reclaim useful capacity, route another decision, or
continue integration work already supported by evidence.

## Persistent contexts, role specialization, and skills

The normal claim owner is one persistent lane agent per PR or coherent claim. It runs
`deliver-pr` and keeps its thread, loaded source context, and worktree across review,
repair, proof, review refresh, live CI, and closeout.

Agent context, role, and skill are separate:

- **context** preserves the PR, claim, source map, evidence, and worktree;
- **role** biases attention and default authority, such as claim owner or independent
  reviewer;
- **skill** supplies the executable procedure and typed next route for the current
  judgment.

Do not create a new subagent merely because the next skill changes. Equally, do not
forbid role-specialized subagents when they improve evidence. A dedicated review context
may retain one PR across `review-pr`, `review-candidate`, `review-tests`, external-oracle
work, and re-review without re-ingesting the candidate for every angle.

A claim lane or dedicated reviewer that finds a bounded candidate-owned defect may fix
it in the same context when the parent grants mutation/publication authority and no
other writer owns the candidate. The repair still returns through affected proof and
review. If that reviewer becomes the writer, final review must retain a genuinely
different oracle, method, threat model, environment, or review context where
substantive independence requires it.

Use a new context when it creates real independence, reaches a different environment,
owns a split claim or prerequisite, or reduces high-output evidence. Do not spawn one
subagent per skill or review lens merely to repeat the same PR ingestion.

The parent brief should usually contain only:

- PR/claim and accepted non-goals when not obvious from GitHub;
- desired context or specialist role when it matters;
- mutation/publication authority;
- merge/close/issue-creation authority;
- worktree permission and local proof budget;
- known prerequisite, finding, review dimension, or hosted wake event.

The repository skills own the procedure and next-step routing. Do not restate every
review, repair, proof, CI, and cleanup rule in each dispatch.

Use ordinary subagents when independent evidence returns to the lane root. Use Agent
Teams only when lateral communication changes the result. Use Ultracode inside one
coherent claim when tasks become ready dynamically; it is not repository state or a
cross-claim scheduler.

## Breadth, not subagent occupancy

For a large PR queue, roughly five or six disjoint PR contexts may be useful when the
runtime and queue support them. That is a default review fan-out, not a topology, quota,
role mix, or occupancy target.

Keep only contexts whose next result can change a decision. Do not keep stale handles,
duplicate waits, low-value work, or already-completed contexts alive to preserve a
number.

The live context set must be deduplicated and current:

- remove completed, closed, cancelled, or missing handles immediately;
- a context moving from review to repair remains one context and one handle;
- a reviewer consuming another review skill remains the same reviewer;
- wait only on the current live set;
- refill capacity only when another independent claim or evidence direction is useful;
- do not terminate bounded work merely to refresh the pool display.

Consume each return as it arrives. Do not wait for the whole batch before merging,
closing, parking, recording a blocker, or resuming a context on its next skill.

## Context hierarchy

### Campaign root

Owns goal meaning, acceptance predicates, claim selection, dependencies,
contradictions, runtime-local frontier, joined evidence, proof debt, exceptions, and
goal reconciliation.

The campaign root keeps claim discovery broad, mutation bounded, proof moving, and
converged candidates closing. Leaf implementation, first-pass deep review, broad
archaeology, raw logs, repetitive proof, CI diagnosis, and routine cleanup belong in
persistent claim contexts, subagents, context forks, or bounded Ultracode workflows.

### Persistent claim lane

Owns one coherent acceptance-and-rollback claim. It runs `deliver-pr`, invokes
`orchestrate-work` for missing evidence, keeps at most one concurrent writer on its
candidate, joins claim-local evidence, and returns a typed result.

The lane remains the same context while skills change its activity. Review may lead to
repair; repair may lead to proof; proof may lead back to review; current review may lead
to live CI and closeout. Do not discard its cache or worktree at ordinary skill
boundaries.

A lane returns to the campaign root at a real remote wait, terminal disposition, named
prerequisite, durable hazard, external-action boundary, or precise `NOT_PROVEN`
boundary.

### Role-specialized context

Owns one PR plus one durable attention bias, such as independent substantive review. It
may consume several related skills and lenses without being replaced. A review context
may inspect claim-vs-code, proof discrimination, production reachability, external
truth, compatibility, risk, and rollback sequentially in the same loaded context.

A specialist does not automatically become claim owner or merge authority. It may be
promoted in place to bounded mutation when the parent grants authority and the
same-candidate writer boundary remains clear. Its evidence returns to the claim lane or
campaign root for cumulative judgment.

### Focused evidence worker or lens

Answers one bounded question or consumes one named skill. It returns findings,
falsifiers, contradictions, uncertainty, and references—not approval or merge
authority.

Read-only work normally requires no worktree. Allocate one only when checkout-local
inspection, proof, or another environment materially changes the evidence. A child that
creates a worktree or process group owns its cleanup.

## Campaign execution

Use `orchestrate-work` after selecting a public flow or substantive atomic skill.

```text
multi-PR campaign
→ dispatch useful disjoint claim lanes and role-specialized review contexts
→ each context follows repository skills without stage-driven replacement
→ join each result as it arrives
→ admit mutation and focused proof separately from review breadth
→ merge, close, park, or record a named blocker
→ refill only when another independent claim or evidence direction is useful
```

Keep review breadth wider than mutation breadth. Claim lanes and dedicated reviewers may
begin with review, consume several review skills, then continue into authorized
candidate mutation without being replaced. Writers and heavy proof are admitted by
claim independence, proof debt, and host capacity—not by a fixed count.

Default to subagents when they preserve campaign context, compress high-output
evidence, change source/oracle/tool/environment/threat model, reduce elapsed time,
improve recovery, or avoid expensive CI cycles. Stop adding subagents when another
result cannot change a decision.

Do not poll unchanged remote state or wait serially for an entire batch.

## Skill-directed continuity

Skills route the current context according to their typed result.

```text
`review-pr`: CHANGES_REQUIRED
→ current authorized context `address-review-comments` / `build-candidate`
→ affected proof
→ affected `final-challenge` / `review-pr`

`review-pr`: REVIEW_CURRENT
→ claim lane `verify-live-ci`

`verify-live-ci`: PRODUCT_OR_TEST_FAILURE
→ claim lane or authorized reviewer `build-candidate`
→ affected proof and review

`verify-live-ci`: INTEGRATION_READY
→ claim lane `merge-reconcile` when authorized
```

Use each skill's routes or valid exits. Do not return an intermediate review packet
merely so another subagent can rediscover the PR. Do not spawn a new reviewer for every
review angle when one reviewer can reliably consume the required skills in the same
context.

Split to a new context only when the durable claim splits, a separate prerequisite gains
an owner, or a genuinely independent evidence source, oracle, threat model, environment,
or attention surface can change the decision.

When GitHub owns the next transition, return `IN_FLIGHT` with the exact wake event. Resume
the same context when the runtime retains it; otherwise reconstruct from GitHub and
repository artifacts without creating a rival candidate.

## Proof and convergence control

Review output is not repository progress until useful findings are disproved, repaired,
or converted into durable blockers. Published repairs are not solid state until their
affected proof and review converge.

Maintain a useful proof path:

- when behavioral repairs need proof and the host permits it, keep focused proof moving;
- use the smallest command that can falsify the changed seam;
- do not start many heavy Cargo jobs merely because many contexts exist;
- when proof debt accumulates, stop starting more mutation and keep remaining capacity
  on review/evidence/integration work;
- a published repair with missing affected proof remains `PR_IN_FLIGHT / NOT_PROVEN`
  unless a known hosted gate directly exercises the seam;
- formatting and `git diff --check` do not prove changed behavior;
- a hosted gate may discharge local instrument limits, but not an unrelated or
  self-attested check.

A `NOT_PROVEN` repair may be published once when a specific hosted gate will exercise
the changed seam. It is not repaired, solid, review-current, or merge-ready until that
proof passes.

When a candidate becomes review-current and required proof is acceptable, merge or
closeout outranks opening another speculative repair. Discovery must not outrun proof
and integration indefinitely.

## Claude-native PR review

For substantive PRs the native route is:

```text
`finish-pr`
→ `address-review-comments`
→ `final-challenge`
→ differentiated `orchestrate-work` lenses, optionally in one persistent reviewer
→ cumulative `review-pr`
→ REVIEW_CURRENT | CHANGES_REQUIRED | NOT_PROVEN |
  BLOCKED_BY_PREREQUISITE | SUPERSEDED_OR_CLOSE
→ current context follows the result
→ only REVIEW_CURRENT enters `verify-live-ci`
→ INTEGRATION_READY | PR_IN_FLIGHT | MERGE_BLOCKED | NOT_PROVEN
→ `merge-reconcile`
```

Review is not diff reading, green CI, mergeability, zero threads, bot approval, or one
subagent verdict. It must proportionately challenge proof discrimination, production
reachability, external truth, claim honesty, semantic authority, compatibility, risk,
and rollback.

One dedicated reviewer may examine several angles by consuming several review skills in
its shared PR context. Separate reviewer contexts are warranted when their independence
comes from a different source, oracle, method, threat model, environment, or attention
surface—not merely a different subagent identity or stage label.

The construction context must not be the only detection surface supporting a
substantive merge. Independence comes from changed evidence, oracle, method, threat
model, environment, or attention—not identity alone.

A clean review is valid. Do not manufacture findings or edits to demonstrate that
review happened.

## Worktrees, Git, and currentness

A worktree and branch are the writer's operational context, not an exact-head lease.
Do not repeatedly reauthenticate an unchanged context with `ls-remote`, PR metadata, or
expected-SHA checks.

Commit and push normally without force. If Git rejects a non-fast-forward push, fetch
and inspect the intervening work. Integrate compatible changes, resolve an actual
conflict, or return a material supersession or ownership blocker. A compatible remote
commit is ordinary collaborative Git, not `CANDIDATE_MOVED`.

This repository squash-merges. Keep candidate, integration, and landed evidence
distinct:

- material candidate change → rerun affected proof and review;
- actual conflict or demonstrated combined-tree interaction → repair and review that
  seam;
- unrelated `main` movement → no rebase, merge-from-main, branch refresh, empty commit,
  full CI replay, or review churn;
- head SHA change alone → no ownership loss or review invalidation;
- current head SHA is useful for CI attribution and final merge compare-and-swap only.

Do not merge or rebase `origin/main` into a candidate as an entry ritual, freshness
repair, or response to an old/dirty label. Base integration requires a concrete reason:
an actual conflict, a demonstrated combined-tree interaction, or a named prerequisite
the candidate now consumes.

If base integration leaves no candidate-owned change, do not publish merge-only history.
Stop, discard the integration commits, and return the real disposition.

## Useful GitHub handoffs

Publish only information that remains useful after the current context disappears:

- changed claim, authority, proof obligation, prerequisite, support, risk, or rollback;
- source-backed evidence that would otherwise be rediscovered;
- a localized review finding or evidence-backed disposition;
- a real external wait and wake event;
- a cumulative review, merged effect, closeout, or goal synthesis.

Use issues for durable research, rulings, plans, dependencies, and successor work. Use
PR bodies/comments for candidate-wide proof or limitation summaries, inline review for
localized findings, and submitted reviews for cumulative judgment.

Keep subagent identity, topology, liveness, retries, ordinary skill transitions, raw
logs, unchanged polling, and temporary task state runtime-local.

## Hard stops

Stop only for concrete hazards:

- two writers would mutate the same candidate concurrently;
- destructive cleanup would lose unsalvaged work;
- repository, candidate, or material-claim authority cannot be established;
- a secret or unsafe release would be published;
- a durable contract is structurally invalid;
- substantive findings remain unresolved or review is `NOT_PROVEN` at merge;
- current rulesets, required checks, mergeability, or queue state block integration.

A branch-head change, unrelated `main` movement, failed dispatch, pending hosted check,
stale subagent handle, or unavailable cleanup operation is not by itself a hard stop.
Otherwise detect, explain, repair, delegate, and continue independent campaign work.

## Repository and Claude hygiene

- read nearest package-local owner guidance before modifying an owning crate;
- production code must not use `unwrap`, `expect`, `panic!`, `todo!`,
  `unimplemented!`, `abort`, or `dbg!` outside documented narrow exceptions;
- never use `git stash` in worktrees; use scoped restore or a WIP commit;
- stage intended paths explicitly;
- use one worktree per genuine concurrent write claim, not per lifecycle pass;
- a persistent claim lane or reviewer may retain its worktree across review, repair, and
  proof when the same context will continue;
- the child that creates a worktree cleans it after retained work is safely published
  or abandoned and no near-term same-context transition needs the cache;
- parent contexts verify cleanup from typed returns and perform broad cleanup only when
  storage blocks work or the campaign is closing;
- preserve shared targets/caches, locked or ambiguous worktrees, and state owned by
  another subagent or tool;
- use short worktree paths on Windows and attribute path-length, shell, timeout, and
  host-saturation failures to the instrument until proven candidate-owned;
- run focused proof, then affected package proof, then broader proof only when risk or
  the merge gate selects it;
- do not run repository-wide Clippy or tests after every edit;
- shared `.claude/settings.json` remains portable and minimal; personal permissions,
  bypass posture, model routing, experimental choices, and broad command allowlists
  belong in user/local settings.

Useful commands:

```bash
just doctor
cargo fmt -p <package> -- --check
cargo clippy -p <package> --all-targets --locked -- -D warnings
cargo test -p <package> --all-targets --locked
just pr-fast
```

Choose the smallest command that can falsify the claim. Current GitHub protection
remains authoritative at merge.
