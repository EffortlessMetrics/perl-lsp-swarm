# Spec packet: #11434 - install-route denominator

Train slot: R03B-E01 - route skeleton denominator. Compiled by #11164 from the reviewed issue body; this packet is the durable builder contract for the node. Hard predecessors and current-tree state live in the checked graph (`.spec/10712-distribution-train-graph/`), never here.

## One proposition

Derive the complete deterministic set of install-route skeletons: admitted source surface x platform/environment x architecture/libc/target x product unit x evidence stage, with explicit missing dimensions.

## Conflict key

`distribution.route_denominator` - one writer at a time.

## Canonical inputs and authority

#9103/#10825 discovery, #9104-#9109 dispositions, #6067 topology, #6855 product identity.

## Owned outputs and contract identity

the route skeleton set with stable IDs, explicit missing dimensions, and a denominator digest; decides which route cells exist, not whether mechanisms work.

## Invalid / unrepresentable states

newly discovered active surface omitted; historical fixture emitted as current; native Windows and WSL collapsed; GNU and musl collapsed; native and emulated ARM64 collapsed; Cargo server-only collapsed with archive pair; Marketplace and Open VSX collapsed; repo-local package metadata collapsed with public package state; npm admitted; nondeterministic row order or duplicate stable identity.

## Implementation latitude and forbidden work

Latitude: denominator derivation and explain over canonical inputs.

Forbidden: concrete evidence adapters, route verdicts, ranking, docs, installer, publication, channel mutation.

## Stop / transfer conditions

stop when deciding whether a route works; that belongs to E02-E07.

## Handoff

stable route IDs and denominator digest to adapters #11436-#11441 and closure #11443.

## Claim / evidence ceiling

which cells exist; never their evidence state.
