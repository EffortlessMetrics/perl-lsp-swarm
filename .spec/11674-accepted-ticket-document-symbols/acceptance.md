# Acceptance: #11674

| Obligation | Evidence |
| --- | --- |
| Every parser-triggered replacement/clear carries the exact accepted ticket | didOpen clean/failed branches, all six guard paths, and `run_post_parse_side_effects` construct `DocumentSymbolIdentity` from the inserted state or ticket |
| Currentness comparison AND complete store mutation are sink-local | `commit_document_symbols`: validate -> compare/record -> mutate inside one critical section |
| Stale/wrong-instance candidates cannot mutate the row | falsifier: instance replaced between extraction and commit is rejected; superseded-generation clear rejected |
| Exact-empty result supersedes prior symbols; parse-failure clear supersedes old exact symbols | preservation tests over handler flows |
| Close/reopen remains distinct instances; eviction still clears | lifecycle eviction unchanged (raw removal, documented exception) |
| Committed identity recorded as the #6729 anchor | monotonic per-URI `(generation, sequence)` ledger + test receipt accessor |
| Sink outcomes use the shared #11672 vocabulary | `commit_document_symbols` returns `ParseEffectCommitOutcomeV1`; claim-local enum removed; mapping documented in the sink module docs and context.md |
| No symbol algorithm, workspace-index, semantic-token, cache-redesign, or parser change | diff limited to the sink, call-site identity plumbing, tests, `.spec` |
