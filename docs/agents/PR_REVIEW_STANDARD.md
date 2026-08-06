# Pull request review standard

## Purpose

Every substantive pull request receives a useful cumulative acceptance review before
merge. Review is a directed attempt to falsify the candidate's claim and establish
what the current evidence supports. It is not diff reading, CI relay, a head-hash
receipt, a subagent verdict, or a mechanical restatement of GitHub state.

This standard applies to Claude, Codex, humans, and other review instruments. The
accountable root may use focused reviewers, external oracles, repository tools, or
direct inspection, but the construction context must not be the only detection
surface supporting merge.

A clean review is valid. It still records what was checked, what realistic wrong
behavior was challenged, what evidence or authority was used, and what remains
unproved.

## Review subjects

Review the current cumulative pull request against the highest applicable authority:

- the controlling issue, current synthesis, accepted claim, and explicit non-goals;
- the governing specification, ADR, policy, product direction, support contract, or
  external authority;
- the cumulative candidate, not only the latest commit;
- the real production, protocol, editor, packaging, installer, release, or operational
  path that should reach the changed behavior;
- the focused proof, negative controls, fixtures, generated artifacts, and known
  limitations;
- the semantic owner and downstream consumers;
- applicable compatibility, security, persistence, migration, packaging, support,
  release, and rollback boundaries;
- submitted reviews, inline threads, finding dispositions, required and advisory
  checks, draft state, mergeability, rulesets, and explicit prerequisites.

The PR head identifies the code currently visible on GitHub. It is useful for
candidate identity, check currentness, and expected-head merge safety. It is not a
review-validity token.

## Required procedure

### 1. Reconstruct the candidate and evidence map

Establish:

```text
claim and non-goals
+ controlling authority
+ cumulative changed seams
+ live callers and consumers
+ proof and limitations
+ prior findings and dispositions
+ current GitHub integration facts
```

Do not infer readiness from labels, task completion, reviewer identity, a bot summary,
textual mergeability, or green checks.

### 2. Trace production reachability

Show how a real request or operation reaches the changed behavior. Depending on the
claim, this may be:

```text
client request → dispatcher → provider → semantic owner → response
published command → resolver → selected asset → installed executable
schema/receipt → loader → validator → consuming decision
workflow trigger → selected gate → artifact → merge decision
```

A compiled component, public setter, fixture-only path, or unit-tested adapter is not
system proof unless the live route actually consumes it. If the intended route is not
wired, the review result is `CHANGES_REQUIRED` or `NOT_PROVEN`, not a narrowed claim
that silently treats the component as complete.

### 3. Challenge proof discrimination and evidence integrity

Ask what realistic wrong implementation the proof rejects. Check, where applicable:

- the test fails against the historical or plausible defect for the intended reason;
- positive, negative, stale, failure, recovery, refusal, and opposite-direction cases
  are represented proportionately;
- the oracle is independent rather than copied from the implementation;
- schema and executable validator acceptance agree;
- digests, artifact identities, topology, prior state, and observed results are loaded
  or recomputed from authoritative inputs rather than repeated as self-attested
  booleans or strings;
- generated artifacts are bound to their source and generator;
- hosted/current-source evidence actually ran the relevant tests rather than a zero-
  test, skipped, stale-head, or unrelated path;
- instrument failure and missing evidence remain `NOT_PROVEN`.

Green CI establishes only what the selected checks actually exercised.

### 4. Challenge external and semantic truth

For user-visible, protocol, language, platform, or release behavior, verify the claim
against competent authority such as perldoc, a protocol specification, platform
contract, real dependency API, release topology, or accepted repository owner.

Check that the change extends or delegates to the existing semantic owner instead of
creating a second authority, private schema, duplicate parser, parallel readiness
model, or compatibility path with no bounded retirement plan.

### 5. Challenge claim honesty, complexity, risk, and rollback

Determine whether:

- the PR is one coherent acceptance-and-rollback claim;
- the title, body, docs, comments, and generated evidence stay inside the proof
  boundary;
- an intended rejection, fallback, limited state, or safe refusal cannot hide the
  exact defect the contract says must block;
- partial implementation is named honestly and does not expose empty or panicking
  public capability;
- scaffolding, duplicate authority, compatibility residue, unnecessary API, and
  non-discriminating test machinery have been removed or explicitly bounded;
- compatibility, security, persistence, packaging, migration, support, release, and
  rollback effects are accurate and actionable.

### 6. Classify current GitHub facts

Read live facts separately from substantive judgment:

- required and advisory check results;
- whether a result belongs to the current candidate;
- unresolved threads and current change requests;
- draft purpose and whether it remains valid;
- requested reviewers deliberately still pending;
- mergeability, conflicts, merge queue, and ruleset state;
- explicit prerequisite state.

Classify failures as candidate-owned, base-owned, integration interaction,
test/oracle defect, instrument failure, environment/capacity, pending, or
`NOT_PROVEN`. Do not widen the PR merely to absorb unrelated failures, and do not
ignore a broader current-source failure that directly contradicts the claim.

### 7. Record a merge posture

Use one current substantive conclusion:

| Posture | Meaning |
| --- | --- |
| `READY_FOR_INTEGRATION` | The reviewed claim is supported; no substantive finding remains; live integration facts may now decide merge eligibility. |
| `CHANGES_REQUIRED` | A candidate-owned correctness, reachability, proof, authority, claim, complexity, risk, or rollback defect must be repaired. |
| `NOT_PROVEN` | Missing, partial, contradictory, stale, or instrument-failed evidence prevents a reliable conclusion. |
| `IN_FLIGHT` | The substantive review is sufficient for now, but a named GitHub-owned transition such as current checks or requested review is pending. |
| `BLOCKED_BY_PREREQUISITE` | A specific external claim or contract must become trustworthy or land before this candidate can be accepted. |
| `SUPERSEDED_OR_CLOSE` | The claim is already satisfied, duplicated, invalidated, or deliberately abandoned; preserve the durable disposition. |

`mergeable: true`, green required checks, zero open threads, or a clean bot review do
not independently imply `READY_FOR_INTEGRATION`.

## Useful GitHub review record

Publish material file-specific findings as inline threads and the cumulative judgment
as a submitted review or useful top-level review conclusion.

```markdown
## Review scope
- Claim, changed seams, live consumers, prior findings, and applicable risk reviewed

## Evidence and falsifiers
- Commands, tests, fixtures, sources, or authorities used
- Realistic wrong behavior challenged

## Findings
- Material findings with severity, affected claim, and evidence

<!-- Or: ## No material findings -->

## Prior finding dispositions
- fixed | refuted | superseded | follow-up, with evidence

## What this establishes
- Conclusions supported by the review

## Residual risk / not proved
- Local uncertainty, excluded surfaces, and instrument limitations

## Merge posture
- READY_FOR_INTEGRATION | CHANGES_REQUIRED | NOT_PROVEN | IN_FLIGHT |
  BLOCKED_BY_PREREQUISITE | SUPERSEDED_OR_CLOSE

## Next action
- Repair, focused re-review, current integration proof, merge path, or named follow-up
```

Do not submit only `LGTM`, `review complete`, reviewer identity, a head SHA, a claim
digest, a check summary, or a status line. Do not duplicate a still-current review
merely to make activity visible.

## Related pull requests and trains

A bounded set of related PRs may need a goal-level review synthesis. This is useful
for a release train, evidence fan-in, stacked authority migration, or several child
contracts feeding one parent.

Review each PR individually first. The synthesis then records, for the bounded set:

| PR | Candidate identity | Hosted/current proof | Substantive result | Merge posture | Explicit prerequisite |
| --- | --- | --- | --- | --- | --- |

Also verify:

- each PR remains one coherent claim;
- parent and child contracts agree on schema, identity, authority, and status
  semantics;
- a fan-in loads and validates child evidence rather than accepting copied summaries;
- limitations and `NOT_PROVEN` states propagate instead of disappearing;
- candidate identities are complete enough to prevent mixing evidence from different
  product trees or artifact sets;
- merge order follows real authority and dependency edges;
- a green parent fixture cannot outrun untrustworthy child contracts.

Name the correct repair and merge order. The synthesis is not a batch approval,
portfolio scheduler, cross-lane ownership map, or substitute for each PR's submitted
review.

## Review repair and currentness

Review is cumulative and semantic. Follow
[`REVIEW_CURRENTNESS.md`](REVIEW_CURRENTNESS.md).

After repair:

```text
identify changed semantic subjects
→ rerun affected proof
→ verify addressed findings
→ review newly changed claim/risk dimensions
→ update the cumulative posture
```

Do not restart a full review because the SHA changed. Broaden only when the repair
materially changes the claim, production route, authority, proof, risk, rollback,
compatibility, or integration conclusion. Formatting, editorial cleanup, generated
receipt refresh, and stronger tests normally receive only the focused verification
their meaning requires.

## Proportionality

The same standard applies proportionately:

- a mechanical documentation correction may need a short authority, meaning, link,
  and claim-boundary review;
- production, compiler, protocol, packaging, security, persistence, migration, or
  release work normally requires the applicable full dimensions;
- a clean review may be concise when the claim and evidence are genuinely narrow.

Proportionality reduces irrelevant work. It does not allow CI, diff reading, or author
self-certification to replace review.

## Non-goals

This standard does not create:

- a fixed reviewer persona or mandatory different account;
- a lifecycle label, stage receipt, claim digest, or exact-head review protocol;
- a new review bot or automatic approval gate;
- sibling-lane surveillance, file reservations, or portfolio scheduling;
- merge authorization independent of live rulesets, required checks, mergeability,
  unresolved substantive findings, and expected-head protection.
