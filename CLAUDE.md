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
- subagent briefing and differentiated review questions;
- evidence joins and contradiction resolution;
- repair promotion and writer admission;
- proof scheduling and proof-debt control;
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

## Review breadth, not subagent occupancy

For a large PR queue, bias toward a broad read-only review surface—often about five or
six disjoint reviewers when the runtime and queue support it. This is a default fan-out,
not a topology, quota, role mix, or occupancy target.

Keep only subagents whose next result can change a decision. Do not keep stale handles,
duplicate waits, low-value reviews, or already-completed subagents alive to preserve a
number.

The live subagent set must be deduplicated and current:

- remove completed, closed, cancelled, or `Not found` handles immediately;
- a promoted reviewer remains one lane, not a completed reviewer plus a new writer;
- wait only on the current live set;
- refill capacity only when another independent result is useful;
- do not terminate a bounded review merely to refresh the pool display.

Consume each return as it arrives. Do not wait for the whole batch before promoting,
merging, closing, parking, or recording a blocker.

Use ordinary subagents when independent results return to the parent or lane root. Use
Agent Teams only when lateral communication changes the result. Use Ultracode inside one
coherent claim when tasks become ready dynamically; it is not repository state or a
cross-claim scheduler.

## Scope hierarchy

### Campaign root

Owns goal meaning, acceptance predicates, claim selection, dependencies,
contradictions, runtime-local frontier, joined evidence, proof debt, exceptions, and
goal reconciliation.

The campaign root keeps review broad, mutation bounded, proof moving, and converged
candidates closing. Leaf implementation, first-pass deep review, broad archaeology, raw
logs, repetitive proof, CI diagnosis, and routine cleanup belong in claim-local lanes,
subagents, context forks, or Ultracode workflows.

### Lane root

Owns one coherent acceptance-and-rollback claim. It runs `deliver-pr`, invokes
`orchestrate-work`, keeps at most one concurrent writer on the candidate, joins
claim-local evidence, and returns a typed result.

A lane root may directly perform tiny tightly coupled claim-local work when briefing and
joining cost more than the context saved. That does not make routine lane-root
implementation the default.

### Worker, writer, and reviewer

- read-only subagents answer one bounded question or consume one named skill;
- reviewers return findings, falsifiers, contradictions, uncertainty, and references—not
  approval;
- one writer mutates a selected candidate branch/worktree at a time;
- a child that creates a worktree or process group owns its cleanup after retained work
  is safely published or abandoned.

Read-only work normally requires no worktree. Allocate one only when checkout-local
inspection, local proof, or likely promotion into a writer justifies it.

## Campaign execution

Use `orchestrate-work` after selecting a public flow or substantive atomic skill.

```text
multi-PR campaign
→ dispatch useful disjoint reviews
→ join each result as it arrives
→ promote only bounded, evidence-backed repairs
→ admit focused proof separately from review fan-out
→ merge, close, park, or record a named blocker
→ refill useful review capacity
```

Keep review breadth wider than mutation breadth. Cheap read-only review and source
archaeology may run broadly. Writers and heavy proof are admitted by claim independence
and host capacity, not by a fixed count.

Prefer promoting the reviewer that already understands the claim when its context and
worktree remain suitable. Do not pay a second cold start merely to rename the role.

Default to subagents when they preserve campaign context, compress high-output
evidence, change source/oracle/tool/environment/threat model, reduce elapsed time,
improve recovery, or avoid expensive CI cycles. Stop adding subagents when another
result cannot change a decision.

Do not poll unchanged remote state or wait serially for an entire review batch.

## Proof and convergence control

Review output is not repository progress until useful findings are either disproved,
repaired, or converted into durable blockers. Published repairs are not solid state
until their affected proof and review converge.

Maintain a useful proof path:

- when behavioral repairs need proof and the host permits it, keep one focused proof
  lane active;
- use the smallest command that can falsify the changed seam;
- do not start many heavy Cargo jobs merely because many subagents exist;
- when proof debt accumulates, stop promoting additional writers and keep remaining
  capacity read-only;
- a published repair with missing local proof remains `PR_IN_FLIGHT / NOT_PROVEN` unless
  a known hosted gate directly exercises the seam;
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
→ differentiated `orchestrate-work` lenses
→ cumulative `review-pr`
→ REVIEW_CURRENT | CHANGES_REQUIRED | NOT_PROVEN |
  BLOCKED_BY_PREREQUISITE | SUPERSEDED_OR_CLOSE
→ only REVIEW_CURRENT enters `verify-live-ci`
→ INTEGRATION_READY | PR_IN_FLIGHT | MERGE_BLOCKED | NOT_PROVEN
→ `merge-reconcile`
```

Review is not diff reading, green CI, mergeability, zero threads, bot approval, or one
subagent verdict. It must proportionately challenge proof discrimination, production
reachability, external truth, claim honesty, semantic authority, compatibility, risk,
and rollback.

The construction context must not be the only detection surface supporting a
substantive merge. Independence comes from changed evidence, oracle, method, threat
model, environment, or attention—not identity alone.

A clean review is valid. Do not manufacture findings or edits to demonstrate that
review happened.

## Worktrees, Git, and currentness

A worktree and branch are the writer's operational context, not an exact-head lease.
Do not repeatedly reauthenticate an unchanged lane with `ls-remote`, PR metadata, or
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

Keep subagent identity, topology, liveness, retries, routine handoffs, raw logs,
unchanged polling, and temporary task state runtime-local.

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
- use one worktree per genuine concurrent write claim, not per lifecycle pass or
  read-only review by default;
- the child that creates a worktree cleans it after retained work is safely published
  and no further local proof is needed;
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