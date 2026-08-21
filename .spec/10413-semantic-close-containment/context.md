# #10413 — Semantic close containment context

## Proposition

A pull request may be correct and mergeable while its controlling issue remains open. Until the durable issue-contract and semantic evaluator train is current, the repository needs one narrow trusted-base guard against terminal closing relations that contradict the PR's own stable claim boundary.

This packet governs **CP00 containment only**. A passing result means no supported high-confidence contradiction was found. It never proves semantic issue completion.

## Existing authority

Current `cargo xtask pr-close-proof` proves ancestry and optional content survival. It does not evaluate issue denominator, phase boundary, proof level, controller fan-in, transfer, or explicitly unproved work.

The PR template supplies bounded structured surfaces:

- `## Claim`
- `## Controlling issue`
- `## Governing contract`
- `## Claim Boundary`
- `## Non-goals`
- `## Remaining work`

CP00 joins terminal GitHub relations to those sections and exact issue classifications. It does not scan arbitrary prose or ask a model to infer completeness.

## Trusted execution boundary

The workflow executes the validator from the exact base SHA under `pull_request_target`. Candidate code is never checked out or executed. PR and issue bodies are untrusted data.

Controls:

- read-only `contents`, `pull-requests`, and `issues` permissions;
- no shell interpolation of PR or issue text;
- bounded event, body, section, relation, and API-response sizes;
- no path construction from candidate text;
- no PR, issue, label, review, branch, ruleset, or body mutation;
- GitHub/API/parse failure on a terminal relation is not proven, never pass;
- no-closing-keyword input exits before issue lookup.

## Closed contradiction set

| Rule | Exact containment claim | CP03 retirement owner |
|---|---|---|
| `CP00-PHASE-TERMINAL` | A stable boundary says phase/partial/slice while the broader issue is terminally closed. | Denominator and close-mode evaluation |
| `CP00-EXPLICITLY-NOT-PROVEN` | Stable boundary text explicitly excludes required complete/full work. | Explicitly-not-established row evaluation |
| `CP00-REMAINING-SAME-ISSUE` | Structured remaining work points to the same issue being closed. | Denominator row disposition evaluation |
| `CP00-CONTROLLER-PACKET-MISSING` | A controller/programme is terminally closed without an explicit semantic close packet reference. | Controller fan-in and packet evaluation |
| `CP00-PREDECESSOR-SUCCESSOR-COLLAPSE` | Historical predecessor/deletion evidence is offered as completion of a surviving successor proposition. | Proposition identity and retirement evaluation |
| `CP00-PROOF-LEVEL-CONTRADICTION` | The PR excludes installed/public/packaged/presentation proof required by the issue. | Required proof-level evaluation |

No generic “incomplete” rule exists.

## Immutable evidence corpus

The checked-in fixtures retain bounded structured excerpts plus canonical GitHub URLs and exact candidate SHAs where a historical PR exists. They include:

- PR #5023 / issue #5001 — Phase 1 only;
- PR #6239 / issue #5016 — partial item-2 slice and remaining caller cohort;
- PR #6282 / issue #5901 — presentation and packaged acceptance explicitly excluded;
- PR #5968 / issue #5231 — predecessor crate deletion versus surviving absorbed-engine retirement;
- controller close with and without a packet;
- valid Phase-1 leaf #2624;
- ordinary atomic close;
- no terminal relation;
- multiple relations with only one contradiction.

Fixtures are offline and deterministic. Live GitHub availability is not required for the core suite.

## Non-goals

- No semantic issue-close evaluator.
- No typed issue contract or evidence-admission registry.
- No natural-language completion inference.
- No branch-required promotion decision.
- No automatic PR-body rewrite or issue mutation.
- No permanent duplicate policy after CP03/CP04 become equal-or-stronger.
