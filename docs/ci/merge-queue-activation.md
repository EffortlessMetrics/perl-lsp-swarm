# Merge Queue Activation Checklist

Queue ruleset `17372329` must stay **disabled** until every item below is true. This is a steward settings action; no repository job enables or disables the queue.

Each precondition bullet carries a stable marker (`<!--precondition:...-->`). `scripts/ci/test_merge_queue_activation_checklist.py` fails if any single bullet is removed, so the guard is structural, not a string search over the prose.

## Preconditions

- [ ] <!--precondition:superseded-producer--> The `merge_group` title producer does **not** rely on commit-to-PR association. #19257's algorithm is superseded: a real native RIPR singleton G had zero commit-to-PR associations while its H identified the PR, so that algorithm would reject a valid G. #19261 must remove or replace the path, not add a second one alongside it.

- [ ] <!--precondition:ruleset-contexts--> Required contexts are reconciled by an authenticated App-owned aggregate, and this is **not satisfied today**. Read back from the live rulesets API on 2026-11-11: main ruleset `16664791` (active) requires exactly five contexts, all bound to the GitHub Actions App (15368): `Compile All Targets (bit-rot guard)`, `Conflict marker check`, `validate-title`, `Perl LSP Rust Small Result`, `ripr+ New Gap Gate`. Queue ruleset `17372329` is **disabled** and carries no `required_status_checks` rule at all — only `merge_queue` (SQUASH, `max_entries_to_build=1`, `min_entries_to_merge=1`, `max_entries_to_merge=1`, ALLGREEN). `Gate Enforcement Contract` is repository policy (#19258) but is **not** installed on `16664791`. Do not describe the two rulesets as agreeing on required contexts until a settings read-back shows it.

- [ ] <!--precondition:exact-h-receipt--> A complete exact-H admission receipt exists for a real native G.

- [ ] <!--precondition:shared-producer--> The local gate and CI invoke the same required-merge-surface commands, and a missing command or filtered step is `NOT_PROVEN`, not green.

- [ ] <!--precondition:singleton-canary--> The canary is a **singleton G**: exactly one pull request is queued, and the resulting `merge_group` G is read back and confirmed to contain that one PR. `max_entries_to_build=1` limits concurrent builds, not group size, so it does not by itself guarantee a singleton. Raise concurrency only after B/G trust and group composition handle speculation.

## Guard

`scripts/ci/test_merge_queue_activation_checklist.py` asserts this file exists, that every precondition bullet carries exactly one stable marker, and that every documented precondition marker is present. It does not enable or disable the queue; the settings change is a steward action. Deleting a bullet fails the guard; a file that merely contains the key phrases does not pass.

## Reverting #19257

Reverting #19257 is a **steward decision**, not an activation step. Steward direction at 2026-11-11T05:16Z: supersede the algorithm, keep `1ec099411` on main, no revert, because live exposure is nil while the queue is disabled and the defect fails closed. A revert becomes the choice only if an authorized activation begins before #19261 lands.
