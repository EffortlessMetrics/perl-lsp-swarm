# Context: #8054 — semantically complete portable category fingerprints

## Field-to-producer table (admitted `*Fact` schema)

| Category | Encoded fields | Not encoded (not present on the current fact record) |
| --- | --- | --- |
| anchors | `id`, `file_id`, `span_start_byte`, `span_end_byte`, `scope_id`, `provenance`, `confidence` | completeness / limitations / generation — bound via producer schema + fingerprint schema instead |
| entities | `id`, `kind`, `canonical_name`, `anchor_id`, `scope_id`, `provenance`, `confidence` | completeness / limitations — same schema binding |
| occurrences | `id`, `kind`, `entity_id` (or explicit `None`), `anchor_id`, `scope_id`, `provenance`, `confidence` | completeness / limitations — same schema binding |
| edges | `id`, `kind`, `from_entity_id`, `to_entity_id`, `via_occurrence_id`, `provenance`, `confidence` | relation-specific payload beyond kind/endpoints — none exists on `EdgeFact` today |

`FileId` / synthetic import-anchor ID generation still uses process-local `DefaultHasher`. That is identity debt, not fingerprint authority, and is not migrated here.

## Category-to-materialized-consumer table

| Category fingerprint | Materialized consumers that must rebuild when it changes |
| --- | --- |
| anchors | shard storage; any future span/scope index keyed by `AnchorFact` |
| entities | `ReferenceIndex` name projection; import/export purge keyed on entity names |
| occurrences | `ReferenceIndex` occurrence/unresolved projections |
| edges | `ReferenceIndex` `EdgeKind::References` targets |

`replace_fact_shard_incremental` may skip a category only when that category's versioned fingerprint is present and equal. Missing/legacy/`None` fingerprints rebuild conservatively. Equal `content_hash` alone does not skip.

## Digest law

```text
validate unique stable IDs within each category
→ project each fact to canonical length-delimited / big-endian fields
→ sort by stable ID
→ hash domain + fingerprint schema + producer schema + count + canonical rows
```

Duplicate stable IDs with any payload make the category fingerprint unavailable.
