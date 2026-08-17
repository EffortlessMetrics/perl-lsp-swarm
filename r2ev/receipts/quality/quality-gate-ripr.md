# Quality Gate

## Quality-gate effect

- decision: `fail`
- mode: `enforce-new-ripr`
- diff RIPR receipt: `present`
- new RIPR gaps: `52`
- repo RIPR+ receipt: `present`
- total RIPR+ gaps: `2515`
- review guidance receipt: `present`
- temporary exceptions: `present`
- active temporary exceptions: `2`
- final enforcement blocked: `true`
- exception: `ripr-total-burndown` final target `repo-wide ripr+ unresolved total = 0`
- exception: `project-coverage-burndown` final target `workspace project coverage >= 95%`
- receipt freshness: `repo_ripr=present, diff_ripr=present, review_guidance=present, exceptions=present`

## Proof Commands

- verify: `cargo xtask quality-gate --mode enforce-new-ripr --exception-policy policy/quality-gate-exceptions.toml --ripr-receipt target/receipts/quality/ripr-plus.json --ripr-pr-receipt target/ripr/pr/repo-exposure.json --review-receipt target/ripr/review/comments.json --ripr-base origin/main --ripr-head HEAD --receipt target/receipts/quality/quality-gate-ripr.json --summary target/receipts/quality/quality-gate-ripr.md --check`
- receipt: `cargo xtask quality-gate --mode enforce-new-ripr --exception-policy policy/quality-gate-exceptions.toml --ripr-receipt target/receipts/quality/ripr-plus.json --ripr-pr-receipt target/ripr/pr/repo-exposure.json --review-receipt target/ripr/review/comments.json --ripr-base origin/main --ripr-head HEAD --receipt target/receipts/quality/quality-gate-ripr.json --summary target/receipts/quality/quality-gate-ripr.md`

## Next Actions

### new_ripr_gap

- path: `crates/perl-lsp-rs/src/runtime/lifecycle/capabilities.rs`
- repair: `Add focused tests that expose the new RIPR seam before merging, then refresh RIPR receipts.`
- verify: `cargo xtask quality-gate --mode enforce-new-ripr --exception-policy policy/quality-gate-exceptions.toml --ripr-receipt target/receipts/quality/ripr-plus.json --ripr-pr-receipt target/ripr/pr/repo-exposure.json --review-receipt target/ripr/review/comments.json --ripr-base origin/main --ripr-head HEAD --receipt target/receipts/quality/quality-gate-ripr.json --summary target/receipts/quality/quality-gate-ripr.md --check`
- receipt: `cargo xtask quality-gate --mode enforce-new-ripr --exception-policy policy/quality-gate-exceptions.toml --ripr-receipt target/receipts/quality/ripr-plus.json --ripr-pr-receipt target/ripr/pr/repo-exposure.json --review-receipt target/ripr/review/comments.json --ripr-base origin/main --ripr-head HEAD --receipt target/receipts/quality/quality-gate-ripr.json --summary target/receipts/quality/quality-gate-ripr.md`
- suggested_test: `Add or update the focused test named by RIPR review guidance for the changed file, line, and seam.`
- ripr gap: `4ffa8ff956d75b1a` `crates/perl-lsp-rs/src/runtime/lifecycle/capabilities.rs:785` seam `owner_function_changed_line` reason `Static evidence names missing discriminator `input that hits the boundary: item.as_str() == Some("data")` for this seam.` suggested test `Add one focused discriminator test.`
- ripr gap: `0c7386e8177b7cc3` `crates/perl-lsp-rs/src/runtime/lifecycle/capabilities.rs:785` seam `owner_function_changed_line` reason `Static evidence names missing discriminator `input that hits the boundary: tag.as_i64() == Some(1)` for this seam.` suggested test `Add one focused discriminator test.`
- ripr gap: `25f90ee825ce7974` `crates/perl-lsp-rs/src/runtime/lifecycle/capabilities.rs:785` seam `owner_function_changed_line` reason `Static evidence names missing discriminator `input that hits the boundary: v == 1` for this seam.` suggested test `Add one focused discriminator test.`

