# Implementation Checklist: #3002 - Module::Runtime document links

## Change order

### Step 1: Add red provider tests

- **File:** `crates/perl-lsp-rs-core/src/providers/document_links/mod.rs`
- **Change:** Add fallible provider tests that call `compute_links` directly.
- **Details:** Cover unqualified and qualified literal calls, both quote styles,
  valid whitespace, comments, multiple calls per line, dynamic arguments,
  malformed/escaped literals, and existing import/POD/pragma behavior.
- **Verify:** `rtk cargo test -p perl-lsp-rs-core document_links`

### Step 2: Implement the provider-local matcher

- **File:** `crates/perl-lsp-rs-core/src/providers/document_links/mod.rs`
- **Change:** Add a private matcher for literal `use_module` and `require_module`
  calls, including `Module::Runtime::` qualified spellings.
- **Details:** Reuse `make_deferred_module_link`; return byte/UTF-16 positions
  consistent with existing provider links; reject comments, dynamic expressions,
  malformed quoting, and unrelated calls. Support multiple calls on one line.
- **Depends on:** Step 1
- **Verify:** `rtk cargo test -p perl-lsp-rs-core document_links`

### Step 3: Add routed LSP proof

- **File:** `crates/perl-lsp-rs/tests/lsp_document_links_test.rs`
- **Change:** Add one `textDocument/documentLink` request assertion for a literal
  Module::Runtime call and its deferred module metadata.
- **Details:** Prove the request reaches the active provider; do not test the
  dead-code alternate scanner.
- **Depends on:** Step 2
- **Verify:** `rtk cargo test -p perl-lsp-rs --test lsp_document_links_test`

### Step 4: Run contract and source-exception checks

- **Files:** no additional source files expected.
- **Verify:**
  - `rtk cargo test -p perl-lsp-rs-core document_links`
  - `rtk cargo test -p perl-lsp-rs --test lsp_document_links_test`
  - `rtk cargo test -p perl-module --test module_import_bdd`
  - `rtk cargo allow check`
  - `rtk cargo allow diff --base origin/main`
  - `rtk cargo fmt --all -- --check`
  - `rtk cargo clippy -p perl-lsp-rs-core --tests -- -D warnings`

### Step 5: Final verification

- **Verify:** `rtk git diff --check`, focused tests above, relevant package check,
  `rtk cargo allow check`, then the repository's exact-head PR checks.

## Callers and consumers

- `compute_links` is called by `crates/perl-lsp-rs/src/runtime/language/document_links.rs`.
- `textDocument/documentLink` is routed by
  `crates/perl-lsp-rs/src/runtime/dispatch/routing.rs`.
- Deferred module links are consumed by the existing `documentLink/resolve` handler.

## Scope boundary

Files in scope:

- `.spec/3002-module-runtime-document-links/` (this contract)
- `crates/perl-lsp-rs-core/src/providers/document_links/mod.rs`
- `crates/perl-lsp-rs/tests/lsp_document_links_test.rs`

Files out of scope:

- `crates/perl-module/**`
- `crates/perl-semantic-analyzer/**`
- `crates/perl-lsp-rs/src/features/lsp_document_link.rs`
- completion, workspace indexing, goto-definition, protocol capability shapes,
  dynamic data-flow, dependency resolution, and unrelated cleanup

## Flags for builder

- Lock the literal range convention in tests: the range covers the module-name
  content, not quote delimiters.
- Keep link metadata identical to existing module links.
- Do not add a public API, dependency, or cargo-allow exception.
- If escaped quotes cannot be handled conservatively without broadening the parser,
  reject the case and preserve the no-guessing claim.
