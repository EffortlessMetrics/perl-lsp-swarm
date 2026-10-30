# Spec packet: #11432 - exact route evidence joins

Train slot: R03B-E00 - evidence-join substrate. Compiled by #11164 from the reviewed issue body; this packet is the durable builder contract for the node. Hard predecessors and current-tree state live in the checked graph (`.spec/10712-distribution-train-graph/`), never here.

## One proposition

One typed, deterministic join substrate: route subject + producer subject + receipt identity -> joined | missing | stale | mismatched | malformed | not_applicable.

## Conflict key

`distribution.route_evidence_join` - one writer at a time.

## Canonical inputs and authority

#10333 (route/product-unit types), #10726 (observation), canonical digest/subject vocabulary.

## Owned outputs and contract identity

join key, bounded evidence-reference vocabulary, failure reasons, privacy rules, and a narrow adapter interface for later route families.

## Invalid / unrepresentable states

same version with different topology digest; same topology with different source/publication SHA; Windows-native evidence reused for WSL; GNU reused for musl; server-only reused for an archive pair; checksum reused as independent provenance; stale/cross-candidate receipt; missing evidence normalized to success; private absolute paths, tokens, proxies, or unbounded raw errors in durable output.

## Implementation latitude and forbidden work

Latitude: join model, normalization, deterministic serialization, explain for one join.

Forbidden: concrete producer adapters, route cell enumeration, populating public_release_claims.v2, classification, documentation.

## Stop / transfer conditions

stop when a field would decide a producer's semantics instead of retaining its verdict.

## Handoff

R03B-E01 #11434 derives route skeletons over this substrate.

## Claim / evidence ceiling

substrate only; no route rows, verdicts, or rankings.
