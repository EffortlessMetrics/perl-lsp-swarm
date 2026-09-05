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
5. runtime plans, workers, worktrees, memory, and conversation.

Document applicability and status are governed by
[`docs/agents/AUTHORITY_STATUS.md`](docs/agents/AUTHORITY_STATUS.md) and its
[machine registry](docs/agents/authority_status.toml). Before using an agent,
review, queue, branch, or orchestration spec/ADR/doc, check that registry. A path listed
as `transitional`, `historical`, or `superseded` does not re-enter the hierarchy merely
because its own old header says “accepted,” “active doctrine,” “north star,” or
“current.”
The registry classifies documentation authority; current source and live GitHub evidence still
govern the underlying facts and transaction state.

This file is Codex's route map. `.agents/skills/` contains the executable provider-
native procedures. Shared docs define invariants and GitHub surface ownership; they do
not replace a named `$skill`.

GitHub owns durable live transaction state. Runtime topology, claim-frame ordering,
liveness, retries, and temporary plans are not repository authority and must not be
written to tracked state files.

Detailed cross-provider contracts remain in
[`docs/agents/DEVELOPMENT_METHOD.md`](docs/agents/DEVELOPMENT_METHOD.md),
[`docs/agents/GITHUB_SURFACES.md`](docs/agents/GITHUB_SURFACES.md),
[`docs/agents/REVIEW_CURRENTNESS.md`](docs/agents/REVIEW_CURRENTNESS.md), and
[`docs/agents/SKILL_CONTRACT.md`](docs/agents/SKILL_CONTRACT.md).

## Select and run the route

Choose the narrowest applicable public flow:

- `$deliver-goal` — durable multi-PR outcome or umbrella;
- `$deliver-pr` — one issue, PR, branch, candidate, or coherent claim;
- `$prepare-issue` — problem, owner, scope, proof seam, or plan;
- `$prepare-proof` — discriminating executable proof;
- `$build-candidate` — implementation, test hardening, simplification, candidate
  challenge;
- `$finish-pr` — publication/resume, repair, substantive review, integration, merge,
  reconciliation.

Enter at the earliest absent or stale useful judgment. Existing coherent work enters
midstream. Selecting a route is not completion: invoke it, follow its named normal and
material backward edges, and do not invent a parallel lifecycle or run a stage locator.

## Operating posture

**Default-complete, recovery-forward.** Continue through every applicable judgment in
the selected route until the claim is reconciled, reaches a real remote-owned wait, or
returns a precise blocker or `NOT_PROVEN` boundary. Do not stop at research, a plan, a
worker result, or green checks when the route still contains useful work.

Make reasonable documented engineering decisions and proceed. Missing historical
ceremony, labels, receipts, or named-agent handoffs is not a reason to discard coherent
work; perform the cheapest still-useful repair and continue.

## Root orchestration and claim frames

The **main/root Codex thread is the accountable orchestrator**. It owns both goal-level
and claim-level orchestration:

- user goal, current interpretation, constraints, and acceptance predicates;
- selection and ordering of coherent claims;
- a runtime-local frame for each active or resumable claim;
- cross-claim prerequisites and contradictions;
- joined evidence and review sufficiency;
- writer allocation and mutation ownership;
- remote waits and wake events;
- GitHub publication, merge judgment, reconciliation, and continuation.

A claim or lane is a **logical frame held by the root**, not normally another
orchestrator agent. These logical claim frames keep claim orchestration in the root
while bounded contexts execute research, mutation, and review. A claim frame may contain:

```text
claim
acceptance predicate
controlling issue / PR / merge subject
current candidate and writer, if any
current route or missing judgment
proof / review status and limitations
external wait and wake event
```

Keep that frame runtime-local and reconstruct it from GitHub/repository artifacts after
replacement. Do not serialize agent liveness or create a second claim-state database.

The root should not personally absorb every high-volume leaf operation. It delegates
bounded **programmes** when another context materially helps:

- research / archaeology / external truth / CI evidence → researcher or focused worker;
- candidate or proof mutation → one writer/builder context;
- fixed-subject adversarial judgment → reviewer context;
- tightly coupled work needing provider-native lateral communication → a team only when
  communication changes the result.

Recursive orchestration is an optional physical execution technique, not the logical
architecture. A child does not become a subordinate orchestrator merely because it owns
a substantial task or because the claim is long-lived.

## Codex orchestration

Use `$orchestrate-work` after selecting a public flow or substantive atomic skill.

Normal shape:

```text
main/root orchestrator
├── logical claim frame A
│   ├── researcher programme(s), when useful
│   ├── one candidate writer / builder programme
│   └── reviewer programme(s)
├── logical claim frame B
│   └── remote wait; no live agent required
└── logical claim frame C
    └── bounded evidence work
```

`$deliver-goal` manages the goal and claim set. `$deliver-pr` focuses the root on one
claim frame and carries that claim through the named SDLC flows. The root may switch
between claim frames when one reaches a genuine GitHub-owned wait; it does not need one
orchestrator process per frame.

For bounded work, name the `$skill`, exact subject, accepted authority and facts,
read/write boundary, falsifiers, sufficient return, stop conditions, and non-goals.
Require a child to consume the named skill when supplied. Do not restate the skill's
method in the brief.

Choose agents when they preserve useful context, compress high-output evidence, change
source/oracle/tool/environment/threat model, reduce elapsed time, improve recovery, or
avoid expensive CI cycles. Stop adding agents when another result cannot change a
decision.

A programme may intentionally span several ordered atomic skills. Do not fork once per
skill when the same subject and artifact context remain load-bearing. Atomic skills
change attention; they do not automatically justify a new identity.

Keep claim frames and wake events in runtime memory only. Reconstruct them from issues,
PRs, reviews, checks, merges, and repository artifacts after replacement. Do not poll
unchanged remote state.

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

## Codex-native PR review

For substantive PRs the native route is:

```text
`$finish-pr`
→ `$address-review-comments` for existing findings
→ `$final-challenge`
→ `$orchestrate-work` for differentiated review lenses
→ cumulative `$review-pr`
→ REVIEW_CURRENT | CHANGES_REQUIRED | NOT_PROVEN |
  BLOCKED_BY_PREREQUISITE | SUPERSEDED_OR_CLOSE
→ only REVIEW_CURRENT enters `$verify-live-ci`
→ INTEGRATION_READY | PR_IN_FLIGHT | MERGE_BLOCKED | NOT_PROVEN
→ `$merge-reconcile`
```

The root owns the cumulative judgment and disposition. Review is not diff reading, green
CI, mergeability, zero threads, bot approval, or a worker verdict. It must
proportionately challenge proof discrimination, production reachability, external
truth, claim honesty, semantic authority/complexity, and risk/rollback.

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

## Repository hygiene and local proof

- use direct PowerShell, Git, GitHub, Cargo, and repository commands for local work;
- preserve the direct command and its relevant output in evidence and handoffs;
- read nearest package-local owner guidance before modifying an owning crate;
- production code must not use `unwrap`, `expect`, `panic!`, `todo!`,
  `unimplemented!`, `abort`, or `dbg!` outside documented narrow exceptions;
- migrating a test's `.expect("…")` onto the `perl-test-must` helpers uses the
  context-preserving `must_with`/`must_some_with`/`must_err_with`; the bare
  `must`/`must_some`/`must_err` are only correct when the call site carried no
  explanation (`cargo xtask ci-hygiene check-must-context` reports the drop);
- never use `git stash` in worktrees; use scoped restore or a WIP commit;
- stage intended paths explicitly;
- use one worktree per genuine concurrent write claim, not per lifecycle pass;
- run focused proof, then affected package proof, then broader proof only when risk or
  the merge gate selects it;
- do not run repository-wide Clippy or tests after every edit.

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
