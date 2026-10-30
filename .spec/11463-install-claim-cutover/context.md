# Spec packet: #11463 - install-claim cutover

Train slot: R03E-P03 - cutover and CI closure. Compiled by #11164 from the reviewed issue body; this packet is the durable builder contract for the node. Hard predecessors and current-tree state live in the checked graph (`.spec/10712-distribution-train-graph/`), never here.

## One proposition

Cut active install-claim enforcement over to the structured inventory/parity system and disposition every old literal rule one at a time (retained_unique_guard | superseded_by_structured_claim | narrowed_to_protocol_or_product_identity | historical_only).

## Conflict key

`distribution.install_claims.cutover` - one writer at a time.

## Canonical inputs and authority

#11456/#11458/#11461; existing product/protocol identity guard owners stay theirs.

## Owned outputs and contract identity

per-literal dispositions, the cheapest source-context gate in ordinary review, the projected-public parity gate on the candidate/publication path; gate success reports denominator, contexts, coverage, and explicit non-green rows.

## Invalid / unrepresentable states

removal of a unique identity guard (e.g. the install.ps1 | iex incident literal retires only as a superseded duplicate, never as an unguarded path); old and new rule both authoritative; source gate claiming public parity; public gate without an exact projected tree; missing inventory row hidden by pass; historical classification as escape; stale generated block ignored; success message omitting coverage/limitations.

## Implementation latitude and forbidden work

Latitude: wiring the structured gates and dispositioning duplicated literals.

Forbidden: a parallel all-purpose docs gate; weakening unrelated product/protocol guards.

## Stop / transfer conditions

stop when a rule protects a different proposition; retain it with its owner.

## Handoff

controller #10342 rail completion; #6355/#6056/#4348/#4350 consume the packet.

## Claim / evidence ceiling

CI cutover; no release, package, registry, or public mutation.
