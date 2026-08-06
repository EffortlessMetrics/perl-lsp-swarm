# Skill contract

Skills are small, self-navigating artifact transformations. They make the next useful
judgment clear without turning the repository into a runtime workflow engine or
cross-lane coordinator.

## Required shape

Every public flow and substantive atomic skill should state:

```text
Purpose
Use when
Do not use when
Authoritative inputs
Focused questions
Recommended procedure
Optional within-claim lenses or delegation
GitHub and repository inputs
Durable updates
What this establishes
What this does not establish
Valid exits and routes
Actual stop conditions
```

## Canonical skill vocabulary

The public flows are:

```text
deliver-goal
deliver-pr
prepare-issue
prepare-proof
build-candidate
finish-pr
```

The initial atomic transformation catalog is:

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

Public flows are the natural user/root entrypoints. Atomic skills are normally called
from a public flow or invoked explicitly for midstream work. Adding, renaming, or
removing an atomic skill is a control-plane change and must update both provider
implementations and route validation.

The internal `orchestrate-work` operation is root-facing execution guidance, not a
public flow or atomic artifact stage. It selects direct execution, a bounded question,
an atomic-skill assignment, a whole-flow lane, or separate independent claim lanes
for the current runtime. It must preserve one writer per candidate, join evidence
rather than votes, and return to the invoking flow. It must not encode a durable
executor graph, lease, reservation, liveness state, provider, model, agent count, or
team topology.

## Local route grammar

Use direct callable skill names from the canonical vocabulary in route sections.

```text
PLAN_READY
  → prepare-proof

PROOF_READY
  → build-candidate

CANDIDATE_READY
  → finish-pr

MATERIAL_PREMISE_CHANGED
  → prepare-issue

WEAK_PROOF
  → review-tests

REVIEW_FINDINGS_OPEN
  → address-review-comments

REVIEW_REQUIRED
  → final-challenge, then review-pr

READY_FOR_INTEGRATION
  → verify-live-ci

PR_IN_FLIGHT
  → return to deliver-goal so another distinct claim may proceed

ALREADY_SATISFIED
  → return to deliver-pr for reconciliation

NOT_APPLICABLE
  → return to the public flow and select the next applicable skill

BLOCKED / NOT_PROVEN
  → name the exact dependency, authority, instrument, or evidence gap
```

Do not mix stage identifiers, agent identities, label names, guessed command names,
and callable skills in one exit vocabulary.

## Applicability

The normal path runs every applicable pass. A pass is not applicable only when:

- its subject genuinely does not exist;
- current evidence already establishes the same judgment;
- the change is proportionally mechanical and has no corresponding decision;
- the flow entered after that judgment and replay would add no value.

A missed earlier pass causes forward repair, not retrospective punishment.

## Candidate and lane rule

A substantive skill operates on one selected claim and its current candidate.

- one claim normally has one current candidate;
- one writer mutates that candidate branch/worktree at a time;
- focused read-only research, oracle, proof, or review may assist when useful;
- helpers do not inspect sibling lanes, touched-file overlap, or nearby symbols as a
  routine ownership check;
- before creating a candidate, check only for an equivalent current PR and explicit
  prerequisites;
- use direct issue or PR comments for material prerequisite, ruling, supersession, or
  actual integration findings;
- the affected lane owns its own conflict resolution and affected re-proof/re-review.

Do not add orchestration metadata, executor DAGs, lane reservations, candidate
frontiers, or persistent liveness state to skills.

A skill may contain a concise within-claim execution note when it materially
clarifies:

- which questions are read-only and independently answerable;
- which candidate branch/worktree receives mutations;
- which evidence or decision must be joined before continuing.

That note must not require a provider, model, agent count, team topology, workflow
engine, or cross-lane surveillance.

## GitHub interaction section

State which native GitHub surfaces the skill reads and may update. Also state which
surfaces must not be treated as authority.

A skill may use labels to classify area, kind, risk, release, or requested attention.
It must not use lifecycle-mirror or agent-completion labels as proof that work
succeeded.

When another lane needs a material fact, a direct issue or PR comment is sufficient.
Do not create another coordination database.

## Review-bearing skill contract

Substantive PR convergence follows
[`PR_REVIEW_STANDARD.md`](PR_REVIEW_STANDARD.md) and
[`REVIEW_CURRENTNESS.md`](REVIEW_CURRENTNESS.md).

- `review-pr` reconstructs the candidate/evidence map, performs directed falsifying
  judgment, publishes useful findings or a useful clean conclusion, and returns an
  explicit merge posture;
- `finish-pr` routes a substantive candidate without useful current review through
  `final-challenge` and `review-pr` before live integration;
- `verify-live-ci` reads integration facts only after substantive review is
  `READY_FOR_INTEGRATION`; green checks, mergeability, zero threads, or bot approval
  cannot create that posture;
- after accepted repair, affected proof and review dimensions are refreshed without
  restarting an unrelated full review merely because the SHA changed;
- `deliver-goal` may synthesize a bounded related PR set only after each candidate has
  its own review, and only to resolve dependency, contract, limitation propagation,
  and repair/merge order.

Review-bearing skills use the shared posture vocabulary proportionately:

```text
READY_FOR_INTEGRATION
CHANGES_REQUIRED
NOT_PROVEN
IN_FLIGHT
BLOCKED_BY_PREREQUISITE
SUPERSEDED_OR_CLOSE
```

The posture is a useful cumulative judgment, not a lifecycle label, claim digest,
exact-head receipt, automatic approval, or merge authorization independent of live
GitHub policy.

## Structural validation

Maintenance-time validation may check:

- metadata and route targets;
- provider semantic coverage;
- no-proof, midstream, in-flight, repair, and backward routes;
- candidate-local writer wording where a skill mutates artifacts;
- review-bearing routes do not bypass `review-pr` for substantive candidates;
- `verify-live-ci` cannot promote missing review into integration readiness;
- root skill-discovery budget;
- absence of retired active references and orchestration metadata.

It must not inspect live issue or PR stage, judge review sufficiency from phrases,
infer neighbouring-lane overlap, require a named agent, authorize mutation, or run
between ordinary skill transitions.
