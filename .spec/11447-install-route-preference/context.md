# Spec packet: #11447 - preferred install routes

Train slot: R03C-D01 - preferred-route selector. Compiled by #11164 from the reviewed issue body; this packet is the durable builder contract for the node. Hard predecessors and current-tree state live in the checked graph (`.spec/10712-distribution-train-graph/`), never here.

## One proposition

Select at most one preferred ordinary-user route per exact context (platform x environment x architecture/libc x projection context), with explicit deciding criteria and truthful no-route/ambiguity results.

## Conflict key

`distribution.route_decision.preference` - one writer at a time.

## Canonical inputs and authority

#11445 classifications only.

## Owned outputs and contract identity

selected(route, criterion, rejected alternatives) | no_eligible_route(blockers) | ambiguous_preference(ties).

## Invalid / unrepresentable states

lexical-ID tie breaking; input-order dependence; server-only over full product; stale package over current archive; candidate over public; manual-PATH route over an equally complete zero-action route without policy reason; one route selected for two execution environments; forced selection when nothing is eligible.

## Implementation latitude and forbidden work

Latitude: explicit deterministic policy over eligible classes.

Forbidden: new normative preference invented to force a winner; classification, evidence, docs, installer changes.

## Stop / transfer conditions

expose ambiguity instead of inventing policy.

## Handoff

#11449 renders from these decisions.

## Claim / evidence ceiling

at most one route per exact cell; explanations evidence-bounded.
