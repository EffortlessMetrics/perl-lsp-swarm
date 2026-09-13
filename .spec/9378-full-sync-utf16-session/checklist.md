# Checklist — #9378 full-sync UTF-16 initialize/session contract

- [x] #8129 selected `full_document_utf16` for the v0.18 envelope.
- [x] One immutable accepted contract owns `sync_kind = full` and
      `position_encoding = utf-16`.
- [x] Every valid position-encoding string list accepts that contract.
- [x] Valid nonempty lists omitting UTF-16 retain the bounded
      `mandatory-utf16-fallback` reason rather than creating another encoding
      or a `no-common-encoding` rejection.
- [x] Absent, null, empty, explicit UTF-16, and mandatory-fallback offers have
      distinct evidence dispositions.
- [x] Non-array values and non-string entries fail typed `-32602` with no
      accepted session.
- [x] First-attempt authority is consumed before classification; accepted and
      rejected first attempts are both one-shot.
- [x] Every second initialize returns `-32600` before parameter-specific
      classification and cannot replace an accepted contract.
- [x] Concurrent attempts have exactly one first-attempt owner.
- [x] Attempted-but-unaccepted state cannot serve requests, complete
      initialization, intercept formatting, mutate documents, start
      watcher/index/bootstrap work, or emit readiness.
- [x] `initialization_accepted()` remains the single serving/completion truth.
- [x] Response `positionEncoding` and `textDocumentSync.change` are derived
      from the accepted contract and verified before acceptance.
- [x] `ClientCapabilities.position_encoding`, local sync-kind authority, and
      independent hard-pinned response strings are removed.
- [x] Pull-diagnostics and effective-surface parity consumers read the accepted
      session contract.
- [x] Bounded evidence records exact offer, reason, session, contract digest,
      response digest, and terminal outcome.
- [x] `.spec/9378-full-sync-utf16-session/` states the same one-shot and
      mandatory-fallback law as code and tests.
- [ ] Wire-level malformed-first → valid-second, accepted-first → malformed-
      second, post-rejection serving, and mandatory-fallback sequences pass on
      the exact current branch.
- [ ] Focused Rust tests, Clippy, rustfmt, generated final-surface checks, and
      substantive review are current after candidate integration.
- [ ] PR #14159 is merged before #9380 begins.

## Integration order

```text
reconcile actual conflicts or combined-tree failures
→ regenerate affected public API and inventory when their surfaces change
→ run focused and affected proof
→ receive fresh exact-head review
→ merge #9378
→ admit #9380
```

The [integration ruling](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14159#issuecomment-5607118090)
supersedes the former #12067-first ordering. Either carrier may integrate independently.
The second to land must retain accepted-session UTF-16 projection together with accepted
Critic policy and topology currentness, and run affected lifecycle/diagnostic identity
tests plus compilation. Do not chase unrelated main movement or race a live writer.
