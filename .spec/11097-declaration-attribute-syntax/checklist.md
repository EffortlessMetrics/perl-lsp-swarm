# Checklist: #11097 — shared declaration-attribute source syntax

- [x] Add one owner-neutral declaration-attribute value module.
- [x] Preserve separator/name/argument/range/recovery identity.
- [x] Preserve order and duplicates by using ordinary sequence storage at the
  containing declaration boundary; do not deduplicate here.
- [x] Reject invalid ranges, out-of-parent ranges, missing exact delimiters,
  invalid delimiter order, and exact/recovered contradiction.
- [x] Add negative controls for the realistic flattening, deduplication,
  reordering, and guessed-exactness mistakes.
- [x] Add the required `.spec` authority packet.
- [ ] Run Rust formatter and tests when a Rust toolchain is available.
- [ ] Run hosted workspace checks for the candidate SHA.

## Verification record

Static inspection and `git diff --check` are available locally. `cargo fmt` and
`cargo test` are not locally executable because this environment has no cargo
binary; hosted CI is the authoritative Rust proof. No missing local proof is
represented as green.

## Stop conditions

Stop before parser production, `NodeKind`, flattened-field migration, or
semantic/provider behavior. A need for any of those returns to the controlling
issue graph rather than widening this candidate.
