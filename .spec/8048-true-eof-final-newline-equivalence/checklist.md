# Checklist: #8048 shift-left equivalence seam

Base pin: `origin/main@b7a7fd57fc30813f0dbfc7772e769e300d5f6ce0` (2026-08-22).
Sibling lane: PR #11873 covers the
`TextRange::whole_document` true-EOF correction; this candidate neither
duplicates nor stacks on it.

- [x] Pure terminal-sequence matrix: empty/no-final/LF/CRLF/bare-CR/mixed/multiple endings across insert-only, trim-only, both, neither (`final_newline_policy_tests`)
- [x] Independent fallible applicator rejecting reversed / out-of-bounds / overlapping / mid-code-point edits and preserving distinct same-position insertion order in UTF-16 and UTF-8-byte encodings (`edit_application_equivalence_tests`)
- [x] Mutation controls restoring extra-newline, trim-all, split-CRLF, force-LF false-pass paths turn red at the seam
- [x] Fixtures whose old `text.lines()` helper cannot distinguish true EOF
- [x] Evidence-timing control: evidence must match recomputation from final returned bytes
- [x] No-op normalization fixture: identity projection classifies as no change with zero actions
- [ ] Native-vs-wire application fixtures consuming #10239/#10242 types (blocked; owners named)
- [ ] Production wiring of `FinalNewlinePolicy` into native/LSP/save/CLI routes (blocked by #10237→#10239→#10242)
- [ ] Terminated-source produced-edit parity over `FormatResult::replace_document` (lands with #11873 geometry cutover)
- [ ] `get_document_end_position` convergence (deferred to the separate #10220 endpoint work)

Proof commands:

```bash
cargo test -p perl-lsp-perltidy --all-targets --locked
cargo clippy -p perl-lsp-perltidy --all-targets --locked -- -D warnings
cargo fmt -p perl-lsp-perltidy -- --check
```
