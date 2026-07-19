# Implementation checklist: agent context v1

- [ ] Add the command to capability, provider, and input-validation surfaces.
- [ ] Add the live dispatcher branch and read-only envelope module.
- [ ] Add capability, validation, parity, redaction, and behavior tests.
- [ ] Update capability snapshots and add the envelope schema.
- [ ] Update the command reference and Codex onboarding guide.
- [ ] Run focused tests, `cargo fmt`, `cargo clippy --workspace`,
  `cargo test -p perl-lsp-rs-core`, and `git diff --check`.
- [ ] Keep bridge support conditional because bridge forwarding is not proven by
  this repository.
