# Spec packet: #11456 - install-claim surface inventory

Train slot: R03D-G-1 / R03E-P00 - pre-render claim-surface denominator. Compiled by #11164 from the reviewed issue body; this packet is the durable builder contract for the node. Hard predecessors and current-tree state live in the checked graph (`.spec/10712-distribution-train-graph/`), never here.

## One proposition

One deterministic inventory of every active surface that can make a material installation claim, created before broad guidance rendering begins.

## Conflict key

`distribution.install_claims.inventory` - one writer at a time.

## Canonical inputs and authority

#9103/#10825 and #9109 source classifications, #11434 stable route identities.

## Owned outputs and contract identity

the reviewed claim-bearing surface denominator: stable block/claim ID, surface identity, projection context, route or explicit non-route binding, owners, migration state, validation requirement, claim ceiling.

## Invalid / unrepresentable states

historical/deferred/retired material supplying current evidence or becoming the current setup destination; a row exceeding its route's evidence; silently appending a discovered surface inside a docs leaf.

## Implementation latitude and forbidden work

Latitude: inventory rows over README, guides, installer text, editor docs, package metadata, release notes, future marketplace copy, examples, projected tree.

Forbidden: deciding whether a claim is true; renderer migration; parity gates.

## Stop / transfer conditions

a newly discovered material surface returns here before any rendering leaf touches it.

## Handoff

#11449/#11451/#11453/#11454 consume rows; #11458 parity runs over the completed denominator.

## Claim / evidence ceiling

the reviewed denominator; distinct from #9103's implementation-surface inventory.
