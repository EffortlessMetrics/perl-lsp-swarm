# Development method

## Purpose

This repository uses a shift-left, review-forward development loop so capable agents can advance work autonomously without turning process into a permission ladder.

The method is deliberately rigorous about the judgments that improve a change and deliberately light about who performs them.

```text
current artifact
→ focused question
→ evidence-backed improvement or falsification
→ locally named next skill or material backward route
```

The passes are the controls. Permanent personas, lifecycle labels, completion hooks, and tracked stage state are not controls.

## Governing law

**Default-complete, recovery-forward.**

For substantive work, normally perform every applicable research, vision, planning, proof, hardening, simplification, review, and reconciliation pass before creating the next more expensive artifact.

Do not skip an applicable pass merely to move faster. The early pass exists because it is cheaper to repair a wrong premise, owner, slice, oracle, or design before implementation, publication, or merge.

When an earlier pass was missed, perform the cheapest version that can still improve the current artifact and continue. Do not discard coherent work or replay history merely to manufacture process evidence.

## Shift-left versus handcuffs

Shift-left places useful context and challenge immediately before the decision they can improve:

```text
concern
→ issue and source research
→ premise and vision challenge
→ current plan
→ proof design and oracle challenge
→ implementation
→ post-build test hardening and simplification
→ candidate challenge
→ GitHub feedback repair
→ fixed-candidate formal review
→ integration and reconciliation
```

A handcuff notices later that an earlier ritual was not recorded and blocks work after most of the economic value has already been lost.

Missing labels, old stage receipts, a named-agent handoff, or an arbitrary plan age are not reasons to stop. Reconstruct truth from current GitHub and repository artifacts, repair what still matters, and proceed.

## Public flows

The user-facing vocabulary is intentionally small:

| Flow | Outcome |
| --- | --- |
| `deliver-goal` | Advance a durable multi-PR outcome through distinct coherent claims |
| `deliver-pr` | Carry one acceptance-and-rollback claim and its current candidate through reconciliation |
| `prepare-issue` | Research, challenge, vision-check, and plan the concern |
| `prepare-proof` | Turn settled intent into discriminating executable proof |
| `build-candidate` | Implement, harden tests, simplify, and challenge the candidate |
| `finish-pr` | Publish or resume, repair feedback, formally review, merge, and reconcile |

A fresh or resumed session starts with the narrowest applicable public flow. Once inside the loop, each skill names its normal successor and material backward routes. Agents do not run a lifecycle locator between skills.

## Claim and lane independence

The normal multi-PR model is simple:

```text
one coherent claim
→ one current candidate
→ one branch/worktree
→ one writer mutating that candidate at a time
→ one PR
```

Several distinct claims may be active because several ordinary PRs may be active. That does not require a durable frontier, overlap map, file reservation, executor graph, or sibling-lane monitor.

Before creating a candidate, check only whether an equivalent current PR already implements the same claim and whether the issue records an explicit prerequisite. Do not inspect neighbouring worktrees, touched-file overlap, nearby symbols, or another lane's implementation merely to predict coordination.

Each lane owns its own proof, review repair, and integration cleanup:

- if another PR lands and the candidate remains valid, do nothing;
- behind-only movement on `main` requires no action;
- an actual Git conflict is resolved by the affected lane, normally the one landing later;
- an explicit stacked prerequisite is retargeted after its prerequisite lands;
- an actual combined-tree failure is repaired in the smallest affected candidate;
- only conflict- or interaction-affected proof and review are refreshed.

Use a direct issue or PR comment when another lane genuinely needs a material fact: a prerequisite changed, a governing ruling changed, one claim superseded another, or a real integration interaction was found. No additional coordination state is needed.

Do not create several competing candidates for one ordinary claim. Parallel alternatives are justified only when comparison itself is necessary to resolve a material uncertainty; normal delivery resumes with one selected candidate.

When a coherent PR is waiting on CI, review, queue state, or auto-merge, leave it in GitHub and advance another distinct claim when useful. A remote-owned wait is not a goal blocker.

## Durable state and runtime state

Repository and GitHub artifacts hold durable truth:

- issues hold the problem, research trail, corrections, current synthesis, plan, dependencies, and next coherent action;
- specifications, ADRs, policies, and tests hold durable accepted contracts and proof obligations;
- branches and worktrees hold one candidate's mutation;
- pull requests hold one coherent acceptance-and-rollback candidate;
- reviews, threads, checks, rulesets, and mergeability hold current integration evidence;
- merge closeout records what landed, what remains, and what becomes actionable next.

Runtime state remains ephemeral:

- currently active agents and models;
- temporary task lists, liveness, and retries;
- raw logs and provisional reasoning;
- temporary worktree or command bookkeeping;
- provider-native delegation choices inside the selected claim.

Do not mirror runtime topology or liveness into GitHub.

## Issue-first as a paved road

The normal fresh path is:

```text
targeted issue and PR search
→ reuse or reconcile an existing issue, or create a lightweight issue
→ research current source and external authority
→ challenge premise, ownership, scope, and vision
→ maintain one current issue synthesis and plan
→ continue immediately
```

A new issue may begin with only the problem, current evidence, and known context. Research and planning progressively add the current synthesis, scope, proof strategy, dependencies, risk, and next action.

When coherent implementation already exists without an issue, link or create the issue where that improves continuity. Do not pretend retrospective filing shifted the implementation left.

## Two vision checks

Vision alignment is evaluated twice because the two artifacts answer different questions.

### Issue and plan mode

Ask whether the work should exist and whether the proposed shape advances the product:

- Is this a real user or repository problem?
- Is it already solved, duplicated, or superseded?
- Is the semantic owner and consumer correct?
- Is the proposed claim coherent and proportionate?
- Is there a simpler or more direct path?
- Does the work advance the compiler-backed Perl tooling vision?

### Candidate and PR mode

Ask whether the actual implementation still serves that vision:

- Did implementation choices alter product meaning?
- Did the candidate silently broaden or narrow the claim?
- Did it create duplicate authority or an unreachable product path?
- Is the complexity justified?
- Does the user-visible route actually reach the changed behavior?

## Proof and review

Executable proof must discriminate realistic wrong implementations. A green test that mirrors the implementation is not sufficient.

After implementation, revisit proof from the actual candidate:

- what realistic incorrect implementation still passes;
- whether the oracle is independent and non-vacuous;
- whether the opposite direction is represented;
- whether the production seam is exercised;
- whether the proof runs at the cheapest effective layer.

Review has two distinct modes:

1. **Mutable candidate challenge:** fixes are expected; inspect correctness, authority, production reachability, compatibility, security, complexity, and claim honesty.
2. **Fixed-candidate formal review:** bind the judgment to an identified candidate; do not mutate during the judgment; a clean review is valid.

The root may perform a pass directly or delegate focused read-only research, proof, or review inside the selected claim when that changes the evidence surface or reduces elapsed work. Delegate when the evidence-to-answer compression ratio is high: CI or log triage, corpus or repository-wide searches, dependency/API audits, external-source collection, failure bisection, broad inventories, or an independently useful proof adversary. The child returns bounded evidence and references; the warm root keeps decisions, contradictions, and integration. This is an evidence-cost trigger, not a required relay.

### Review standard

Review is **directed, falsifying, and verified**. Aim it at the declared claim and its
real production seam, and try to disprove the claim rather than merely restating the
diff or green checks. Where applicable, establish:

- claim honesty against current source and external authority;
- semantic and external correctness;
- proof discrimination against a realistic wrong implementation;
- production-path reachability;
- negative and fallback behavior;
- compatibility and rollback;
- remaining uncertainty and evidence limits.

A clean review is valid when these questions were considered and no actionable finding
remains. Do not manufacture a finding, edit, or second identity to make review visible.
Material candidate or claim changes require affected proof and a fresh review of the
resulting candidate.

## Hard stops

Stop only where a concrete hazard or unresolved authority remains:

- two writers would mutate the same candidate branch/worktree concurrently;
- destructive cleanup would lose unsalvaged work;
- repository, branch, or candidate identity cannot be established;
- a secret or unsafe release would be published;
- a durable contract is structurally invalid;
- substantive review findings remain unresolved;
- current GitHub branch protection, rulesets, merge queue, or required checks block merge;
- a material product or semantic decision genuinely belongs to the accountable owner.

Everything else normally follows:

```text
detect
→ explain
→ repair
→ continue
```
