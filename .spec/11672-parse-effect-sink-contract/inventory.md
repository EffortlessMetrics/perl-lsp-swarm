# parse_effect_sinks.v1

Checked static inventory generated from `crates/perl-lsp-rs/src/runtime/parse_effect_contract.rs` (#11672).

## `diagnostics.parser-outbound-publication`

- title: Parser diagnostics outbound publication (fast + debounced + syntax-only routes)
- owner: #11673
- ticket inputs: document_instance, generation, client_uri, normalized_uri, snapshot, captured_text, settle_ownership
- sink-local subject: publishDiagnostics stream keyed by client URI; replace-mode per LSP
- store: outbound_publishDiagnostics
- owns mutation sites: yes
- mutation boundary: Outbound::notify("textDocument/publishDiagnostics") admission via publish_parse_errors_fast / publish_diagnostics (debounced target) / syntax-only publisher
- currentness comparison: helper precheck then callback (residual window admitted)
- terminal/clear policy:
  - clean: publish
  - recovered_partial: publish
  - budget_exhausted: publish
  - cancelled: publish
  - catastrophic_minimal: publish
  - guarded_no_parser_state: publish
  - desynchronized: out_of_scope
  - instrument_failure: out_of_scope
- focused proof filter: parse_effect_sink
- composed proof owner: #11676
- compatibility adapter: commit_parse_effect_if_current (exit: #7379)
- disposition: new focused child (#11673)
- claim ceiling: Route inventory + outcome vocabulary only; no publication behavior change here.

## `diagnostics.didopen-guard-admission-publication`

- title: Open/edit guard paths' empty/binary diagnostics publication (template/oversize/binary, didOpen + didChange)
- owner: #11673
- ticket inputs: none (pre-parse admission; no accepted ticket exists by construction until #11668 mints one for guarded opens)
- sink-local subject: publishDiagnostics stream keyed by client URI (empty or binary set)
- store: outbound_publishDiagnostics
- owns mutation sites: no
- mutation boundary: shares the #12031 sink-local diagnostics_sink::commit_push_diagnostics boundary with diagnostics.parser-outbound-publication; its pre-#12031 direct Outbound::notify guard branches retired into that single enqueue
- currentness comparison: same-thread admission before acceptance exists
- terminal/clear policy:
  - clean: publish
  - recovered_partial: publish
  - budget_exhausted: publish
  - cancelled: publish
  - catastrophic_minimal: publish
  - guarded_no_parser_state: publish
  - desynchronized: out_of_scope
  - instrument_failure: out_of_scope
- focused proof filter: parse_effect_sink
- composed proof owner: #11676
- compatibility adapter: none
- disposition: new focused child (#11673)
- claim ceiling: Inventory + ledger registration only; admission route unchanged in this PR. Mutation-site ownership is reported through the shared diagnostics_sink registration until #11673 gives this row its own focused commit law.

## `document-symbols.replace-or-clear`

- title: Document-symbol replacement/clear (reindex after accepted parse, clear on failure/close/guard)
- owner: #11674
- ticket inputs: document_instance, generation, client_uri, normalized_uri, snapshot, captured_text, settle_ownership
- sink-local subject: per-URI symbol document inside symbol_index
- store: symbol_index
- owns mutation sites: yes
- mutation boundary: document_symbols_sink replace_document_symbols(uri, symbols) / remove_document(uri) under one lock acquisition (#12035 accepted-symbols boundary; the pre-#12035 text_sync call sites retired with it)
- currentness comparison: helper precheck then callback (residual window admitted)
- terminal/clear policy:
  - clean: replace
  - recovered_partial: replace
  - budget_exhausted: replace
  - cancelled: clear
  - catastrophic_minimal: replace
  - guarded_no_parser_state: clear
  - desynchronized: out_of_scope
  - instrument_failure: out_of_scope
- focused proof filter: parse_effect_sink
- composed proof owner: #11676
- compatibility adapter: commit_parse_effect_if_current (exit: #7379)
- disposition: new focused child (#11674)
- claim ceiling: Route inventory only; symbol store untouched in this PR.

## `workspace-index.live-contribution-replacement`

- title: Workspace-index contribution replacement from live open buffers
- owner: #8619/#8642
- ticket inputs: document_instance, generation, client_uri, normalized_uri, captured_text
- sink-local subject: WorkspaceIndex file/fact entries crossed with typed non-zero SourceCommit
- store: workspace_index
- owns mutation sites: yes
- mutation boundary: external: WorkspaceIndex::index_live_file returning SourceCommitOutcome {Accepted,NoOp,RejectedStale,Failed} -- atomic publication authority stays #8619/#8642; this contract references it and does not reimplement it
- currentness comparison: external owner
- terminal/clear policy:
  - clean: replace
  - recovered_partial: replace
  - budget_exhausted: replace
  - cancelled: clear
  - catastrophic_minimal: replace
  - guarded_no_parser_state: clear
  - desynchronized: out_of_scope
  - instrument_failure: out_of_scope
- focused proof filter: parse_effect_sink
- composed proof owner: #11676
- compatibility adapter: commit_parse_effect_if_current (exit: #7379)
- disposition: existing exact owner (#8619/#8642)
- claim ceiling: Reference existing SourceCommit/SourceCommitOutcome authority; exact accepted-ticket integration lands with the parser-state train.

## `workspace-index.reader-capture-projection`

- title: Workspace-index reader capture/projection (query-time reads of indexed facts)
- owner: #8619/#8642
- ticket inputs: none (read-side projection consumes committed index state, not tickets)
- sink-local subject: read locks over WorkspaceIndex maps
- store: workspace_index
- owns mutation sites: no
- mutation boundary: none (read-only projection over externally owned index)
- currentness comparison: external owner
- terminal/clear policy:
  - clean: out_of_scope
  - recovered_partial: out_of_scope
  - budget_exhausted: out_of_scope
  - cancelled: out_of_scope
  - catastrophic_minimal: out_of_scope
  - guarded_no_parser_state: out_of_scope
  - desynchronized: out_of_scope
  - instrument_failure: out_of_scope
- focused proof filter: parse_effect_sink
- composed proof owner: #8619
- compatibility adapter: none
- disposition: existing exact owner (#8619/#8642)
- claim ceiling: Read-authority reference only; no read path is touched here.

## `semantic-project.contribution-publication`

- title: Semantic/project contribution publication (fact shards, cross-file indexes)
- owner: #7309
- ticket inputs: none (publication authority derives its own candidate identity from committed facts)
- sink-local subject: semantic fact shards and cross-file semantic indexes
- store: workspace_index
- owns mutation sites: no
- mutation boundary: external: semantic fact-shard write-through owned by the #7309 publication seam; no local reimplementation permitted
- currentness comparison: external owner
- terminal/clear policy:
  - clean: out_of_scope
  - recovered_partial: out_of_scope
  - budget_exhausted: out_of_scope
  - cancelled: out_of_scope
  - catastrophic_minimal: out_of_scope
  - guarded_no_parser_state: out_of_scope
  - desynchronized: out_of_scope
  - instrument_failure: out_of_scope
- focused proof filter: parse_effect_sink
- composed proof owner: #7309
- compatibility adapter: none
- disposition: existing exact owner (#7309)
- claim ceiling: Authority reference only.

## `result-id.local-state`

- title: Local document-symbol/semantic-token result-ID cache state (per-kind result IDs)
- owner: #6729
- ticket inputs: client_uri, normalized_uri
- sink-local subject: semantic_tokens_cache entry keyed by normalized URI
- store: semantic_tokens_cache
- owns mutation sites: yes
- mutation boundary: semantic_tokens_cache.lock() insert on provider compute + remove on open-document session eviction (#6729 owns per-kind result-ID identity; parser acceptance does not write this cache today)
- currentness comparison: not applicable
- terminal/clear policy:
  - clean: replace
  - recovered_partial: replace
  - budget_exhausted: replace
  - cancelled: clear
  - catastrophic_minimal: replace
  - guarded_no_parser_state: clear
  - desynchronized: out_of_scope
  - instrument_failure: out_of_scope
- focused proof filter: parse_effect_sink
- composed proof owner: #6729
- compatibility adapter: none
- disposition: existing exact owner (#6729)
- claim ceiling: Cache-authority reference + eviction-site ledger only; cache policy unchanged.

## `semantic-tokens.current-result-publication`

- title: Semantic-token current-result publication (pull-mode provider responses)
- owner: #9162/#9165/#9167
- ticket inputs: snapshot, client_uri
- sink-local subject: provider response derived from the published snapshot
- store: semantic_tokens_cache
- owns mutation sites: no
- mutation boundary: none locally: pull publication currentness/equivalence is owned by the #9162 train; this row prevents collapsing it with diagnostic computation or result-ID caches
- currentness comparison: external owner
- terminal/clear policy:
  - clean: out_of_scope
  - recovered_partial: out_of_scope
  - budget_exhausted: out_of_scope
  - cancelled: out_of_scope
  - catastrophic_minimal: out_of_scope
  - guarded_no_parser_state: out_of_scope
  - desynchronized: out_of_scope
  - instrument_failure: out_of_scope
- focused proof filter: parse_effect_sink
- composed proof owner: #9162
- compatibility adapter: none
- disposition: existing exact owner (#9162/#9165/#9167)
- claim ceiling: Authority reference only.

## `parser-state.accepted-snapshot-publication`

- title: Accepted parsed-snapshot publication into the open document (lazy source-region/type/semantic results derive from it)
- owner: #11665/#11668/#11670
- ticket inputs: document_instance, generation, snapshot, captured_text
- sink-local subject: DocumentState snapshot slot; instance-minting insert at didOpen; generation Arc identity closes reopen ABA
- store: documents_map
- owns mutation sites: yes
- mutation boundary: DocumentState::from_parts + publish_parsed_if_current (didOpen and synchronous fallback routes) + the fresh-instance documents.lock().insert at didOpen
- currentness comparison: same-thread admission before acceptance exists
- terminal/clear policy:
  - clean: replace
  - recovered_partial: replace
  - budget_exhausted: replace
  - cancelled: clear
  - catastrophic_minimal: replace
  - guarded_no_parser_state: clear
  - desynchronized: out_of_scope
  - instrument_failure: out_of_scope
- focused proof filter: parse_effect_sink
- composed proof owner: #11670
- compatibility adapter: none
- disposition: not proven (accepted-ticket minting/atomic acceptance has not landed yet (#11665/#11668/#11670 open); publish_parsed_if_current exists but the immutable AcceptedParseGeneration contract does not, so this row cannot claim proven governance)
- claim ceiling: Dependency pinning only; this contract neither changes nor gates the publication.

## `didopen-guard.minimal-document-admission`

- title: Pre-acceptance document-map admissions (guard minimal states on template/oversize/binary opens+edits; didChange text-state advance and reinstall)
- owner: #11665/#11668
- ticket inputs: none (these admissions happen before (or beside) any accepted parse; no ticket exists)
- sink-local subject: documents_map entry installed as minimal state, text-state-replaced ahead of a deferred parse, or reinstated after the synchronous fallback publish
- store: documents_map
- owns mutation sites: yes
- mutation boundary: minimal_state/minimal_state_from_rope guard inserts, replace_text_state advances, and the scoped documents.insert(doc_state) sites in didChange/didOpen lifecycle code
- currentness comparison: same-thread admission before acceptance exists
- terminal/clear policy:
  - clean: replace
  - recovered_partial: replace
  - budget_exhausted: replace
  - cancelled: clear
  - catastrophic_minimal: replace
  - guarded_no_parser_state: clear
  - desynchronized: out_of_scope
  - instrument_failure: out_of_scope
- focused proof filter: parse_effect_sink
- composed proof owner: #11668
- compatibility adapter: none
- disposition: not proven (pre-acceptance synchronous admission predates the accepted-state train; governed once #11668 mints tickets for guarded opens and deferred parses)
- claim ceiling: Ledger registration only; admission behavior unchanged.

## `readiness.active-document-parse-lifecycle`

- title: Active-document parser readiness/progress lifecycle counters
- owner: #11675
- ticket inputs: client_uri, settle_ownership
- sink-local subject: Coordinator pending-parse lifecycle per URI (notify_change increments; notify_parse_complete decrements exactly once per lifecycle, settle-hook owned when the async worker already credited it)
- store: coordinator_parse_lifecycle
- owns mutation sites: yes
- mutation boundary: Coordinator::notify_change / Coordinator::notify_parse_complete under coordinator state lock (#3660 settle ownership)
- currentness comparison: not applicable
- terminal/clear policy:
  - clean: observe
  - recovered_partial: observe
  - budget_exhausted: observe
  - cancelled: observe
  - catastrophic_minimal: observe
  - guarded_no_parser_state: observe
  - desynchronized: observe
  - instrument_failure: observe
- focused proof filter: parse_effect_sink
- composed proof owner: #11675
- compatibility adapter: commit_parse_effect_if_current (exit: #7379)
- disposition: new focused child (#11675)
- claim ceiling: Lifecycle-route inventory only; counter semantics deliberately untouched here. Currentness is not applicable: counters are idempotent bookkeeping keyed by settle-ownership (#3660) and intentionally fire even when a stale effect's content mutation was rejected, so coordinator state stays consistent.

## `readiness.open-ready-publication`

- title: Open-buffer active-document-ready notification and first-file readiness transition
- owner: #11675
- ticket inputs: document_instance, generation, client_uri
- sink-local subject: $/perlLsp/activeDocumentReady envelope generation-tagged with the first accepted generation; IndexState Idle->Ready transition
- store: workspace_readiness_publication
- owns mutation sites: yes
- mutation boundary: workspace_progress::send_active_document_ready_notification + Coordinator::transition_to_ready inside the didOpen background task's Accepted arm and the workspace-scan completion path
- currentness comparison: helper precheck then callback (residual window admitted)
- terminal/clear policy:
  - clean: publish
  - recovered_partial: publish
  - budget_exhausted: publish
  - cancelled: publish
  - catastrophic_minimal: publish
  - guarded_no_parser_state: publish
  - desynchronized: out_of_scope
  - instrument_failure: out_of_scope
- focused proof filter: parse_effect_sink
- composed proof owner: #11675
- compatibility adapter: commit_parse_effect_if_current (exit: #7379)
- disposition: new focused child (#11675)
- claim ceiling: Publication-route inventory only.

## `evidence.parse-effect-observations`

- title: Parse/effect timing and evidence observations (spans, worker metrics)
- owner: #9444
- ticket inputs: none (observations are advisory; they never gate correctness-bearing commits)
- sink-local subject: PERL_LSP_TIMING spans, ParseWorkerMetrics counters
- store: coordinator_parse_lifecycle
- owns mutation sites: no
- mutation boundary: none correctness-bearing (advisory observation only)
- currentness comparison: not applicable
- terminal/clear policy:
  - clean: observe
  - recovered_partial: observe
  - budget_exhausted: observe
  - cancelled: observe
  - catastrophic_minimal: observe
  - guarded_no_parser_state: observe
  - desynchronized: observe
  - instrument_failure: observe
- focused proof filter: parse_effect_sink
- composed proof owner: #9444
- compatibility adapter: none
- disposition: not applicable (evidence-only sink; excluded from the outcome contract because it can never be a stale-correctness boundary)
- claim ceiling: Classification only.

## `compat.legacy-generic-callback-helper`

- title: Legacy generic callback helper commit_parse_effect_if_current (+ free-function core)
- owner: #7379
- ticket inputs: document_instance, generation, normalized_uri
- sink-local subject: documents_map read-only check; NOT an atomic sink boundary
- store: documents_map
- owns mutation sites: yes
- mutation boundary: documents.lock() precheck released BEFORE arbitrary closure runs; admitted residual TOCTOU window; invoking this helper never satisfies any row's commit law
- currentness comparison: helper precheck then callback (residual window admitted)
- terminal/clear policy:
  - clean: delegate_to_owning_sink
  - recovered_partial: delegate_to_owning_sink
  - budget_exhausted: delegate_to_owning_sink
  - cancelled: delegate_to_owning_sink
  - catastrophic_minimal: delegate_to_owning_sink
  - guarded_no_parser_state: delegate_to_owning_sink
  - desynchronized: delegate_to_owning_sink
  - instrument_failure: delegate_to_owning_sink
- focused proof filter: parse_effect_sink
- composed proof owner: #7379
- compatibility adapter: commit_parse_effect_if_current (exit: #7379)
- disposition: compatibility projection with exit
- claim ceiling: Reported compatibility adapter with explicit consumers and removal owner (#7379 fan-in); retires as focused children cut each call site over to sink-local compare-and-mutate commits returning ParseEffectCommitOutcomeV1.
