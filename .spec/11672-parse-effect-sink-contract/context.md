# Context: #11672 — accepted-ticket effect-sink contract and checked inventory

Current runtime routes post-parse actions through
`run_post_parse_side_effects` and `commit_parse_effect_if_current`
(`crates/perl-lsp-rs/src/runtime/text_sync.rs`). The helper rechecks document
instance + generation under `documents.lock()`, releases the lock, and only
then invokes the commit closure. Its own rustdoc admits this is not atomic: a
newer edit can land between check and mutation (the admitted residual TOCTOU
window). The denominator is wider than that helper: synchronous didOpen guard
routes insert minimal `DocumentState`, publish empty/binary diagnostics, and
clear symbols without any ticket; the didChange lifecycle advances text state,
reinstalls the map entry, and spawns background workspace-index tasks; and the
didOpen open-ready path publishes readiness notifications after an accepted
index commit.

Different effects also have different real owners. Treating them as one
callback category hides distinct mutation boundaries, identity requirements,
and failure modes; letting each sink invent a local ticket vocabulary would
create drift.

This claim defines, without changing any sink behavior:

1. One closed common outcome vocabulary, `ParseEffectCommitOutcomeV1`
   (`crates/perl-lsp-rs/src/runtime/parse_effect_contract.rs`), covering
   committed-current, typed rejections, typed non-application
   (`SupersededBeforeMutation`, `NoEffectRequired`), safe-clear commits,
   sink/product failures, and a two-way evidence split: absent evidence
   (currentness unobservable) maps to `NotProven`, while unreliable evidence
   (an instrument/schema failing mid-commit) stays the distinct typed
   `InstrumentOrSchemaFailure`; neither is ever a commit or non-application.
2. One checked static inventory, `parse_effect_sinks_v1`, with one row per
   governed parse-derived effect naming its exact owner issue, accepted-ticket
   inputs, sink-local subject, store, irreversible mutation boundary,
   currentness-comparison location, full terminal/clear policy matrix,
   focused and composed proof owners, compatibility adapter exit, exactly one
   disposition, and a claim ceiling.
3. Deterministic checks (test filter `parse_effect_sink`) proving unique IDs,
   single owner/disposition per row, no duplicate mutation authority, ticket
   fields declared where a currentness comparison applies, total
   per-terminal-class policies, adapter exits present in source, external
   authorities referenced rather than reimplemented, a call-site ledger whose
   counts ratchet every registered production mutation site against source
   (`usize::MAX` on unreadable files), a closed partition of the outcome
   vocabulary, and a second-run-clean generated projection committed at
   `inventory.md`.

The accepted parser-state train (#11665/#11668/#11670) is still open, so no
immutable accepted ticket type exists in source yet. The snapshot-publication
and pre-acceptance admission rows therefore carry honest `not proven`
dispositions pinning that dependency; nothing here mints tickets or gates on
them.

Named residue outside this contract's denominator: didClose/didChange
lifecycle cleanups that are event-derived rather than parse-derived (session
cancellation, semantic-token cache eviction on close, generation poisoning to
`u32::MAX`, cancel-flag stores). They are close/open lifecycle authority
(#1374/#3660), not post-parse effects, and are deliberately not rows here;
the call-site ledger still registers their symbol-clear site under the
document-symbols row so the store stays singly owned.

Ownership rulings honored: #11665 owns accepted parser tickets; each sink owns
its compare-and-mutate operation; #8619/#8642 remain workspace-index atomic
publication/read authority (`SourceCommitOutcome` is referenced, never
reimplemented); #7309 remains semantic/project publication authority; #7286/
#7288 own diagnostic computation/result reuse, not outbound publication
currentness; #6729 owns per-kind result-ID caches; #9162/#9165/#9167 own
semantic-token currentness/equivalence. The legacy generic callback helper is
ledgered as compatibility projection with exit owner #7379 fan-in; invoking it
never satisfies any row's commit law.
