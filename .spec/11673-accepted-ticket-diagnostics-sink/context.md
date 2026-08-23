# Context: #11673 - accepted-ticket push-diagnostics sink

Every parser-triggered `textDocument/publishDiagnostics` replacement or clear must
commit through one sink-local accepted-ticket boundary. Current main wraps the
post-parse callbacks in `commit_parse_effect_if_current`, but that is a
check-before-callback guard: the irreversible outbound `notify` still runs after the
check, and `publish_parse_errors_fast` performs no commit-time currency check at all.

Concretely on `main` (61a57f2e4):

- `publish_diagnostics` / `publish_syntax_only_diagnostics` re-validate with a
  value-only comparison (`generation.load() != gen_at_snapshot`) against a cloned
  `Arc<AtomicU32>`. A didClose+didOpen cycle that leaves the old instance's counter
  untouched passes this comparison and publishes stale diagnostics onto the URI now
  owned by the new document instance (close/reopen ABA).
- `publish_parse_errors_fast` snapshots under the documents lock and notifies with no
  staleness check; any await/preemption between snapshot and enqueue publishes stale-N
  errors after N+1 acceptance.
- The didOpen/didChange guarded no-parse paths (template, oversize, binary) call
  `self.notify` directly with no ticket identity at all.

The sink owns one operation: validate `(document_instance, generation)` currency at
the boundary under the sink lock, compare/record the committed diagnostic ticket and
monotonic sequence per normalized URI, enqueue exactly one replacement or clear while
still inside the boundary, and return a typed outcome. A later stale callback cannot
overwrite or reorder a committed publication because its own validation runs inside
the same critical section after the newer commit.

Candidate classes kept distinct: parser-fast replacement, canonical-full replacement,
current clear, guarded no-parse replacement/clear. Pull diagnostics, severity policy,
diagnostic computation, result caching (#7286/#7288), debounce scheduling mechanics,
and close-time clears (lifecycle-owned, not parser-triggered) remain out of scope.

Dependency note: #11672's outcome vocabulary (`ParseEffectCommitOutcomeV1`, PR
#11989) is still open in another lane. This candidate defines its claim-local
outcome type shaped for retargeting when that vocabulary lands.

Base: `main@61a57f2e4`.
