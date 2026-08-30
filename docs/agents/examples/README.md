# Worked-lane corpus

Worked lanes are calibration examples drawn from durable repository and GitHub
artifacts. They show proportion, evidence boundaries, and routing decisions. They are
optional just-in-time references and **not runtime authority**: a lane never overrides
the current method contracts in [`../README.md`](../README.md), and a transition that
happened once in one lane is not thereby doctrine.

The corpus is requested by
[#5247](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5247), which names
eight required example categories. This page is the accounting for those categories.

## Why the ledger exists

A corpus that is smaller than its stated scope reads as complete unless something says
otherwise. #5247's research ruling of 2026-08-10
([comment](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5247#issuecomment-5234827911))
named the failure mode directly:

> a PR appearing in the pilot matrix is not by itself sufficient to satisfy one of
> #5247's eight example categories. The example should name the defining transition and
> cite durable evidence that actually demonstrates it. For example, #5717 is strong
> material for **feedback repair / focused re-review**; that does not automatically make
> it evidence for **proof invalidates plan**.

So a category is `COVERED` only when a document *in this directory* narrates the
defining transition on cited evidence. Two things that are not coverage:

- a durable lane exists on GitHub but no curated document narrates it — the material is
  real, the corpus still does not carry the category;
- a nearby pilot demonstrates something adjacent — adjacency is not the transition.

`Source receipts` names only artifacts the lane document itself cites, issue/PR references and
commits alike, because a row may not claim evidence its lane never used.

`ABSENT` rows therefore name no lane, no receipt, and no ruling, and instead record what
a lane would have to demonstrate. Where strong uncurated material is known, the row
points at it so the next slice starts from evidence rather than from a survey.

The ledger consumes terminal rulings; it does not create them.
[#4192](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4192) is closed
`completed` and states that its finite matrix is complete, that future dogfood requires
a new bounded claim, and that the receipt is not an audit queue. Rows cite its rulings
and do not append to it.

[`tests/test_worked_lane_corpus.py`](../../../tests/test_worked_lane_corpus.py) holds
this page to the directory it describes: an unmapped document, a mapped document that
does not exist, a receipt the lane never used, two rows sharing one defining transition,
or an `ABSENT` row that names a lane all fail the check.

That check is structural. It can show that a row cites evidence its own lane uses; it
cannot read the lane and decide whether the narrative actually demonstrates the
transition claimed. A row promoted to `COVERED` with a plausible sentence and a
real receipt will pass. Judging that promotion is review's job, and the ledger exists to
put it in front of a reviewer rather than to settle it.

## Category ledger

### fresh-semantic-change

- **Status:** ABSENT
- **Worked lane:** none
- **Source receipts:** none
- **Terminal ruling:** none
- **Defining transition:** issue capture through research, premise challenge, plan, proof, implementation, hardening, simplification, candidate challenge, and reconciliation, on an ordinary product/compiler/LSP change.
- **What remains unproved:** #5247 explicitly requires this lane to be a product change rather than another control-plane example, and the only curated lane is control-plane. No reconciled product claim in the corpus carries directed-review receipts across the full route.

### existing-pr-midstream

- **Status:** ABSENT
- **Worked lane:** none
- **Source receipts:** none
- **Terminal ruling:** none
- **Defining transition:** a coherent PR that skipped the modern chronology enters at the earliest still-useful missing judgment, without replayed stages, invented red-test history, or rejection for missing receipts.
- **What remains unproved:** durable material exists but is uncurated — the #4179 pilot receipt records a `PROMOTE` for behind-only candidate behavior on the PR #5665 / #5676 lineage, where "main movement did not trigger ceremonial rebase or head mutation when the candidate remained conflict-free". That covers the no-rebase decision; it has not been curated into a lane that shows midstream entry itself.

### docs-or-metadata-no-proof

- **Status:** ABSENT
- **Worked lane:** none
- **Source receipts:** none
- **Terminal ruling:** none
- **Defining transition:** a non-executable claim receives proportionate issue and candidate review, fabricates no red test or build artifact, and publishes and reconciles honestly.
- **What remains unproved:** no curated lane, though candidate material is stronger than a docs PR usually is — #5311 carries dispositioned bot findings and named-lens member reviews. What a lane still has to establish is the proportionate-review half: that the review was independent enough to be review rather than author-side self-disposition, on a claim with nothing to run.

### proof-invalidates-plan

- **Status:** ABSENT
- **Worked lane:** none
- **Source receipts:** none
- **Terminal ruling:** none
- **Defining transition:** a test, oracle, or reproduction shows the proposed owner, behavior, scope, or premise was wrong, and the lane routes backward to `prepare-issue` or plan research before downstream implementation.
- **What remains unproved:** no durable case of a backward route is known. The nearest material, PR #5717's evidence-boundary correction, is in-candidate hardening repaired inside the same claim; it never returned to issue or plan research, so citing it here would be the exact over-attribution the 2026-08-10 ruling forbids.

### feedback-repair-and-focused-rereview

- **Status:** ABSENT
- **Worked lane:** none
- **Source receipts:** none
- **Terminal ruling:** none
- **Defining transition:** a bot or human finding is verified against primary evidence, given an explicit disposition, repaired by the one integrating writer, and answered by rerunning only the affected proof while unchanged areas keep their current evidence.
- **What remains unproved:** the material is strong and the lane document does not carry it. #4192 records a `PROMOTE` for the review-forward synthetic integration proof on PR #5717, whose directed review produced four findings that were verified and repaired before merge. But `integration-trigger-and-proof-caller.md` narrates only that "the later repair pass found a concrete safety gap" and "added focused regression tests" — it never identifies the review as the source, names a disposition, or shows the one-writer boundary. This row was `COVERED` until review caught that the ledger's author had imported the finding from #5717's threads rather than reading it in the lane.

### clean-formal-review

- **Status:** ABSENT
- **Worked lane:** none
- **Source receipts:** none
- **Terminal ruling:** none
- **Defining transition:** a fixed-candidate review closes with no material finding, states why no change was invented to prove the review happened, and states what the review did and did not establish.
- **What remains unproved:** #4192 records a `PROMOTE` for clean formal review on a current claim, carried by PR #5672 and reconciled on issue #5220 (32/32 convergence fixtures), so the ruling exists without a curated lane. A lane would also have to be honest about the shape of the receipt rather than implying a conventional approval.

### ci-instrument-failure

- **Status:** COVERED
- **Worked lane:** `integration-trigger-and-proof-caller.md`
- **Source receipts:** PR [#5717](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5717)
- **Terminal ruling:** [#4192](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4192) — `PROMOTE` for the review-forward synthetic integration proof
- **Defining transition:** a check that produced no verdict — a focused rerun that timed out after 305 seconds without output — is retained as `NOT_PROVEN` instead of being read as a product pass or a product failure, and the merge that followed is not treated as supplying the missing receipt.
- **What remains unproved:** the lane covers a timeout only. It does not show a check that failed to spawn, one that evaluated an older head, or one that lost output, and it does not show disjoint work continuing around an instrument failure.

### multi-pr-goal

- **Status:** ABSENT
- **Worked lane:** none
- **Source receipts:** none
- **Terminal ruling:** none
- **Defining transition:** an umbrella outcome inspects current main and its child graph, selects one coherent claim, delivers it, reconciles the umbrella against what landed, and then either selects the next claim or completes — with no queue, scheduler, or build-all wave.
- **What remains unproved:** the underlying lane is real and reconciled — umbrella #4556 decomposed into #4588/#5713 and #4589/#5717 and closed `completed` with a terminal state naming both squashes — but the curated document narrates only the two claims, never the umbrella, the selection between them, or the reconciliation. The corpus does not carry this category until a lane documents those steps.

## Standing boundaries

Two of #5247's acceptance conditions are not category rows and are not satisfied by any
current lane:

- **Cross-provider coverage.** #5247 asks for at least one Claude-driven and one
  Codex-driven lane. No receipt attributes a driver to any curated lane, and the closed
  [#4179](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4179) pilot receipt
  lists "complete Claude/Codex cross-provider pilot comparison" among its remaining
  `NOT_PROVEN` cases.
- **Lower-cost calibration.** #4179 lists "lower-cost-model calibration" in the same
  set. Both are recorded there as `NOT_PROVEN` rather than as default-method
  prerequisites, and neither is claimed here.

Candidate lanes for future categories are tracked on #11114, #11120, #11126 and #11132;
[#11141](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11141) decides which
of them become reusable and has not posted a terminal ruling. Those decide what may
*enter* the corpus later; they are not a prerequisite for accounting for the lane that
has already landed.

## Adding a lane

Write the document, then add its receipts to the row it covers and flip that row to
`COVERED`. A row moves to `COVERED` only when the named document narrates that row's
defining transition on the receipts cited — not when a PR appears in a matrix, and not
when an adjacent category already points at the same document.

Two mechanics that are easy to get wrong:

- **Cite issues and PRs as `#NNNN`.** A full GitHub link is welcome alongside it, but the
  bare `#NNNN` is what the check looks for, so `PR 5717` or `see pull 5717` fails.
- **`Source receipts` is what the row relies on, not an index of the lane.** The binding
  is one-way: every receipt a row cites must appear in its lane document, but the lane
  may reference more than the row does. `ci-instrument-failure` cites only #5717 while
  its lane also names #4588, #4589 and #5713 — the row is a focused claim about one
  transition, not a bibliography.
