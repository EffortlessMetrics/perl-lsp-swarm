# Merge Queue Activation Checklist

Queue ruleset `17372329` must stay **disabled** until every item below is true. This is a steward settings action; no repository job enables or disables the queue.

Each precondition bullet carries a stable marker (`<!--precondition:...-->`). `scripts/ci/test_merge_queue_activation_checklist.py` fails if any single bullet is removed, so the guard is structural, not a string search over the prose.

## Current reconstruction owner

As of 2026-11-13, [#19261](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/19261)
is closed unmerged (2026-11-12); retain its source and evidence, but it is
not the activation carrier. [#19326](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/19326)
owns bounded reconstruction on current main, including hosted App control,
complete G proof and qualified source/settings rollback. Its title-producer
prerequisite remains with #18255 and PR #19332. A source repair or this checklist
alone does not establish native acceptance or authorize activation.

## Preconditions

- [ ] <!--precondition:superseded-producer--> The `merge_group` title producer does **not** rely on commit-to-PR association. #19257's algorithm is superseded: a real native RIPR singleton G had zero commit-to-PR associations while its H identified the PR, so that algorithm would reject a valid G. The existing #18255 title owner must remove or replace the path, not add a second one alongside it; #19326 owns its integration into the reconstructed cutover.

- [ ] <!--precondition:ruleset-contexts--> Required contexts are reconciled by an authenticated App-owned aggregate, and this is **not satisfied today**. Read back from the live rulesets API on 2026-11-11: main ruleset `16664791` (active) requires exactly five contexts, all bound to the GitHub Actions App (15368): `Compile All Targets (bit-rot guard)`, `Conflict marker check`, `validate-title`, `Perl LSP Rust Small Result`, `ripr+ New Gap Gate`. Queue ruleset `17372329` is **disabled** and carries no `required_status_checks` rule at all — only `merge_queue` (SQUASH, `max_entries_to_build=1`, `min_entries_to_merge=1`, `max_entries_to_merge=1`, ALLGREEN). `Gate Enforcement Contract` is repository policy (#19258) but is **not** installed on `16664791`. Do not describe the two rulesets as agreeing on required contexts until a settings read-back shows it.

- [ ] <!--precondition:exact-h-receipt--> A complete exact-H admission receipt exists for a real native G.

- [ ] <!--precondition:shared-producer--> The local gate and CI invoke the same required-merge-surface commands, and a missing command or filtered step is `NOT_PROVEN`, not green.

- [ ] <!--precondition:singleton-canary--> The canary is a **singleton G**: exactly one pull request is queued, and the resulting `merge_group` G is read back and confirmed to contain that one PR. `max_entries_to_build=1` limits concurrent builds, not group size, so it does not by itself guarantee a singleton. Raise concurrency only after B/G trust and group composition handle speculation.

## Guard

`scripts/ci/test_merge_queue_activation_checklist.py` asserts this file exists, that the precondition bullets sit under `## Preconditions`, that each carries exactly one stable marker and its load-bearing phrases, and that the queue is described as disabled. It does not enable or disable the queue; the settings change is a steward action. Deleting a bullet, moving one out of `## Preconditions`, or gutting its prose fails the guard; a file that merely contains the key phrases does not pass.

Scope boundary: the guard reads this file only. It does not query the live rulesets, does not verify that the prose below is true, and cannot detect a settings change. Those are steward actions with their own read-back.

## Reverting #19257

Reverting #19257 is a **steward decision**, not an activation step. Steward direction at 2026-11-11T05:16Z: supersede the algorithm, keep `1ec099411` on main, no revert, because live exposure is nil while the queue is disabled and the defect fails closed. If an authorized activation is proposed before the replacement title producer is qualified, the authorized owner must resolve that prerequisite and rollback under #19326; the closed #19261 is not a landing dependency. This historical no-revert decision does not authorize activation or establish that rollback has been qualified.
