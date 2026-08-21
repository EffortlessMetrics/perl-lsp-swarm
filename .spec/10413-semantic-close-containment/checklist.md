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

- [ ] Removing a phase contradiction changes the #5023 fixture result.
- [ ] Ignoring Remaining work changes the #6239 fixture result.
- [ ] Treating predecessor deletion as successor retirement changes the #5968 fixture result.
- [ ] Flattening packaged/presentation exclusion changes the #6282 fixture result.
- [ ] Treating controller child count or prose as a packet changes the controller fixture result.
- [x] Scanning fenced or quoted examples would make the no-terminal fixture fail.
- [x] Executing hostile metadata would create a marker file; the test proves none appears.
- [x] Oversized input fails closed.
- [x] Unknown fixture fields are rejected.

## Verification floor

- [x] `cargo test -p xtask --bin semantic-close-containment --locked`
- [x] `cargo clippy -p xtask --bin semantic-close-containment --locked -- -D warnings`
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo xtask check-file-policy`
- [ ] `cargo xtask workflow-trigger-lint`
- [ ] `cargo xtask workflow-policy-lint`
- [ ] `git diff --check`
- [ ] Follow-up adds and verifies the base-owned `pull_request_target` workflow, including exact-base execution and exact candidate-head proof.

## Review

- [ ] GitHub relation review challenges closing-keyword parsing and ignored examples.
- [ ] Claim-boundary review challenges false positives and the Phase-1 leaf exception.
- [ ] Security review treats every metadata field as hostile.
- [ ] Cost review verifies no-keyword early exit and hard bounds.
- [ ] Retirement review confirms no permanent second semantic policy engine.
- [ ] Mutation review disables every contradiction independently.

## Retirement

- [ ] CP03/CP04 replay every immutable fixture with equal-or-stronger invalid-close rejection and valid-close acceptance.
- [ ] Required semantic preflight is current.
- [x] The CP00 workflow is absent from this evaluator/fixture prerequisite; fixtures remain in the canonical corpus.
- [ ] The separate trusted `pull_request_target` enforcement follow-up lands and is verified before live enforcement is claimed.
