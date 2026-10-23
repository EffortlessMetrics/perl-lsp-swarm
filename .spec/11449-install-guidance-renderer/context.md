# Spec packet: #11449 - guidance renderer and ownership markers

Train slot: R03D-G00 - rendering substrate. Compiled by #11164 from the reviewed issue body; this packet is the durable builder contract for the node. Hard predecessors and current-tree state live in the checked graph (`.spec/10712-distribution-train-graph/`), never here.

## One proposition

One deterministic renderer/ownership substrate consuming exact route decisions plus the complete #11456 claim-surface denominator, writing bounded generated blocks without deciding route truth.

## Conflict key

`distribution.install_guidance.renderer` - one writer at a time.

## Canonical inputs and authority

#10333 model, #11447 decisions, #11456 inventory.

## Owned outputs and contract identity

render/check/explain operations, stable ownership markers, dev/candidate/projected-public/public context support.

## Invalid / unrepresentable states

manual edits inside governed blocks; wrong-context decision packet; v1 catalog used for current guidance; missing/ambiguous preferred route rendered as a command; duplicate/nested/malformed markers; block ID absent from #11456; wrong inventory row; nondeterministic order; stale digests; private paths or raw host errors in output.

## Implementation latitude and forbidden work

Latitude: pure projection over registered rows.

Forbidden: reading producer files, discovering its own denominator, creating new block IDs, selecting or repairing routes, migrating README/guide/release content in this PR.

## Stop / transfer conditions

stop when a rendered claim would need new route truth.

## Handoff

G01 #11451, G02 #11453, G03 #11454 migrate rows atomically with their surfaces.

## Claim / evidence ceiling

renderer proves it can consume registered rows; it owns no surface.
