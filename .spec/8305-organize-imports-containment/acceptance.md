# Acceptance: #8305 — organize-imports containment

Each acceptance row lists the proof that discriminates it.

- [ ] No current client can receive the line-oriented organizer edit.
  Proof: `organize_imports_containment_tests.rs` (provider-level, executable
  statement between import-looking lines), rewritten `lsp_batteries_included_test`,
  `lsp_code_actions_tests`, `lsp_code_actions_comprehensive_tests{,_enhanced}`,
  and the exact-process fixture `crates/perllsp/tests/
  lsp_organize_imports_containment_process.rs` (filtered + unfiltered requests
  against the real `perllsp --stdio` binary).

- [ ] Production/default capability advertisement matches runtime availability.
  Proof: `source.organizeImports` absent from every profile snapshot
  (`production_capabilities.json`, `ga_lock_capabilities.json`,
  `all_capabilities.json`, `lsp_cap_snap__*`), from
  `code_action_kinds_include_exact_advertised_set`, and from the initialize
  response asserted by the process fixture; `BuildFlags` no longer carries the
  flag, so no profile can re-enable advertisement without a code change.

- [ ] Direct, filtered, resolve, command, extension, and compatibility bypasses
  fail closed.
  Proof: filtered and unfiltered provider/process tests assert zero legacy edits;
  resolve handler is pinned to quickfix-only edit filling (forged
  `source.organizeImports` action resolves unchanged, no injected edit); no
  executeCommand route exists (route inventory); extension command/menu/keybinding
  contributions removed with tests asserting absence.

- [ ] No enabled empty edit or silent alternate transform remains.
  Proof: no action of kind `source.organizeImports` is produced anywhere;
  architecture guard fails if any production source references the withdrawn
  organizer symbols again.

- [ ] Unrelated code-action and refactor families remain behaviorally unchanged.
  Proof: existing quickfix/refactor/source-fix-all/modernize suites pass
  unmodified (`cargo test -p perl-lsp-rs-core -p perl-lsp-rs --all-targets
  --locked code_action`); snapshots for other kinds unchanged.

- [ ] Current support/status/docs record withdrawn/not-proven, not GA or broad
  organizer support.
  Proof: features SOT descriptions drop SourceOrganizeImports from advertised
  kinds prose; LSP_FEATURES_OVERVIEW, EXTENSION.md, VS_CODE_SETUP,
  COMMANDS_REFERENCE, IMPORT_OPTIMIZER_GUIDE carry withdrawal state.

- [ ] Exact-process negative fixtures prove unrelated source bytes cannot change.
  Proof: process fixture opens real Perl source with executable statements
  between import-looking lines and asserts no returned edit touches them.

- [ ] A route/architecture guard prevents the legacy sorter or broad
  first-to-last replacement from becoming live authority again.
  Proof: `no_production_route_references_the_withdrawn_organizer` source-scan
  test plus behavioral no-legacy-action assertions.

- [ ] #8319/#10696 remain the only restoration path.
  Proof: spec packet context.md records restoration pointers; guard test names
  the issues in its failure message.

- [ ] No directive model, semantic disposition, replacement plan, range mapper,
  or external adapter enters this PR.
  Proof: diff scope review — only reachability and truthful product state change.
