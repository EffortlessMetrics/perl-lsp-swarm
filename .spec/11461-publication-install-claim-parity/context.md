# Spec packet: #11461 - publication install-claim parity

Train slot: R03E-P02 - publication-context parity. Compiled by #11164 from the reviewed issue body; this packet is the durable builder contract for the node. Hard predecessors and current-tree state live in the checked graph (`.spec/10712-distribution-train-graph/`), never here.

## One proposition

Run route-claim parity over the exact projected publication tree produced for the candidate, keeping source, candidate, projected, published, and publicly verified stages independent.

## Conflict key

`distribution.install_claims.publication_parity` - one writer at a time.

## Canonical inputs and authority

#11458, #6356 projected-tree identity, #4348/#4350.

## Owned outputs and contract identity

drift reports naming the exact block/route/dimension and source owner against one exact projected-tree SHA/digest.

## Invalid / unrepresentable states

source tree substituted for projected tree; projected tree with stale installer claim; candidate called published; published package called publicly installed; missing generated block; wrong publication SHA; public-context command resolving a source-only route; one registry satisfying another; identical versions masking different bytes.

## Implementation latitude and forbidden work

Latitude: parity over the exact projected tree.

Forbidden: publishing, mutating the public repository, querying mutable latest state, repairing source/public differences, #4351 behavior.

## Stop / transfer conditions

stop before real public verification (#4351).

## Handoff

#11463 cut over; #4350 consumes the result without parsing prose.

## Claim / evidence ceiling

publication-context parity; never public verification.
