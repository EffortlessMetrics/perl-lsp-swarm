# Skill contract

Skills are provider-native, self-navigating artifact transformations. They make the
next useful judgment and route clear without turning the repository into a runtime
workflow engine, scheduler, or cross-lane state store.

## Provider-native authority

```text
Claude Code operation
→ CLAUDE.md
→ .claude/skills/*

Codex operation
→ AGENTS.md
→ .agents/skills/*
```

Shared docs define invariants and parity expectations. A running provider must not need
to reconstruct its procedure from a provider-neutral manual.

## Hierarchical execution scopes

Skills use scope-relative roots:

```text
campaign root
→ governs a durable multi-claim goal

lane root
→ governs one acceptance-and-rollback claim through deliver-pr

worker / writer / reviewer
→ executes one bounded atomic skill or question
```

Campaign-root leaf execution is exceptional. Lane-root direct execution remains valid
for tiny claim-local work. A whole-flow `deliver-pr` delegate is a lane root and may
invoke `orchestrate-work` within its claim. A leaf child may recursively delegate only
when its brief explicitly grants claim-local orchestration authority.

One writer mutates each current candidate. Runtime agent topology, frontier, tasks,
liveness, retries, and wake events remain ephemeral.

## Public and atomic vocabulary

Public flows:

```text
deliver-goal
deliver-pr
prepare-issue
prepare-proof
build-candidate
finish-pr
```

Atomic transformations:

```text
# Issue and plan
find-or-create-issue
research-issue
review-issue
issue-to-plan
research-plan
review-plan
compile-spec

# Proof, build, and hardening
spec-to-test
review-tests
build-from-proof
improve-test-suite
simplify-candidate
review-candidate

# Publication, review, and integration
publish-pr
address-review-comments
final-challenge
review-pr
verify-live-ci
merge-reconcile
```

`orchestrate-work` is an internal root operation. It compiles an ephemeral runtime
route for the selected public flow or atomic skill and returns to the invoking flow. It
is not a public stage or durable graph node.

## Required substantive-skill affordances

Every substantive public flow or atomic skill should make these operational facts easy
to find, proportionately:

```text
Purpose and applicability
Authoritative inputs
Root-retained decisions
Delegable read-only questions
Mutation owner / one-writer boundary
Useful parallelism and communication needs
Join predicate
Expected graph-delta return
Local proof budget
External wait and wake event
Useful GitHub update boundary
Normal and material backward routes
What this establishes / does not establish
Actual stop conditions
```

This is guidance for the orchestrator, not a machine-readable executor schema. Do not
encode model, provider, agent count, live issue, stage, task, frontier, liveness, or
team topology.

## Route trace

The selected route must be explicit in the active context and child brief:

```text
deliver-goal
→ deliver-pr(#123)
→ orchestrate-work
→ writer: build-candidate
→ reviewer: review-tests
→ lane root: finish-pr
```

Every child brief carries:

- parent route and exact durable subject;
- selected provider-native skill or bounded question;
- established facts and accepted authority;
- read-only, writer, reviewer, or lane-root boundary;
- candidate/worktree and one-writer identity where applicable;
- falsifiers, sufficient return, backward routes, stop conditions, and non-goals.

The orchestrator must run the named route. It must not replace a skill with an ad-hoc
lifecycle recipe.

Route traces remain runtime-local. They are not committed or posted as stage progress.

## Runtime-local frontier

`deliver-goal` may keep a bounded in-context projection:

```text
claim
goal predicate
lane context
durable subject
current judgment
next material action
external wait
wake event
```

Reconstruct it from current GitHub/repository artifacts after replacement or
compaction. Do not create a tracked frontier, active-goal pointer, dashboard, label
projection, or lane-state file.

## Graph-delta returns and joins

Read-only workers return:

```text
subject/basis
conclusion
direct evidence and authority
scope searched
contradiction/uncertainty
what is and is not established
affected claim/proof/authority edge
recommended route
NOT_PROVEN boundary
overflow references
```

Writers add candidate identity, behavior changed, proof run/not run, repaired findings,
limitations, and typed flow result. Reviewers return findings/evidence, not approval.

The campaign or lane root joins evidence rather than votes. Repeated claims from one
source remain one evidence path. Builder self-report remains author evidence.
Contradictions survive until resolved through evidence or an accountable decision.

## GitHub boundary

Write to GitHub only when the result is useful later:

- corrected premise, governing decision, issue synthesis, plan, or dependency;
- material prerequisite, supersession, or actual integration interaction;
- candidate claim/proof/limitation/deviation update;
- localized inline review finding;
- cumulative submitted review or useful clean conclusion;
- evidence-backed disposition;
- named remote wait needed by another operator;
- merge/closeout and residual claim.

Do not write agent assignments, liveness, frontier rows, skill-completion messages,
polling updates, raw transcripts, provisional reasoning, or duplicate unchanged
summaries.

## Candidate and lane rule

```text
one coherent claim
→ one current candidate
→ one branch/worktree
→ one writer mutating that candidate at a time
→ one PR
```

Different claims may overlap files or crates. Coordination becomes mandatory only for
duplicate claims, same-candidate writers, explicit prerequisites, destructive shared
runtime state, actual conflicts, or demonstrated combined-tree interactions.
Behind-only movement requires no action.

## Local route grammar

Use direct callable provider-local skill names:

```text
PLAN_READY → prepare-proof
PROOF_READY → build-candidate
CANDIDATE_READY → finish-pr
MATERIAL_PREMISE_CHANGED → prepare-issue
WEAK_PROOF → review-tests
REVIEW_FINDINGS_OPEN → address-review-comments
REVIEW_REQUIRED → final-challenge, orchestrate-work, review-pr
REVIEW_CURRENT → verify-live-ci
INTEGRATION_READY → merge-reconcile
PR_IN_FLIGHT → return to deliver-goal and wait for the named wake event
BLOCKED / NOT_PROVEN → name the exact dependency, authority, instrument, or evidence gap
```

Do not mix agent identities, labels, guessed commands, and callable skills in one route
vocabulary.

## Review-bearing provider contract

Both provider flows must implement:

```text
finish-pr
→ orchestrate-work
→ final-challenge / review-tests / review-candidate as applicable
→ cumulative review-pr
→ REVIEW_CURRENT
→ verify-live-ci
→ INTEGRATION_READY
→ merge-reconcile
```

Green CI, mergeability, zero threads, bot output, or author self-certification cannot
create `REVIEW_CURRENT`. A pending check may leave review current while integration is
`PR_IN_FLIGHT`.

Review is cumulative and semantic. After repair, refresh affected proof/findings and
materially changed claim, reachability, authority, risk, rollback, compatibility, or
integration dimensions—not every dimension merely because the SHA changed.

## Structural validation

Static validation may check provider skill existence, route targets, semantic coverage,
no-bypass edges, one-writer language, hierarchy/route markers, and absence of retired
state machinery.

It must not select live agents, inspect liveness, persist a frontier, decide which live
claim acts next, judge actual review quality from phrases, or authorize mutation/merge.