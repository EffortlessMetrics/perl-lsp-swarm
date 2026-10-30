# Spec packet: #11443 - install-route closure and fan-in

Train slot: R03B-E07 - cross-cutting route closure. Compiled by #11164 from the reviewed issue body; this packet is the durable builder contract for the node. Hard predecessors and current-tree state live in the checked graph (`.spec/10712-distribution-train-graph/`), never here.

## One proposition

Compose route-family rows with cross-cutting evidence and emit one deterministic, complete public_release_claims.v2 install-route packet: one exact final route row or one explicit non-green result per denominator row.

## Conflict key

`distribution.route_projection.closure` - one writer at a time.

## Canonical inputs and authority

#11434 denominator, adapters #11436-#11441, #7831 channels, #7832 PATH, #5903, #4348/#4350/#4351.

## Owned outputs and contract identity

one complete route packet; PATH, transition, package currentness, and publication stages stay separate fields.

## Invalid / unrepresentable states

current-process PATH as fresh process; clean install as upgrade/rollback/removal; source/candidate as public; public package presence as installed behavior; same version/different SHA joins; missing denominator row; failed sibling disappearing; stale transition reused after installer change; mixed candidate SHAs in one packet; v1 or npm satisfying current closure.

## Implementation latitude and forbidden work

Latitude: composition and closure over exact producer verdicts.

Forbidden: ranking, generated docs, installer/package/workflow behavior, release or public mutation.

## Stop / transfer conditions

transfer every failed producer invariant to its E02-E06 or canonical evidence owner.

## Handoff

#10336 / D00 #11445 classifies rows without reading producer files or prose.

## Claim / evidence ceiling

one terminal projection result per route; never a preference.
