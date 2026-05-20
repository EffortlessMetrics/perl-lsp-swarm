# The spec/proposal system, fully explained

The system is a **repo source-of-truth stack**. Its central rule is:

> **Do not make every document do every job.**

Each artifact owns one kind of truth: **why**, **what**, **what decision**, **how**, **what now**, **what proves it**, and **what changed**. The model separates proposals/PRDs, specs, ADRs, implementation plans, active goals, support tiers, policy ledgers, and closeouts rather than letting one giant doc become the roadmap, design, task list, CI policy, and release proof all at once.

The end result is a repo where a human, Codex, Droid, Claude, or CI can answer:

```text
Why are we doing this?
What exact behavior must be true?
What architecture decision did we make?
What PR-sized work comes next?
What is the active lane right now?
What proves the claim?
Which support tier changed?
Which policy ledgers changed?
What happened after merge?
```

That is the whole system.

---

## 1. The stack at a glance

```text
Roadmap
  -> Proposal / PRD
    -> Specs
      -> ADRs where needed
        -> Implementation plan
          -> Active goal manifest
            -> Issues / PRs
              -> Proof commands
              -> CI lanes
              -> support-tier updates
              -> policy receipts
                -> Closeout / handoff
```

Each layer narrows the previous one:

- **Roadmap**: direction.
- **Proposal**: why the initiative should exist.
- **Spec**: behavior contract.
- **ADR**: architecture decision.
- **Implementation plan**: PR sequence.
- **Active goal manifest**: what Codex is executing now.
- **Support-tier map**: what users may believe.
- **Policy ledger**: exceptions, lanes, lint rules, file allowances, package classification, receipts.
- **Closeout**: what actually happened.

---

## 2. Why the system exists

The point is **repo-operational memory**.

Without this system, agents and humans rely on stale chat context, old PR descriptions, ambiguous README claims, hidden CI costs, broad TODO lists, and hallucinated commands or policies.

With this system, the repo itself provides the execution path:

```text
.codex/goals/active.toml
  -> linked implementation plan
    -> linked spec
      -> linked proposal
        -> linked support-tier and policy proof
```

---

## 3. Artifact types

### 3.1 Roadmap

**Owns**: release direction, milestone themes, high-level sequencing.

**Does not own**: acceptance tests, PR order, detailed implementation tasks.

Typical locations:

```text
ROADMAP.md
docs/roadmap.md
```

### 3.2 Proposal / PRD

**Owns**: why the work exists.

Typical location:

```text
docs/proposals/
```

A proposal covers the problem, users, success criteria, alternatives, risks, and expected evidence. It should not own detailed PR checklists.

### 3.3 Spec

**Owns**: what behavior must be true.

Typical location:

```text
docs/specs/
```

Specs define behavior, non-goals, evidence, test mapping, implementation mapping, CI proof, promotion rules, and failure modes. They are contracts, not queues.

### 3.4 ADR

**Owns**: durable architecture decisions.

Typical location:

```text
docs/adr/
```

Use ADRs for decisions with lasting impact (for example, parse truth authority, publication boundary rules, or proof-family boundaries).

### 3.5 Implementation plan

**Owns**: PR-sized sequencing.

Typical location:

```text
plans/<milestone>/
```

Plans are concrete: exact work item goal, changed files/surfaces, non-goals, proof commands, rollback, and claim boundary.

### 3.6 Active goal manifest

**Owns**: what Codex/agent/operator is actively executing now.

Typical location:

```text
.codex/goals/active.toml
.codex/goals/archive/
```

The key rule is to keep execution state machine-readable and separate from runtime/product state.

### 3.7 Support tiers

**Owns**: product claim -> proof command mapping.

Typical location:

```text
docs/status/SUPPORT_TIERS.md
```

No stable claim without proof mapping.

### 3.8 Policy ledgers

**Own**: exceptions and governance receipts (package boundaries, CI lanes, lint rules, file policies, no-panic exceptions, coverage matrices).

Typical location:

```text
policy/*.toml
ci/**/*.toml
docs/tracking/**/*.toml
```

### 3.9 Closeout / handoff

**Owns**: what actually happened after merge.

Typical locations:

```text
docs/handoffs/
plans/<milestone>/closeout.md
docs/releases/
docs/release/
```

---

## 4. Directory layout

A mature layout:

```text
docs/
  proposals/
  specs/
  adr/
  status/
  handoffs/

plans/
  <milestone>/

.codex/
  goals/

policy/
  *.toml
```

Use stable prefixes such as `PERLLSP-SPEC-0001` so CI and tooling can validate links and IDs.

---

## 5. Linking model

Artifacts should form a graph:

```text
roadmap -> proposal -> spec -> adr/plan -> active goal -> issue -> pr -> proof -> closeout
```

Predictable metadata fields help CI validate graph integrity:

```md
Status:
Owner:
Created:
Milestone:
Linked proposal:
Linked specs:
Linked ADRs:
Linked plan:
Linked issues:
Linked PRs:
Support-tier impact:
Policy impact:
```

---

## 6. Status lifecycle

Recommended statuses:

- Proposal/spec/ADR: `draft`, `proposed`, `accepted`, `implemented`, `superseded`, `rejected`
- Plan item: `ready`, `active`, `blocked`, `done`, `superseded`
- Active goal: `active`, `paused`, `complete`, `archived`

---

## 7. Anti-duplication rule

Keep one source per truth:

- Claim stability: `docs/status/SUPPORT_TIERS.md`
- CI lanes: `policy/ci-lane-whitelist.toml`
- Package classifications: `policy/package-boundary.toml`
- Active execution: `.codex/goals/active.toml`
- PR order: `plans/<milestone>/implementation-plan.md`
- Initiative why: `docs/proposals/*.md`
- Behavior contract: `docs/specs/*.md`
- Durable decision: `docs/adr/*.md`

Do not copy the same fact into five different docs.

---

## 8. Codex operating flow

```text
1. Read .codex/goals/active.toml
2. Pick next ready work item
3. Read linked plan item
4. Read linked spec
5. Read proposal for context
6. Read ADRs if architecture is involved
7. Make one PR-sized change
8. Update support tiers/policies only when claims/policies change
9. Run listed proof commands
10. Update active goal manifest
11. Open/review/merge per policy
12. Add closeout notes when lane completes
```

Codex should verify named commands/lints/crates/workflows before relying on them.

---

## 9. CI checks that enforce the system

Suggested checks:

```text
cargo xtask check-doc-artifacts
cargo xtask check-goals
cargo xtask check-package-boundary
cargo xtask check-ci-lanes
cargo xtask check-support-tiers
cargo xtask policy-report
```

The purpose is to turn documentation into executable repo infrastructure.

---

## 10. PR structure

PRs should identify layer and boundary:

- Summary
- Links (proposal/spec/ADR/plan/issue)
- Scope and non-goals
- Support-tier impact
- Policy impact
- Proof commands
- Claim boundary
- Rollback

Claim boundary prevents narrow proof from being interpreted as broad product guarantees.

---

## 11. Key principles

1. One artifact, one kind of truth.
2. Specs are contracts, not queues.
3. Plans are PR-sized and executable.
4. Claims must be proof-mapped.
5. Exceptions belong in ledgers with owner/reason/review posture.
6. Agent state must be machine-readable.
7. Do not encode fake repo rules.
8. Verify specifics before acting.

---

## 12. Minimal rollout order

A practical rollout:

1. Define the model docs and templates.
2. Add `policy/doc-artifacts.toml`.
3. Add `cargo xtask check-doc-artifacts`.
4. Add `.codex/goals/active.toml`.
5. Add `cargo xtask check-goals`.
6. Add first proposal.
7. Add first spec.
8. Add support tiers.
9. Add policy ledgers.
10. Wire CI checks (start advisory, then promote).

---

## 13. Shortest mental model

```text
Proposal = why
Spec = what
ADR = durable decision
Plan = how
Active goal = what now
Support tiers = what users may believe
Policy ledgers = exceptions and proof obligations
CI = what proved it
Closeout = what happened
```

The stack works when layers are linked, validated, and non-duplicative.
