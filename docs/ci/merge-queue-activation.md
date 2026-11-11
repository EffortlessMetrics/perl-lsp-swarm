# Merge Queue Activation Checklist

Queue ruleset `17372329` must stay **disabled** until every item below is true. This is a steward settings action; no repository job enables or disables the queue.

## Preconditions

- [ ] The `merge_group` title producer does **not** rely on commit-to-PR association. #19257's algorithm is superseded: a real native RIPR singleton G `71529dab` had zero commit-to-PR associations while its H `16f49ab` identified #14065, so the algorithm would reject a valid G. #19261 must remove or replace that path, not add a second one alongside it.
- [ ] The live main ruleset `16664791` and the queue ruleset `17372329` agree on the required contexts, including `Gate Enforcement Contract`.
- [ ] A complete exact-H admission receipt exists for a real native G.
- [ ] The local gate and CI invoke the same required-merge-surface commands, and a missing command or filtered step is `NOT_PROVEN`, not green.
- [ ] The queue is enabled at `max_entries_to_build=1` for the canary, then raised only after B/G trust and group composition handle speculation.

## Guard

`scripts/ci/test_merge_queue_activation_checklist.py` asserts this file exists and names the superseded-producer precondition. It does not enable or disable the queue; the settings change is a steward action.

If activation work starts before #19261 lands, revert #19257 first.
