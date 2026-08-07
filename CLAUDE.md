# Claude repository operating contract

## Product direction

perl-lsp is becoming a compiler-backed Perl toolchain whose parser, compiler facts,
workspace model, LSP, DAP, packaging, and editor behavior remain honest about source,
freshness, confidence, fallback, and dynamic boundaries.

Optimize for real user-visible closure, semantic ownership, deterministic proof, and
maintainable current-main behavior—not local component completion or workflow
compliance.

## Authority

Use the highest applicable current authority:

1. current `origin/main`, live GitHub issues, PRs, reviews, checks, rulesets, and
   actual repository behavior;
2. accepted specifications, ADRs, policies, generated contracts, and independent
   proof;
3. this file, `.claude/skills/`, and the nearest package-local `CLAUDE.md` or
   `AGENTS.md`;
4. shared invariant/reference docs under `docs/agents/`;
5. Claude plans, subagents, Teams/Ultracode state, worktrees, memory, and conversation.

For Claude Code, this file and `.claude/skills/` are operational authority. Shared
documents explain cross-provider invariants; they do not replace the skill the running
thread or subagent must execute.

GitHub owns durable transaction state. Runtime frontier, subagent topology,
assignments, liveness, retries, raw logs, and provisional reasoning stay in the active
contexts. Do not create tracked active-goal, lane, stage, resume, or executor-state
files.

## Public flows

Use the narrowest applicable provider-native flow:

- `deliver-goal` — govern a durable multi-PR outcome or campaign;
- `deliver-pr` — carry one coherent acceptance-and-rollback claim;
- `prepare-issue` — settle problem, owner, scope, plan, and proof seam;
- `prepare-proof` — establish discriminating executable proof;
- `build-candidate` — implement, harden, simplify, and challenge one candidate;
- `finish-pr` — publish or resume, repair, review, integrate, merge, and reconcile.

Enter at the earliest absent or stale useful judgment. Existing coherent work enters
midstream. Follow the selected skill's named normal route and material backward routes;
do not invent an ad-hoc lifecycle recipe or run a stage locator between skills.

## Hierarchical Claude execution

For substantive work, the main Claude thread normally orchestrates rather than
becoming a leaf executor.

```text
campaign root
→ owns goal meaning, acceptance predicates, claim selection, cross-lane decisions,
  contradictions, evidence joins, merge judgment, and goal reconciliation

lane root
→ owns one coherent claim through `deliver-pr`
→ may invoke `orchestrate-work` inside that claim
→ keeps one candidate writer and joins claim-local evidence

worker / writer / reviewer context
→ consumes the named atomic skill or bounded question
→ performs disposable investigation, proof, mutation, repair, or review
→ returns compact evidence or candidate deltas
```

Campaign-root leaf execution is exceptional. Use it only when the work is itself
orchestration/judgment, one decisive fact must be inspected directly, or briefing and
joining clearly cost more than the permanent main-thread context pollution. Tiny
claim-local work may remain with a lane root or current writer.

A whole-flow assignment creates a lane root, not merely a builder:

```text
Take issue #123 through `deliver-pr`.
You own only this claim. Use `orchestrate-work` within it as useful, keep one writer,
follow normal and material backward routes, and return a compact typed lane result.
```

A leaf subagent may spawn further work only when its brief explicitly grants
claim-local orchestration authority. It may not select unrelated claims or expand the
parent goal.

Use focused subagents, long-running whole-flow agents, context forks, Ultracode dynamic
workflows, and Agent Teams when they preserve campaign context, compress high-output
evidence, change the source/oracle/tool/environment/threat model, reduce elapsed time,
or avoid avoidable hosted CI cycles. Use Teams only where lateral communication
changes the result; maximal fan-out is not sophistication.

## Route trace

The intended route must remain easy to follow in the active context and in useful
GitHub artifacts.

At dispatch, name the route explicitly:

```text
`deliver-goal`
→ `deliver-pr`(#123)
→ `orchestrate-work`
→ writer: `build-candidate`
→ reviewer: `review-tests`
→ lane root: `finish-pr`
```

Every child brief names:

- parent route and exact durable subject;
- selected public flow, atomic skill, or bounded question;
- established facts and governing authority;
- read-only, writer, reviewer, or lane-root boundary;
- candidate branch/worktree and one-writer identity where applicable;
- realistic falsifiers and sufficient return;
- material backward routes, stop conditions, and non-goals.

The orchestrator must actually invoke and operate that route. A child does not receive
a hand-written replacement lifecycle when repository skills already define the work.

Route traces are runtime-local. Do not commit frontier files or post stage-completion
comments. GitHub becomes part of the trace only when the information is reusable.

## Useful GitHub writes

Write to GitHub at evidence, decision, review, handoff, and closeout boundaries:

- corrected premise, governing decision, current issue synthesis, or plan;
- material prerequisite, supersession, or actual cross-claim interaction;
- candidate claim, proof, limitation, or deviation update;
- localized inline review finding;
- cumulative submitted review or useful clean conclusion;
- evidence-backed finding disposition;
- named remote-owned wait when another operator needs the handoff;
- merge/closure effect and residual claim.

Do not write agent assignments, liveness, runtime frontier rows, skill-completion
announcements, polling updates, raw transcripts, provisional reasoning, or duplicate
comments when the durable conclusion did not change.

One direct issue/PR comment is enough for a material cross-lane fact. Inline findings
belong in review threads. GitHub comments and reviews carry useful discovered
information—not the executor topology.

## Evidence joins

Focused subagents return graph deltas, not approval verdicts:

```text
subject and basis
conclusion
direct evidence and authority
contradiction or uncertainty
what is established
what is not established
affected claim/proof/authority edge
recommended route
stable overflow references
```

Writers also return candidate identity, behavior changed, proof run/not run, repaired
findings, limitations, and the typed flow result.

The campaign or lane root verifies load-bearing evidence, preserves contradictions,
rejects vote counting and unsupported confidence, chooses the next route, and writes
only the useful durable result to GitHub.

## Candidate and concurrency contract

One coherent claim normally has one current candidate and one writer mutating its
branch/worktree at a time. Read-only research, proof, oracle, CI, and review work may
run concurrently.

Different claims may touch the same files or crates. Coordinate only for a duplicate
claim, same-candidate writer collision, explicit prerequisite, destructive shared
runtime state, actual Git conflict, or demonstrated combined-tree interaction.
Behind-only movement requires no action.

## Claude-native PR review

For a substantive PR, the normal Claude route is:

```text
`finish-pr`
→ repair findings through `address-review-comments`
→ `final-challenge`
→ `orchestrate-work` for differentiated review lenses
→ main thread joins evidence and performs cumulative `review-pr`
→ REVIEW_CURRENT | CHANGES_REQUIRED | NOT_PROVEN |
  BLOCKED_BY_PREREQUISITE | SUPERSEDED_OR_CLOSE
→ only REVIEW_CURRENT enters `verify-live-ci`
→ INTEGRATION_READY | PR_IN_FLIGHT | MERGE_BLOCKED | NOT_PROVEN
→ `merge-reconcile`
```

Review is not reading a diff, relaying green CI, posting a head hash, or repeating a
subagent verdict. Where applicable it establishes proof discrimination, production
reachability, external/semantic truth, claim honesty, authority/complexity, and
risk/rollback.

The construction context must not be the only detection surface supporting a
substantive merge. Use focused subagents or fresh contexts when they materially change
the source, oracle, method, environment, threat model, or attention surface.

Green checks, `mergeable: true`, zero threads, bot approval, or author
self-certification cannot create `REVIEW_CURRENT`. Merge requires both a current
substantive judgment and current integration evidence. Use the head SHA only as
compare-and-swap protection at merge time.

When GitHub owns the next transition, leave the coherent candidate there and return
`PR_IN_FLIGHT`; do not poll unchanged state. The campaign root may advance another
claim and revisit only on a material wake event.

## Proof and currentness

Keep candidate, integration-basis, and landed evidence distinct.

- material candidate/claim change → rerun affected proof and review;
- finding repair → verify the finding, proof, and changed seam;
- actual conflict or combined-tree repair → review the affected interaction;
- formatting, editorial cleanup, generated receipt refresh, or stronger tests → no
  full-review restart unless meaning changed;
- unrelated `main` movement → no rebase, empty commit, proof replay, or review churn.

Never weaken a test, ratchet, support claim, or required proof to obtain green status.
Use `NOT_PROVEN` for missing, partial, stale, contradictory, or instrument-failed
evidence.

## Local proof and hygiene

- read the nearest package-local owner guidance before modifying an owning crate;
- production code must not use `unwrap`, `expect`, `panic!`, `todo!`,
  `unimplemented!`, `abort`, or `dbg!` outside documented narrow exceptions;
- never use `git stash` in worktrees; use scoped restore or a WIP commit;
- stage intended paths explicitly;
- use one worktree per genuine concurrent write claim, not per lifecycle pass;
- run focused proof first, affected package proof next, and broader proof only when
  risk or merge policy selects it;
- do not run repository-wide Clippy or tests after every edit;
- shared `.claude/settings.json` remains portable and minimal; personal permissions,
  bypass posture, model routing, and experiments stay in user/local settings.

Useful commands:

```bash
just doctor
cargo fmt -p <package> -- --check
cargo clippy -p <package> --all-targets --locked -- -D warnings
cargo test -p <package> --all-targets --locked
just pr-fast
```

## Hard stops

Stop only for a concrete preventable hazard: same-candidate writer collision,
destructive loss of unsalvaged work, unestablished repository/candidate/authority,
unsafe secret or release publication, structurally invalid durable contract,
unresolved substantive finding, missing/`NOT_PROVEN` substantive review at merge, or
live GitHub protection blocking integration. Otherwise detect, explain, repair, and
continue.