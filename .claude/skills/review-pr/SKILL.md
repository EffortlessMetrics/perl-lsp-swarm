---
name: review-pr
description: Run Claude Code's cumulative substantive pull-request review from the accountable main thread, orchestrating useful adversarial lenses and publishing one evidence-backed judgment before live integration.
user-invocable: false
---

# Review PR

Run the substantive review in Claude Code. This skill is the operational review
procedure; do not defer the work to shared doctrine, a bot summary, green CI, or the
fact that the main thread read the diff.

The **main Claude thread owns the cumulative judgment and GitHub review** for the
selected root-held claim frame. Focused subagents, reviewer programmes, and context
forks may gather evidence or challenge one dimension, but they do not authorize merge
and their verdicts are not votes.

## Authoritative inputs

Read the selected PR, controlling issue/current synthesis, accepted claim/non-goals,
governing specification/ADR/policy or competent external authority, cumulative diff,
live production or operational consumers, focused proof/limitations, submitted reviews
and inline threads, and current GitHub integration facts.

Use the PR head to identify the candidate currently visible on GitHub. It is not a
review-validity token. Do not compute a claim digest, run review-start/review-done
receipt machinery, or post a status-only exact-head comment.

## Review orchestration

### Main-thread decisions

The main thread retains review scope and current/stale dimensions, evidence credibility
and contradictions, finding severity/disposition, whether the claim is supported or
requires repair/`NOT_PROVEN`/prerequisite/closeout, and the one cumulative submitted
review plus next route.

### Useful read-only programmes

Reuse current joined adversarial evidence from `finish-pr` or an earlier invocation
when claim, production path, authority, proof, compatibility, risk, and rollback remain
current. Do not dispatch duplicate review merely because this skill was entered again.

When an applicable lens is absent, stale, contradictory, or materially changed, invoke
`orchestrate-work` only for missing dimensions:

- **claim-vs-code** — decompose every property the PR asserts and verify it against the
  cumulative diff;
- `review-tests` for proof discrimination, historical-defect controls,
  schema/validator agreement, and false-green tests;
- `review-candidate` for implementation correctness, semantic ownership, production
  reachability, complexity, compatibility, risk, and rollback;
- bounded production-path tracing from real request/command/installer/workflow/runtime
  consumer to changed seam;
- bounded external truth against perldoc, protocol/platform documentation, dependency
  API, release topology, or another competent authority;
- focused security, persistence, packaging, migration, performance, or support review.

A useful child brief names the exact PR/candidate, controlling claim, established
facts, authorities, one read-only question, named skill, realistic falsifiers, required
evidence, uncertainty, and non-goals.

Decompose broad claims into individually checkable propositions. Check each proposition
against direct evidence and report it as confirmed, refuted, or `NOT_PROVEN`. Give
review programmes concrete attack hypotheses rather than vague "is this correct?"
prompts.

Differing directions beat additional reviewers. When two lenses examine one surface,
give them different sources, oracles, methods, threat models, environments, or useful
attention surfaces. Repeated same-framing answers are not corroboration.

Use ordinary subagents or context forks for independent returns. Use an Agent Team only
when lateral communication changes the result. Context inheritance is useful context,
not independent evidence by itself.

### Mutation owner and join

The construction context must not be the only detection surface supporting a
substantive merge. Independence comes from changed source, oracle, threat model, method,
environment, or attention—not merely identity.

Join evidence in the main thread rather than counting answers. Resolve contradictions
against source and proof, reject unsupported confidence, and inspect load-bearing seams
before publishing the cumulative judgment. One candidate writer repairs accepted
findings through `address-review-comments`; read-only reviewers do not mutate.

## Required review procedure

1. **Reconstruct the candidate and evidence map.** Establish claim/non-goals,
   controlling authority, cumulative seams, live callers/consumers, proof/limitations,
   prior findings/dispositions, and current GitHub facts.
2. **Trace production reachability.** Show how a real request or operation reaches the
   changed behavior. Component existence is not system proof unless the live route
   consumes it.
3. **Challenge proof discrimination and evidence integrity.** Identify realistic wrong
   implementations the proof rejects; check negative/opposite directions, stale/failure
   behavior, independent oracles, schema/validator agreement, identity binding, and
   whether hosted proof exercised the claim.
4. **Challenge external and semantic truth.** Verify user-visible, language, protocol,
   platform, dependency, and release claims against competent authority and confirm the
   correct semantic owner.
5. **Challenge claim honesty, complexity, risk, and rollback.** Keep title, body, code,
   tests, docs, and generated evidence inside one acceptance-and-rollback claim.
6. **Classify GitHub facts separately.** Checks, threads, draft state, mergeability,
   rulesets, queue state, and prerequisites inform integration but do not create
   substantive review.
7. **Publish the review.** Post file/line-anchored material findings and the cumulative
   conclusion as one submitted review through `scripts/reviews/inline`. Submit as
   `COMMENT`; this repository does not submit `APPROVE`.

A `COMMENTED` review is only a GitHub fact; it does not become substantive merely
because it exists. When the cumulative conclusion is `REVIEW_CURRENT`, generate the
subject-bound marker from the current PR diff:

```bash
python3 scripts/ci/check-pr-semantic-review-currentness.py \
  <pr> <owner/repo> --emit-marker --result REVIEW_CURRENT
```

Append the emitted `semantic-review:v1` marker — an HTML comment bound to the current
PR diff — to the same useful review record. The marker binds semantic currentness without turning the full head SHA into a
ceremonial review receipt.

**The main thread posts the cumulative review.** A bounded review programme returns
file/line-anchored findings as evidence and does not publish an unjoined verdict.

## Durable review record

For a substantive review, preserve the useful durable record shape below. Choose exactly
one outcome heading: a finding-bearing review uses `## Findings`; a clean review uses
`## No material findings`. Never include both `## Findings` and `## No material findings`
in one review record.

```markdown
## Review scope
- Claim, cumulative seams, live consumers, prior findings, and applicable risk reviewed

## Propositions checked
- Hypothesis → confirmed | refuted | NOT_PROVEN, with source/command

## Evidence and falsifiers
- Commands, tests, fixtures, sources, authorities, realistic wrong behavior challenged

## <Findings OR No material findings — replace with exactly one literal heading>
- For Findings: material findings with severity, affected claim, and evidence
- For No material findings: what was challenged and why no material finding remains

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
- Repair, focused re-review, live integration evaluation, closeout, or named follow-up
```

Do not submit only `LGTM`, `review complete`, reviewer identity, head SHA, check
summary, or status line. A clean review is valid when it records what was examined,
what wrong behavior was challenged, and what remains unproved.

## Substantive review results

```text
REVIEW_CURRENT
CHANGES_REQUIRED
NOT_PROVEN
BLOCKED_BY_PREREQUISITE
SUPERSEDED_OR_CLOSE
```

Green checks, `mergeable: true`, zero threads, bot approval, or author self-
certification cannot create `REVIEW_CURRENT`.

## Semantic currentness

- later commit alone does not invalidate review;
- base movement alone is not a finding; a conflict-free candidate behind `main` needs no
  rebase, branch update, CI replay, or review refresh;
- finding repair requires checking that finding, proof, and changed seam;
- material claim, production-route, authority, proof, compatibility, risk, or rollback
  change requires affected review;
- formatting, editorial cleanup, generated receipt refresh, and stronger tests do not
  trigger full review unless a conclusion changes;
- conflict/combined-tree repair receives focused review of the affected interaction.

## Routes

- `REVIEW_CURRENT` → `verify-live-ci`
- `CHANGES_REQUIRED` / `REVIEW_FINDINGS_OPEN` → `address-review-comments`
- weak/non-discriminating proof → `review-tests` or `prepare-proof`
- candidate correctness/reachability/ownership/complexity/rollback uncertainty →
  `review-candidate` through `orchestrate-work`
- `REVIEW_SCOPE_CHANGED` → review affected dimensions; `prepare-issue` only when claim/owner changed
- `BLOCKED_BY_PREREQUISITE` → preserve the exact prerequisite
- `SUPERSEDED_OR_CLOSE` → preserve durable closeout
- `NOT_PROVEN` / `REVIEW_NOT_PROVEN` → resolve missing evidence, authority, or instrument
