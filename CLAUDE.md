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

This file is Claude Code's route map. `.claude/skills/` contains the executable
provider-native procedures. Shared docs define invariants and GitHub surface ownership;
they do not replace a named skill.

GitHub owns durable live transaction state. Runtime topology, frontier, task order,
liveness, retries, and temporary plans are not repository authority and must not be
written to tracked state files.

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
- `finish-pr` — publication/resume, repair, substantive review, integration, merge,
  reconciliation.

Enter at the earliest absent or stale useful judgment. Existing coherent work enters
midstream. Selecting a route is not completion: invoke it, follow its named normal and
material backward edges, and do not invent a parallel lifecycle or run a stage locator.

## Operating posture

**Default-complete, recovery-forward.** Continue through every applicable judgment in
the selected route until the claim is reconciled, reaches a real remote-owned wait, or
returns a precise blocker or `NOT_PROVEN` boundary. Do not stop at research, a plan, a
subagent result, or green checks when the route still contains useful work.

Make reasonable documented engineering decisions and proceed. Missing historical
ceremony, labels, receipts, or named-agent handoffs is not a reason to discard coherent
work; perform the cheapest still-useful repair and continue.

## Scope hierarchy

### Campaign root

Owns goal meaning, acceptance predicates, claim selection, cross-lane dependencies,
contradictions, runtime-local frontier, joined evidence, exceptions, and goal
reconciliation.

For substantive work the campaign root normally orchestrates. Leaf implementation,
broad archaeology, raw logs, repetitive proof, and review exploration should run in
claim-local lanes, subagents, context forks, or Ultracode workflows so the campaign
context remains decision-rich and raw-output-light.

### Lane root

Owns one coherent acceptance-and-rollback claim. It runs `deliver-pr`, invokes
`orchestrate-work`, keeps one candidate writer, joins claim-local evidence, publishes
useful GitHub updates, and returns a typed result to its campaign root.

A lane root may directly perform tiny tightly coupled claim-local work when briefing
and joining cost more than the context saved. That does not make campaign-root leaf
execution the default.

### Worker, writer, and reviewer

- read-only subagents answer one bounded question or consume one named skill;
- one writer mutates the selected candidate branch/worktree;
- reviewers change the evidence surface and return findings, falsifiers,
  contradictions, uncertainty, and references—not approval.

A leaf worker may not widen into claim orchestration unless the brief explicitly grants
lane-root authority.

## Claude orchestration

Use `orchestrate-work` after selecting a public flow or substantive atomic skill.

Normal shapes:

```text
campaign outcome
→ campaign root runs `deliver-goal`
→ substantial claims become whole-flow `deliver-pr` lane agents

claim lane
→ lane root runs the named route
→ focused subagents consume named skills or bounded questions
→ one writer integrates candidate mutation
→ differentiated reviewers challenge proof and candidate
→ lane root joins evidence and returns a typed result
```

Use a compact whole-flow brief:

```text
Take issue #123 through `deliver-pr`.
You are the accountable lane root for this claim. Use GitHub as durable state, invoke
`orchestrate-work` within the claim, keep one candidate writer, follow the public flow's
normal and material backward routes, and return the typed lane result.
Do not select unrelated claims or alter the parent goal.
```

For focused work, name the skill, exact subject, accepted authority and facts,
read/write boundary, falsifiers, sufficient return, stop conditions, and non-goals.
Require the child to consume the named skill when supplied.

Choose agents when they preserve campaign/lane context, compress high-output evidence,
change source/oracle/tool/environment/threat model, reduce elapsed time, improve
recovery, or avoid expensive CI cycles. Stop adding agents when another result cannot
change a decision.

Use ordinary subagents when independent results return to the lane root. Use Agent
Teams only when lateral communication changes the result. Use Ultracode inside one
coherent claim when tasks become ready dynamically; it is not repository state or a
cross-claim scheduler.

Keep campaign frontier and wake events in runtime memory only. Reconstruct them from
issues, PRs, reviews, checks, merges, and repository artifacts after compaction or
replacement. Do not poll unchanged remote state.

## Useful GitHub handoffs

Post or update GitHub only when information remains useful after the current context
disappears:

- claim, authority, plan, proof obligation, route, prerequisite, support, risk, or
  rollback meaning changed;
- source-backed evidence would otherwise be rediscovered;
- a localized finding belongs in an inline review;
- a finding receives an evidence-backed disposition;
- a real external wait and wake event need to survive handoff;
- a useful cumulative review, merged effect, or goal synthesis is ready.

Use issues for durable research/rulings/plans/dependencies/goal synthesis; PR bodies or
comments for candidate-wide route/proof/limitation summaries; inline reviews for
localized findings; review replies for dispositions; submitted reviews for cumulative
judgment; and issue closeout for landed effects and residual claims.

Keep agent identity, topology, liveness, retries, task state, provisional reasoning,
raw logs, unchanged polling, and routine skill transitions runtime-local.

When another context benefits and the route is not obvious, post one compact route
declaration with parent goal, claim, entry flow, current named transition, reason,
durable subject, and wake event. Update it only when the material route changes. It is
a resumability aid, not lifecycle authority.

## Claude-native PR review

For substantive PRs the native route is:

```text
`finish-pr`
→ `address-review-comments` for existing findings
→ `final-challenge`
→ `orchestrate-work` for differentiated review lenses
→ cumulative `review-pr`
→ REVIEW_CURRENT | CHANGES_REQUIRED | NOT_PROVEN |
  BLOCKED_BY_PREREQUISITE | SUPERSEDED_OR_CLOSE
→ only REVIEW_CURRENT enters `verify-live-ci`
→ INTEGRATION_READY | PR_IN_FLIGHT | MERGE_BLOCKED | NOT_PROVEN
→ `merge-reconcile`
```

Review is not diff reading, green CI, mergeability, zero threads, bot approval, or a
subagent verdict. It must proportionately challenge proof discrimination, production
reachability, external truth, claim honesty, semantic authority/complexity, and
risk/rollback.

The construction context must not be the only detection surface supporting a
substantive merge. Independence comes from changed evidence, oracle, method, threat
model, environment, or attention—not identity alone.

A clean review is valid. Do not manufacture findings or edits to demonstrate that the
review happened.

Substantive review and integration posture remain separate. Pending remote checks leave
review current and return `PR_IN_FLIGHT`.

## Proof and currentness

Keep candidate, integration, and landed evidence distinct.

- material candidate change → rerun affected proof and review;
- actual conflict or combined-tree interaction → repair and review the affected seam;
- unrelated `main` movement with a conflict-free candidate → no rebase, branch refresh,
  full CI replay, or review churn;
- head SHA change alone → no review invalidation;
- merge uses current head only as compare-and-swap protection.

Never weaken a test, ratchet, support claim, or required proof merely to obtain green
status. Missing, partial, stale, contradictory, or instrument-failed evidence is
`NOT_PROVEN`.

## Hard stops

Stop only for concrete hazards:

- two writers would mutate the same candidate concurrently;
- destructive cleanup would lose unsalvaged work;
- repository/candidate/material-claim identity or authority cannot be established;
- a secret or unsafe release would be published;
- a durable contract is structurally invalid;
- substantive findings remain unresolved or review is missing/`NOT_PROVEN` at merge;
- current rulesets, required checks, mergeability, or queue state block integration.

Otherwise detect, explain, repair, and continue.

## Repository and Claude hygiene

- read nearest package-local owner guidance before modifying an owning crate;
- production code must not use `unwrap`, `expect`, `panic!`, `todo!`,
  `unimplemented!`, `abort`, or `dbg!` outside documented narrow exceptions;
- never use `git stash` in worktrees; use scoped restore or a WIP commit;
- stage intended paths explicitly;
- use one worktree per genuine concurrent write claim, not per lifecycle pass;
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
