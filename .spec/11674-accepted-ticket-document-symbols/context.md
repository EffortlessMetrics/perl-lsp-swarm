# Context: #11674 - accepted-ticket document-symbol sink

Every parser-triggered local symbol replacement/clear must commit through one
sink-local accepted-ticket boundary, and the committed result identity must be
recorded for #6729's document-symbol row.

On `main@197d45cbb`:

- `run_post_parse_side_effects` wraps `reindex_document_symbols` /
  `clear_document_symbols` in `commit_parse_effect_if_current`, but that checks
  before the callback: symbol extraction and the irreversible
  `symbol_index` replacement/clear happen afterwards with no ticket identity,
  so a superseding edit inside the extraction window can be overwritten by a
  stale N result.
- didOpen (clean and parse-failed branches) and every template/oversize/binary
  guard path mutate the store directly with no identity at all.
- No record of WHICH accepted ticket a committed symbol row belongs to exists,
  so a future result-ID cache (#6729 document-symbol row) would have nothing
  honest to key on.

The sink (`document_symbols_sink.rs`) mirrors #12031's push-diagnostics
boundary: validate `(document_instance, generation)` currency under the sink
lock, compare/record a monotonic per-URI committed ticket sequence, and mutate
the complete local row atomically inside the section. Candidate classes kept
distinct: exact replacement (possibly empty), parse-derived clear, guarded
no-parse clear, and lifecycle eviction.

Scope notes:

- didClose eviction (`evict_open_document_session_state`) remains a
  lifecycle-owned raw removal: the document has left the map, so no ticket can
  validate against it. Every parser-triggered path now refuses raw helpers.
- No document-symbol result-ID cache consumer exists on current main (the
  request path computes live); this claim records the accepted identity only.
- #7309 semantic publication and #8619/#8642 workspace indexing are untouched;
  the store remains a name-search index, not a hierarchy authority.

Dependency note: #11672's outcome vocabulary (PR #11989) is still open in
another lane; the claim-local outcome type is shaped for retargeting.

Base: `main@197d45cbb`.
