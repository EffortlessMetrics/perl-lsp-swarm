# Spec packet: #11458 - source install-claim parity

Train slot: R03E-P01 - source-context claim parity. Compiled by #11164 from the reviewed issue body; this packet is the durable builder contract for the node. Hard predecessors and current-tree state live in the checked graph (`.spec/10712-distribution-train-graph/`), never here.

## One proposition

Validate every active source-context install claim against its exact route and decision; a claim may not exceed any route dimension.

## Conflict key

`distribution.install_claims.source_parity` - one writer at a time.

## Canonical inputs and authority

#11456 inventory, #11443 rows, #11447 decisions, generated guidance #11449-#11454.

## Owned outputs and contract identity

source-tree parity verdicts; generated blocks validate by identity/digest, curated blocks by explicit bounded annotation.

## Invalid / unrepresentable states

full-product wording on server-only; all-platform wording from one target; native ARM64 from emulation; easy/zero-step with PATH/session action; current package from recipe/PR only; public wording from candidate; upgrade/rollback from clean install only; checksum wording as provenance; curated claim without route/claim identity.

## Implementation latitude and forbidden work

Latitude: source-context validation with exact exceeded-dimension reporting.

Forbidden: publication-tree validation, copy rewrite, literal cutover, producer repair.

## Stop / transfer conditions

stop before projected-public validation or fixing producer facts.

## Handoff

#11461 runs publication-context parity.

## Claim / evidence ceiling

source-tree parity only.
