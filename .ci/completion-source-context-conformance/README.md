# Completion source-context conformance (#13246)

`cases.json` is the independently authored oracle for opaque-region classification,
completion broad policy, and serialized LSP quiet-path behavior. Expectations come from
Perl/source-context contracts and explicit completion product policy — not from
`SourceRegionIndex` output, `lexical_context.rs` snapshots, or live completion item catalogs.

## Proof layers

1. **Canonical** — `SourceRegionIndex` kind at the probe (`perl-parser-core` integration test).
2. **Completion policy** — bounded lexical POD/comment/string/heredoc classifiers used by completion
   (`perl-lsp-rs-core` unit tests in `lexical_context.rs`).
3. **Serialized LSP** — `textDocument/completion` result class (`perl-lsp-rs` integration test).

Each row lists which layers apply via `layers`. A correct region with wrong policy or a quiet
core policy with a leaking LSP payload is a failure on the affected layer.

## Commands

```bash
cargo test -p perl-parser-core --locked --test completion_source_context_conformance
cargo test -p perl-lsp-rs-core --locked completion_source_context_policy_matrix
cargo test -p perl-lsp-rs --locked --test completion_source_context_conformance
cargo xtask source-context check
```

A green matrix gate proves the instrument and selected rows executed with deterministic output.
It does not claim every row family in issue #13246 is already represented; extend `cases.json`
in place rather than adding parallel corpora.

## Known canonical gaps

Rows with `canonical_mismatch_owner` document unfixed producer defects (for example `#13241`
`=cut` identifier-continuation on `SourceRegionIndex`). Those rows still pin expectations and
are excluded from the canonical green denominator until the owner lands.
