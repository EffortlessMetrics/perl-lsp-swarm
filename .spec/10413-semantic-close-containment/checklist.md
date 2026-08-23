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
- [x] `cargo fmt -p xtask -- --check` (workspace-wide `--all` hits a Windows command-length limit; no Rust files changed in the enforcement follow-up)
- [x] `cargo xtask check-file-policy`
- [x] `cargo xtask workflow-trigger-lint` (workflow trigger shape only; it does not independently validate the advisory event inventory under `[[checks]]`)
- [x] `cargo xtask workflow-policy-lint --check-lane-whitelist`
- [x] `git diff --check`
- [x] The base-owned `pull_request_target` shape is statically proven: checkout remains pinned to the event base SHA, the PR head is fetched only as an inert Git object and is never checked out, and evaluation runs from the trusted base. The required-checks event mapping was manually checked against the workflow. Live execution starts after this workflow lands. The workflow remains advisory and has no merge authority; any required-promotion decision stays under #10168.

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
- [ ] The base-owned trusted-base enforcement workflow is verified live before any required-promotion decision is claimed.
