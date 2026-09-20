# Context: #11675 - accepted-ticket active-document parser readiness

Active-document parser readiness becomes one generation-owned sink state
derived from the exact accepted parser ticket plus the required core effect
outcomes (profile v1: push diagnostics publication + local document symbols,
the two sinks landed by #12031 and #12035).

On `main@e81c9513b` the only active-document readiness signal was
`perl-lsp/active-document-ready`, emitted by didOpen/didChange background
WORKSPACE-INDEX tasks on successful `index_live_file` commits. That is
workspace-index-derived: it can precede any required-effect publication
(deterministically in inline-task test builds), disappears when an index
commit fails, and has no notion of the accepted ticket, recovered/limited
parser outcomes, guarded documents, or supersession.

The state machine (`readiness.rs`, tiers 1-2 of EFS-04 only):

- `PendingParser` installed for the exact target generation BEFORE parse work
  begins (didOpen insert, didChange generation bump / async enqueue);
- `ParserStateAcceptedEffectsPending` minted when that ticket's snapshot is
  published, classified Clean / RecoveredOrLimited (limitation carried) /
  Failed;
- Failed becomes terminal `UnavailableTerminal`; guarded no-parse documents
  become terminal `Guarded`; neither projects readiness;
- each required-effect sink commit attaches to the entry whose identity
  matches exactly; all required rows satisfied mints `ParserCoreReady`
  or `RecoveredOrLimitedReady`;
- every newer install supersedes the predecessor; close removes the claim;
- the notification is now a projection emitted at minting only.

Profile v1 applicability is fixed at install: pull-diagnostic clients mark
the push-publication row not_applicable. Queue settlement, pending-parse
counters, worker completion, and workspace-index state cannot construct or
mint readiness; provider policy (#3099), workspace (#10791/#8619/#8642),
semantic (#7309), dependency tiers, and scheduler mechanics are untouched.

Base: `main@e81c9513b`.
