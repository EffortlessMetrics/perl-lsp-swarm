# Checklist: #11672

- [x] Verify currentness: accepted-ticket train (#11665/#11668/#11670) still open; no rival #11672 PR; no existing effect-sink `.spec` packet.
- [x] Inventory the complete post-parse effect surface: helper routes, didOpen/didChange guard admissions, symbol store, outbound diagnostics (fast/debounced/syntax-only/guard), workspace-index live commits and readers, semantic publication, result-ID caches, readiness lifecycle, open-ready publication, evidence observations, legacy helper.
- [x] Define `ParseEffectCommitOutcomeV1` closed vocabulary with typed non-application and NotProven semantics.
- [x] Define terminal-class matrix (`TerminalParseClassV1` × `SinkCurrentActionV1`) with total per-row policies.
- [x] Write `parse_effect_sinks_v1` static inventory: one exact owner, ticket inputs, sink-local subject, mutation boundary, currentness location, policy matrix, proof owners, adapter exit, disposition, claim ceiling per row.
- [x] Add shift-left falsifier checks: unique IDs, single owner/disposition, no duplicate mutation authority, ticket fields where currentness applies, total terminal policies, adapters-with-exit present in source, external owners not reimplemented, outcome vocabulary closed partition.
- [x] Add call-site ledger ratchet over production sources so an unregistered effect site fails deterministically.
- [x] Generate deterministic human projection and commit it as `inventory.md`; second-run cleanliness enforced against the committed file.
- [x] Focused proof green: `cargo test -p perl-lsp-rs --lib parse_effect_sink` (12 checks), fmt, clippy on owning packages only.
- [ ] Focused children cut each sink over to sink-local compare-and-mutate returning the common outcomes (#11675 readiness, #11674 document symbols, #11673 diagnostics).
- [ ] Accepted-ticket train lands (#11665/#11668/#11670); reclassify the two `not proven` admission/snapshot rows.
- [ ] Legacy helper retires through #7379 fan-in once no ledgered consumer remains.

## Writer conflicts / rollback / stop conditions

- Single candidate branch `agent/11672-effect-sink-contract`; one writer.
- Rollback = drop the additive module + `.spec` packet; no runtime state to unwind.
- Stop conditions honored: no sink behavior change, no accepted-ticket architecture change, no generic retained-state framework, no provider/performance/support action in this PR.
