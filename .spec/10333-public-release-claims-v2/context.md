# Spec packet: #10333 - public_release_claims.v2 install-route rows

Train slot: R03A - route-aware claim contract. Compiled by #11164 from the reviewed issue body; this packet is the durable builder contract for the node. Hard predecessors and current-tree state live in the checked graph (`.spec/10712-distribution-train-graph/`), never here.

## One proposition

Introduce public_release_claims.v2 as the sole current route-aware claim contract: one schema, one typed Rust model/validator, and an explicit historical disposition for v1.

## Conflict key

`distribution.public_release_claims.schema` - one writer at a time.

## Canonical inputs and authority

#8087 (controller), #10712/#10726 (train/currentness), #9103/#10825 and #9104-#9109 (source surfaces), #6067/#6855 (topology/product), #6355 (historical v1 validator).

## Owned outputs and contract identity

schemas/public_release_claims.v2.schema.json plus the typed public_release_claims model/validator in xtask; product units (core_server/debug_preview/archive_pair/editor_package/advanced_server_only) and evidence stages (declared/candidate_built/candidate_installed/publicly_published/publicly_verified) stay independent.

## Invalid / unrepresentable states

server-only-as-pair; native-Windows-as-WSL; emulated-x64-as-native-ARM64; declared-as-public; command/route contradiction; duplicate or unstable IDs; stale/cross-SHA represented as current; missing evidence represented as pass; v1 satisfying a current install claim; npm as an active v0.18 route; private paths, raw environment, or error payloads in any field.

## Implementation latitude and forbidden work

Latitude: model, schema, validator, compatibility helpers, and focused fixtures only; exact receipts consumed, never inferred from issue state or moving SHAs.

Forbidden: route population, ranking, generated docs, installer/package behavior, release/public mutation, a second active Python validator.

## Stop / transfer conditions

stop and return to #11164 when a durable evidence-join semantic appears; transfer producer defects to their owning issues.

## Handoff

#11432 consumes the route types without redefining them.

## Claim / evidence ceiling

contract only; no route evidence, verdicts, or public claims.
