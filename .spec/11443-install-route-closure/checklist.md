# Checklist: #11443

- [ ] Exact predecessor/base/head identities recorded in the PR.
- [ ] Canonical authorities consumed/retired named (see context.md).
- [ ] Wrong-case fixtures written before the implementation.
- [ ] Focused proof run and named falsifiers exercised.
- [ ] cargo test -p xtask --all-targets --locked (packet generation/check/explain, adapter mutation suite, second-run clean)
- [ ] Adjacent findings transferred to their owning issues, not absorbed.
- [ ] Stop conditions honored; no forbidden work absorbed.
