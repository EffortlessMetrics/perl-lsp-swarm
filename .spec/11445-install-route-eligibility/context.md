# Spec packet: #11445 - install-route eligibility

Train slot: R03C-D00 - eligibility classifier. Compiled by #11164 from the reviewed issue body; this packet is the durable builder contract for the node. Hard predecessors and current-tree state live in the checked graph (`.spec/10712-distribution-train-graph/`), never here.

## One proposition

One pure deterministic function: exact install-route row -> eligibility class + first limiting predicate + bounded next action.

## Conflict key

`distribution.route_decision.eligibility` - one writer at a time.

## Canonical inputs and authority

#11443 route rows only.

## Owned outputs and contract identity

eligibility classes (primary_easy ... not_proven/failed) with the stable first limiting predicate in canonical policy order, independent of field order.

## Invalid / unrepresentable states

scoring or weights; stale channel called easy; server-only called full product; new-session requirement omitted; manual PATH called zero-action; candidate evidence called public; checksum called provenance; missing rollback/upgrade cells ignored; two field orders producing different reasons.

## Implementation latitude and forbidden work

Latitude: classification and explanation only.

Forbidden: cross-route comparison, preferred selection, docs, producer or public mutation.

## Stop / transfer conditions

stop before comparing alternatives; D01 owns selection.

## Handoff

#11447 consumes classifications.

## Claim / evidence ceiling

classification never strengthens the underlying route row.
