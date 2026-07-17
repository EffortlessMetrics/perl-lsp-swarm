# Implementation Checklist: #1715 — Rename Protocol

## Change order (compiles at each step)

### Step 1: Read current PR and protocol seam
- **Files:** PR #4406, `rename.rs`, `capabilities.rs`, direct rename tests
- **Change:** Reconcile the existing implementation and review findings.
- **Verify:** Confirm all cited functions and tests exist on current `origin/main`.

### Step 2: Harden prepare-rename capability handling
- **File:** `crates/perl-lsp-rs/src/runtime/lifecycle/capabilities.rs`
- **Change:** Accept only protocol value `1`; preserve zero for absent/invalid values.
- **Verify:** Capability unit tests, including the out-of-range case.

### Step 3: Return valid prepare-rename variants
- **File:** `crates/perl-lsp-rs/src/runtime/language/rename.rs`
- **Change:** Reject keyword targets, preserve sigil-inclusive ranges, and delegate
  only valid plain identifiers.
- **Depends on:** Step 2
- **Verify:** `cargo test -p perl-lsp-rs --test lsp_rename_tests`

### Step 4: Format WorkspaceEdit responses
- **File:** `crates/perl-lsp-rs/src/runtime/language/rename.rs`
- **Change:** Route empty edits through the formatter; preserve metadata and use
  live document versions without reentrant document locking.
- **Depends on:** Step 1
- **Verify:** Focused conversion unit test and documentChanges integration test.

### Step 5: Record acceptance and review proof
- **Files:** `.spec/1715-rename-protocol/{context,acceptance,checklist}.md`,
  `crates/perl-lsp-rs/tests/lsp_rename_tests.rs`
- **Change:** Capture hazards, contracts, blast radius, test grid, and claim boundary.
- **Verify:** `git diff --check` and all named focused tests pass.

### Step 6: Final verification
- **Verify:** `cargo fmt --manifest-path crates/perl-lsp-rs/Cargo.toml -- --check`,
  `git diff --check`, focused rename tests, exact-head hosted required contexts.

## Callers and consumers

- `to_workspace_edit_format` is called by rename response paths in
  `handle_rename_workspace_inner`.
- `changes_to_document_changes` is private and called only by the formatter.
- `DocumentState.version` is read from the existing document map; no public type
  or signature changes are introduced.

## Scope boundary

Files in scope: rename response formatting, prepare-rename capability parsing,
direct rename tests, and `.spec/1715-rename-protocol`.

Files out of scope: change-annotation generation, parser grammar, unrelated
providers, release surfaces, and `doctor.rs`.

## Flags for builder

- `version: null` is reserved for documents not present in the live document map.
- Keep the issue claim partial: change annotations remain a follow-up.
- Do not merge until both required contexts are present and successful on the
  exact PR head.
