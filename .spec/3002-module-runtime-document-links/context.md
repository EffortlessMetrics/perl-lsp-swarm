# Context: #3002 - Module::Runtime document links

## Problem

The active `textDocument/documentLink` provider recognizes line-headed `use` and
`require` imports, but it does not emit deferred module links for statically named
`use_module()` and `require_module()` calls. The original issue also claimed gaps in
parsing, semantic analysis, and navigation; current main disproves those claims.

## Why this approach

The routed path is `crates/perl-lsp-rs/src/runtime/language/document_links.rs` to
`crates/perl-lsp-rs-core/src/providers/document_links/mod.rs::compute_links`.
The provider already owns line-oriented document-link extraction, deferred module
link metadata, and UTF-16 range conversion. A private matcher beside the existing
import-head branch keeps the change local and reuses the current resolve contract.

Only literal module names are safe to link without a new data-flow or workspace-index
contract. Dynamic variables, concatenations, interpolation, and runtime execution
remain explicitly unresolved.

## Alternatives rejected

- **Change `perl-module::parse_module_import_head`:** rejected because that parser
  intentionally models line-headed `use`/`require` statements and its tests reject
  function-call forms.
- **Change semantic analysis or completion:** rejected because those paths already
  recognize static Module::Runtime calls and are not the failing user surface.
- **Add a shared semantic helper or workspace-index fact:** deferred; it would widen
  the contract without enabling deterministic links for dynamic values.
- **Edit `features/lsp_document_link.rs`:** rejected because it is an alternate,
  legacy scanner and is not reached by the routed request handler.

## Prior art and evidence

- Issue research: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3002
- Accuracy verdict: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3002#issuecomment-4999508246
- Adversarial test verdict: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3002#issuecomment-4999506727
- Architecture verdict: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3002#issuecomment-4999513556
- Current semantic coverage: `crates/perl-semantic-analyzer/src/analysis/declaration.rs`
  and `crates/perl-semantic-analyzer/tests/require_import_resolution.rs`.
- Current document-link coverage: `crates/perl-lsp-rs-core/src/providers/document_links/mod.rs`
  and `crates/perl-lsp-rs/tests/lsp_document_links_test.rs`.

## Claim boundary

This slice proves static document-link emission and preserves existing resolution
metadata. It does not prove runtime loading, dependency availability, dynamic
module resolution, semantic import behavior, completion, or goto-definition.

## Cargo-allow policy

No production or test source exception is required. The implementation must not add
`unwrap`, `expect`, panic macros, unchecked indexing, or new lint suppressions. Run
`cargo allow check` against the final head and record any repository-wide
baseline debt separately. Run `cargo allow diff --base origin/main` for the
lane-level comparison; an empty new-entry result is the expected encoding for this
spec.
