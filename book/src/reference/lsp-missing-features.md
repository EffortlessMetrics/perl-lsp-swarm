# LSP Missing Features & Test Coverage Report

> **Last Updated**: 2026-11-01
> **Source of truth**: `features.toml` (capabilities) + `docs/project/CURRENT_STATUS.md` (metrics)
>
> This report lists **non-advertised** or **planned/preview** features only.
> If this file conflicts with `features.toml`, update this file.

---

## Summary

- **Advertised LSP features** are tracked in `features.toml` and should be GA/production.
- **Missing/not-advertised LSP surfaces** include preview notebook support, formatting routes withdrawn under #11955, and other catalog rows with `advertised = false` (see sections below). Whole-document formatting (`lsp.formatting`) remains **advertised** and live.
- For **coverage metrics**, see `docs/project/CURRENT_STATUS.md` (do not restate numbers here).

---

## Missing / Not Advertised LSP Features (from `features.toml`)

Every row below matches a catalog entry with `advertised = false`. Restoration owners for withdrawn formatting routes are taken from `features.toml` descriptions.

### Notebook Support (Preview)

1. **`lsp.notebook_document_sync`** - Notebook document synchronization
   - Status: preview, not advertised
   - Implemented: handlers + capability gating
   - Tests: `tests/lsp_comprehensive_3_17_test.rs`

2. **`lsp.notebook_cell_execution`** - Execution summary tracking
   - Status: preview, not advertised
   - Implemented: executionSummary tracking
   - Tests: `tests/lsp_comprehensive_3_17_test.rs`

3. **`lsp.notebook_document_filter_relative_pattern`** - Notebook document-filter relative patterns (LSP 3.18)
   - Status: unsupported, not advertised
   - Tests: `crates/perl-lsp-rs/tests/lsp_318_negative_claims.rs`

### LSP 3.17 / 3.18 unadvertised capability surfaces

1. **`lsp.file_watcher_relative_pattern`** - Relative-pattern watcher globs (`workspace/didChangeWatchedFiles`)
   - Status: not advertised (LSP 3.17 watcher surface; identity split #8897)
   - Tests: `crates/perl-lsp-rs/tests/lsp_registration_tests.rs`, `lsp_318_negative_claims.rs`

2. **`lsp.document_filter_relative_pattern`** - Document-filter relative-pattern support (LSP 3.18 text-document filter surface)
   - Status: unsupported, not advertised
   - Tests: `crates/perl-lsp-rs/tests/lsp_318_negative_claims.rs`

---

## Withdrawn formatting routes (#11955)

These catalog rows stay `advertised = false`; live dispatch refuses direct requests (see `live_dispatch_keeps_document_live_and_refuses_withdrawn_surfaces` in formatting policy tests). Whole-document formatting remains **advertised** (see the lsp.formatting row in the catalog) and is intentionally omitted from the inventory below.

| Feature id | Route | Restoration owner |
| --- | --- | --- |
| **`lsp.range_formatting`** | `textDocument/rangeFormatting` | #9317 |
| **`lsp.ranges_formatting`** | `textDocument/rangesFormatting` (@proposed) | #7089 |
| **`lsp.on_type_formatting`** | `textDocument/onTypeFormatting` | #9320 |
| **`lsp.will_save_wait_until`** | `textDocument/willSaveWaitUntil` (returns `method_not_advertised()` in text-sync lifecycle) | #8092 |

---

## Related DAP Gaps (Tracked in `features.toml`)

These DAP catalog rows have `advertised = false`. They are listed here because clients often ask about them alongside LSP gaps; they are not LSP features.

- **`dap.inline_values`** - Custom inlineValues extension; fail-closed until #9089 negotiation is proven
- **`dap.watchpoints`** - Data breakpoints; fail-closed until #9091 re-enable gate
- **`dap.step_in_targets`** - Targeted step-in refused (#9069)
- **`dap.goto_targets`** - gotoTargets not advertised
- **`dap.goto`** - goto request not advertised
- **`dap.restart_frame`** - No perl5db frame-restart primitive
- **`dap.terminate_threads`** - No per-thread terminate primitive

For DAP roadmap items and the broader advertised DAP surface, see `features.toml` and `docs/project/ROADMAP.md`.

---

## Test Coverage Notes

- Per-feature tests are declared in `features.toml`.
- Missing/preview features above are covered by the tests referenced in `features.toml`.

---

## How to Update This Report

1. Update `features.toml` (capability status is the source of truth).
2. Run `just status-update` to refresh computed docs.
3. Re-align this file if any items changed.
4. Run `python3 -m unittest scripts.tests.test_lsp_missing_features_report` to verify catalog parity.
