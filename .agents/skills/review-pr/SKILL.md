---
name: review-pr
description: Run Codex's cumulative substantive pull-request review in one persistent PR context, using differentiated evidence lenses and routing accepted findings through repair and affected proof without discarding context.
---

# Review PR

Run the substantive review in Codex. This skill is the operational review procedure; do
not defer the judgment to a shared method document, bot summary, green CI, mergeability,
or the fact that one context read the diff.

The cumulative reviewer may be the persistent claim lane or a role-specialized
`pr-reviewer` context. It owns the review judgment for one PR. Focused workers gather
bounded evidence or challenge one dimension; they do not authorize merge, and their
verdicts are not votes.

## Authoritative inputs

Read the selected PR, controlling issue/current synthesis, accepted claim/non-goals,
governing specification/ADR/policy or competent external authority, cumulative diff,
live production or operational consumers, current candidate-bound local result,
focused/affected proof and limitations, submitted reviews/inline threads, and current
GitHub integration facts.

Use the PR head to identify the candidate currently visible on GitHub. It is not a
review-validity token. Do not compute a claim digest, run review-start/review-done
receipt machinery, or post a status-only exact-head comment.

## Context, role, and review skills

Keep the objects separate:

- the **PR context** preserves the candidate map, accumulated evidence, prior findings,
  and worktree;
- the **reviewer role** biases attention toward adversarial judgment and defaults to no
  mutation;
- review skills and lenses provide the current procedure and threat model.

One reviewer may consume `$review-candidate`, `$review-tests`, production-path,
external-oracle, security, compatibility, packaging, persistence, migration, support,
and affected re-review skills in the same loaded context. Do not spawn one reviewer per
angle merely to repeat PR ingestion.

Use another reviewer or worker when a genuinely different source, oracle, method, threat
model, environment, or attention surface can change the decision. Identity alone is not
independence.

## Review orchestration

### Integrating-reviewer decisions

The persistent review context retains:

- review scope and which dimensions are current or stale;
- propositions the PR claim actually asserts;
- which evidence is credible, duplicated, contradictory, or incomplete;
- finding severity and candidate-owned versus prerequisite/follow-up disposition;
- whether the claim is supported, changes are required, evidence is not proven, a
  prerequisite blocks, or the claim is superseded;
- the one cumulative submitted review and next route.

### Reuse before fan-out

First determine whether `$finish-pr` or an earlier invocation already produced current
joined adversarial evidence. Reuse it when claim, production path, authority, local
candidate result, proof, compatibility, risk, and rollback remain current. Do not
dispatch duplicate passes merely because this skill was entered separately.

When an applicable dimension is absent, stale, contradictory, or materially changed,
invoke `$orchestrate-work` only for that missing evidence:

- **claim-vs-code** — decompose the title/body claim into individually checkable
  propositions and verify each against source;
- `$review-tests` — challenge proof discrimination, historical-defect controls,
  schema/validator agreement, and false-green tests;
- `$review-candidate` — challenge implementation correctness, semantic ownership,
  production reachability, complexity, compatibility, risk, rollback, and product
  vision;
- production-path trace — follow a real request/command/installer/workflow/runtime
  consumer to the changed seam;
- external truth — use perldoc, protocol/platform documentation, dependency APIs,
  release topology, or another competent authority;
- focused security, persistence, packaging, migration, performance, or support review.

A useful worker brief names the exact candidate, controlling proposition, established
facts, authorities, one bounded read-only question, named `$skill`, realistic
falsifiers, required evidence, uncertainty, and non-goals.

### Propositions and attack hypotheses

Decompose a chain claim into its links. Report every proposition as confirmed, refuted,
or `NOT_PROVEN`; one false or unproved link prevents the chain claim.

Supply concrete attack hypotheses. “Check whether this is correct” invites agreement;
“can a failed first publish attempt leave the tree dirty for attempts two and three?”
returns either a defect or a reasoned refutation. When the integrating reviewer already
holds a positive read, submit that read for falsification rather than treating it as
self-authenticating.

Differing directions beat additional workers. An external oracle, production-path
trace, and proof-discrimination pass are three directions. Three workers asked the same
vague question against the same source are one correlated read.

## Mutation and context continuity

The construction context must not be the only detection surface supporting a
substantive merge. Independence comes from changed source, oracle, threat model, method,
environment, or attention—not merely identity.

A dedicated reviewer may be promoted in place to repair its accepted bounded finding
when:

- the finding is candidate-owned and supported;
- the repair remains inside the claim/non-goals;
- the parent grants mutation/publication authority;
- no other writer is mutating the candidate.

On promotion, keep the same PR context and worktree, invoke
`$address-review-comments` / `$build-candidate`, commit one coherent candidate, run
`$prove-before-push`, and return through affected `$final-challenge` and `$review-pr`.
Focused child lenses remain read-only unless separately and explicitly promoted.

If the reviewer became the writer, add a genuinely different oracle, method, threat
model, environment, or reviewer before `REVIEW_CURRENT` wherever the mutation made this
context the sole detection surface.

Join evidence rather than counting answers. Resolve contradictions against source and
proof, reject unsupported confidence, and inspect load-bearing seams before publishing
the cumulative judgment.

## Required review procedure

1. **Reconstruct the candidate and evidence map.** Establish claim/non-goals,
   controlling authority, cumulative seams, live callers/consumers, candidate-bound
   local result, proof/limitations, prior findings/dispositions, and current GitHub
   facts.
2. **Decompose and attack the claim.** List substantive propositions and the realistic
   wrong behavior attacked for each.
3. **Trace production reachability.** Show how a real operation reaches the changed
   behavior. Compiled components, setters, adapters, and fixtures are not system proof
   unless the live route consumes them.
4. **Challenge proof discrimination and integrity.** Check negative, stale, failure,
   recovery, refusal, and opposite-direction controls; independent oracles;
   schema/validator agreement; loaded/recomputed identities/hashes; generated-source
   binding; and whether local/hosted proof exercised the claim.
5. **Challenge external and semantic truth.** Verify language, protocol, platform,
   dependency, release, and user-visible claims against competent authority. Confirm
   the correct semantic owner rather than creating a second parser/readiness/schema/
   compatibility authority.
6. **Challenge claim honesty, complexity, vision, risk, and rollback.** Keep title, body,
   code, tests, docs, and generated evidence inside one acceptance-and-rollback claim.
   Do not let fallback, safe refusal, limitation, or partial implementation conceal a
   condition that must block.
7. **Classify GitHub facts separately.** Checks, draft state, threads, requested reviews,
   mergeability, rulesets, queue state, and prerequisites inform integration but do not
   create substantive review.
8. **Publish one review.** Post material file/line findings and one cumulative conclusion
   atomically through:

   ```bash
   scripts/reviews/inline --pr <n> --body <summary> [--findings <file>]
   ```

   Submit as `COMMENT`; this repository does not submit `APPROVE`. Correct an invalid
   diff location and resubmit rather than dropping line anchoring. Findings are
   dispositioned later through `scripts/reviews/disposition`.

   Attribute a failed check before recording it. Confirm the evaluated candidate and
   compare an equivalent gate/failure signature at the merge base before calling it
   base-owned. A stale/cancelled run or generic red `main` is not enough.

The integrating reviewer posts. A bounded lens returns findings as evidence and does not
publish a separate cumulative review.

## Substantive review results

```text
REVIEW_CURRENT
CHANGES_REQUIRED
NOT_PROVEN
BLOCKED_BY_PREREQUISITE
SUPERSEDED_OR_CLOSE
```

- `REVIEW_CURRENT` means the reviewed claim is supported, the current candidate-bound
  local result is honest, and no substantive finding remains. It may enter
  `$verify-live-ci`.
- `CHANGES_REQUIRED` means a candidate-owned correctness, reachability, proof,
  authority, complexity, vision, risk, or rollback defect requires repair.
- `NOT_PROVEN` preserves missing, contradictory, stale, partial, or instrument-failed
  evidence.
- `BLOCKED_BY_PREREQUISITE` names the exact external claim/contract required first.
- `SUPERSEDED_OR_CLOSE` preserves why the claim should not proceed.

Green checks, mergeability, zero threads, bot approval, or author self-certification
cannot create `REVIEW_CURRENT`.

## Useful GitHub review record

```markdown
## Review scope
- Claim, cumulative seams, live consumers, prior findings, and applicable risk reviewed

## Propositions checked
- <proposition>: confirmed | refuted | NOT_PROVEN
  - attack hypothesis
  - source, command, oracle, or authority

## Evidence and falsifiers
- Commands, tests, fixtures, sources, and realistic wrong behavior challenged

## Findings
- Material findings with severity, affected claim, and evidence
<!-- Or: ## No material findings -->

## Prior finding dispositions
- fixed | refuted | superseded | follow-up, with evidence

## What this establishes
- Conclusions supported by the review

## Residual risk / not proved
- Local uncertainty, excluded surfaces, and instrument limitations

## Current GitHub facts
- Checks, threads, draft/ready state, mergeability, and prerequisites as a snapshot

## Substantive review result
- REVIEW_CURRENT | CHANGES_REQUIRED | NOT_PROVEN | BLOCKED_BY_PREREQUISITE |
  SUPERSEDED_OR_CLOSE

## Next action
- Repair in this context, focused re-review, live integration, closeout, or prerequisite
```

Do not submit only `LGTM`, reviewer identity, head SHA, check summary, or a status line.
A clean review is valid when it records what was examined, what wrong behavior was
challenged, and what remains unproved.

Keep worker topology, raw exploration, temporary experiments, duplicated clean reports,
and routine progress runtime-local.

## Semantic currentness

- a later commit alone does not invalidate review;
- a conflict-free candidate behind `main` needs no rebase, branch update, CI replay, or
  review refresh;
- finding repair refreshes that finding, changed seam, local candidate result, and
  affected proof/review dimensions;
- material claim, production-route, authority, proof, compatibility, vision, risk, or
  rollback change requires affected review;
- formatting, editorial cleanup, generated receipt refresh, and stronger tests do not
  trigger full review unless a conclusion changes;
- actual conflict/combined-tree repair receives focused review of the interaction.

Do not restart a full deep review or duplicate a still-current review merely to show
activity.

## Routes

- `REVIEW_CURRENT` → `$verify-live-ci`
- `CHANGES_REQUIRED` / `REVIEW_FINDINGS_OPEN` → same persistent review/claim context
  `$address-review-comments`; use `$build-candidate` for mutation, then
  `$prove-before-push`, affected `$final-challenge`, and affected `$review-pr`
- weak/non-discriminating proof → `$review-tests` or `$prepare-proof`
- candidate correctness/reachability/ownership/complexity/rollback uncertainty →
  `$review-candidate` through `$orchestrate-work`
- `REVIEW_SCOPE_CHANGED` → review affected dimensions; `$prepare-issue` only when claim
  or owner changed
- `BLOCKED_BY_PREREQUISITE` → preserve the exact prerequisite
- `SUPERSEDED_OR_CLOSE` → durable closeout through `$merge-reconcile` when authorized
- `NOT_PROVEN` / `REVIEW_NOT_PROVEN` → resolve missing evidence, authority, or instrument
