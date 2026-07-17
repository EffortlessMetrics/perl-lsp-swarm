# Issue #1715 — Acceptance Criteria

- [x] `prepareSupportDefaultBehavior` enables the default behavior variant only
      for protocol value `1`; out-of-range values do not wrap into `1`.
- [x] Plain valid identifiers can return `{ "defaultBehavior": true }` without
      also returning `range` or `placeholder`.
- [x] Sigiled variables return `{ range, placeholder }`, include the sigil, and
      do not include `defaultBehavior`.
- [x] Reserved Perl keywords return `null` from prepare-rename.
- [x] Empty rename exits pass through the negotiated WorkspaceEdit formatter.
- [x] `changes` conversion emits `documentChanges` entries with URI and edits.
- [x] Open-document entries preserve `DocumentState.version`; non-open entries
      use `null`.
- [x] Other WorkspaceEdit fields, including `changeAnnotations`, are preserved.
- [x] The legacy client path requires an object-valued `changes` map and omits
      `documentChanges`.
- [ ] Exact-head hosted required contexts pass: `Perl LSP Rust Small Result`
      and `ripr+ New Gap Gate`.
