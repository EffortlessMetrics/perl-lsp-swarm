# #10413 — Semantic close containment checklist

## Construction

- [x] Trusted-base standalone validator exists under `xtask`.
- [x] The standalone validator is safe for trusted-base invocation; it does not check out or execute candidate code.
- [x] Terminal relation parsing ignores fenced examples and blockquotes.
- [x] Stable PR sections are parsed separately from arbitrary prose.
- [x] Event, body, section, relation, source-line, and API-output bounds are explicit.
- [x] No-terminal-relation path performs no issue lookup.
- [x] GitHub issue lookup uses typed repository/issue identities and no shell.
- [x] Every relation retains an independent disposition.
- [x] Reports state `semantic_completion_proven = false`.

## Rule controls

- [x] `CP00-PHASE-TERMINAL` has invalid and valid phase-leaf fixtures.
- [x] `CP00-EXPLICITLY-NOT-PROVEN` has a focused fixture.
- [x] `CP00-REMAINING-SAME-ISSUE` replays #6239/#5016.
- [x] `CP00-CONTROLLER-PACKET-MISSING` has packet-missing and packet-present fixtures.
- [x] `CP00-PREDECESSOR-SUCCESSOR-COLLAPSE` replays #5968/#5231.
- [x] `CP00-PROOF-LEVEL-CONTRADICTION` replays #6282/#5901.
- [x] Every rule names a CP03 retirement mapping.

## Negative controls

- [x] Removing a phase contradiction changes the #5023 fixture result. Proven by `mutation_controls_prove_each_rule_owns_its_fixture_disposition`: the intact `invalid-phase-terminal-5023-5001` fixture stays red with every rule enabled, and disabling `CP00-PHASE-TERMINAL` alone — on an isolated Phase-1-only boundary seeded from the #5001 issue subject so no sibling rule can mask the mutation — must produce a clean `PASS_NO_HIGH_CONFIDENCE_CONTRADICTION`.
- [x] Ignoring Remaining work changes the #6239 fixture result. The control disables `CP00-REMAINING-SAME-ISSUE` against `invalid-remaining-same-issue` and requires a clean pass; on the #6239 replay itself the disposition also changes (the partial/slice close line hands the row to `CP00-PHASE-TERMINAL`), so the #6239 result never survives this mutation unchanged.
- [x] Treating predecessor deletion as successor retirement changes the #5968 fixture result. Same control with `CP00-PREDECESSOR-SUCCESSOR-COLLAPSE` disabled against `invalid-predecessor-successor-5968-5231`; a clean pass is required.
- [x] Flattening packaged/presentation exclusion changes the #6282 fixture result. Same control with `CP00-PROOF-LEVEL-CONTRADICTION` disabled on an isolated release-exclusion boundary seeded from the #5901 issue subject; a clean pass is required.
- [x] Treating controller child count or prose as a packet changes the controller fixture result. Same control with `CP00-CONTROLLER-PACKET-MISSING` disabled against `invalid-controller-no-packet` (clean pass required), plus `controller_child_counts_or_prose_are_not_a_semantic_close_packet`, which proves child counts and completion prose outside `Governing contract` are never a packet and that negated packet lines (`not supplied`/`missing`/...) are rejected; packet detection is line-scoped and requires a concrete packet reference.
- [x] Scanning fenced or quoted examples would make the no-terminal fixture fail.
- [x] Executing hostile metadata would create a marker file; the test proves none appears.
- [x] Oversized input fails closed.
- [x] Unknown fixture fields are rejected.

## Verification floor

- [x] `cargo test -p xtask --bin semantic-close-containment --locked`
- [x] `cargo clippy -p xtask --bin semantic-close-containment --locked -- -D warnings`
- [x] `cargo fmt -p xtask -- --check` (workspace-wide `--all` hits a Windows command-length limit; no Rust files changed in the enforcement follow-up)
- [x] `cargo xtask check-file-policy`
- [x] `cargo xtask workflow-trigger-lint` (workflow trigger shape only; it does not independently validate the advisory event inventory under `[[checks]]`)
- [x] `cargo xtask workflow-policy-lint --check-lane-whitelist`
- [x] `git diff --check`
- [x] The base-owned `pull_request_target` shape is statically proven: checkout remains pinned to the event base SHA, the PR head is fetched only as an inert Git object and is never checked out, and evaluation runs from the trusted base. The required-checks event mapping was manually checked against the workflow. Live execution starts after this workflow lands. The workflow remains advisory and has no merge authority; any required-promotion decision stays under #10168.

## Review

Differentiated review record, 2026-08-23, against `main@915daa765` plus this change:

- [x] GitHub relation review challenges closing-keyword parsing and ignored examples. Keyword set matches GitHub's supported terminal keywords (close/fix/resolve inflections, optional colon, `#N`, `owner/repo#N`, issue URL), and fences (including longer-fence and trailing-text closers) plus blockquotes (including lazy continuation) are excluded per the accepted parser ruling. Known bounded false negative: a closing keyword GitHub still honors inside a blockquote stays invisible to CP00; quoted template examples are pervasive in repository PR bodies and CP00 is advisory, so the high-confidence-only tradeoff stands and full relation surface retires to CP03.
- [x] Claim-boundary review challenges false positives and the Phase-1 leaf exception. The phase-leaf exception requires explicit issue-side phase markers (`valid-phase-leaf-2624`), multi-relation rows stay independent through `Controlling issue`/`Claim Boundary` scoping, and word-level triggers (for example `partial` in negated prose) can only produce an advisory failure with an exact `Advances`/`Refs` replacement — no merge authority, no semantic claim.
- [x] Security review treats every metadata field as hostile. The only subprocess is `gh api` with a charset-validated `owner/name` endpoint and a `u64` issue number; no shell, path, or workflow interpolation of PR/issue text; receipts sanitize control characters and cap bytes so candidate text cannot forge log lines; every input bound bails to `INSTRUMENT_FAILURE`/`NOT_PROVEN_GITHUB` (exit 3); the workflow pins checkout to the event base SHA (regex plus `rev-parse` proof), fetches the PR head only as an inert object, sets `persist-credentials: false`, and grants read-only permissions.
- [x] Cost review verifies no-keyword early exit and hard bounds. A PR without a terminal relation exits after the bounded relation scan with zero issue lookups (proven live: 2026-08-23 runs print `no automatic closing relation; issue/domain lookup skipped`), and live evaluation performs at most one cached `gh api` call per unique relation under `MAX_RELATIONS`.
- [x] Retirement review confirms no permanent second semantic policy engine. Every rule carries a tested CP03 retirement mapping, the immutable fixtures remain the canonical replay corpus for CP03/CP04, and CP00 joins only stable PR sections plus exact issue classification lines — no semantic completion inference is admitted.
- [x] Mutation review disables every contradiction independently. `mutation_controls_prove_each_rule_owns_its_fixture_disposition` disables each of the six rules in isolation and requires the disposition to change to a clean pass (isolated boundaries for the phase and proof-level controls so sibling rules cannot mask the mutation), so removing or weakening any single guard turns the strict expected-code comparison red; the packet-strictness control covers the controller prose/child-count loosening, including negated packet lines.

## Retirement

- [ ] CP03/CP04 replay every immutable fixture with equal-or-stronger invalid-close rejection and valid-close acceptance. Blocked on the CP03/CP04 train (#10381–#10384); the handoff stays recorded in the #10413 closeout and the fixtures remain the canonical corpus.
- [ ] Required semantic preflight is current. Owned by the CP01–CP05 train under #10168; not a CP00 close condition (2026-08-22T23:10Z boundary ruling).
- [x] The CP00 workflow is absent from this evaluator/fixture prerequisite; fixtures remain in the canonical corpus.
- [x] The base-owned trusted-base enforcement workflow is verified live before any required-promotion decision is claimed. Verified on real `pull_request_target` events 2026-08-23T02:13Z–08:26Z: fast-path runs print `no automatic closing relation; issue/domain lookup skipped` (for example run 32627901280 on PR #11986), and a terminal-relation run performed the live issue lookup and emitted an independent row (`PASS_NO_HIGH_CONFIDENCE_CONTRADICTION ... #6660 line 32`, run 32625188223 on PR #6766). No required-promotion decision has been made; that review stays under #10168.
