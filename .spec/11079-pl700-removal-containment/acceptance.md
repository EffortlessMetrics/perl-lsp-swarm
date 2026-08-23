# Acceptance: #11079 — PL700 removal containment

Each acceptance property, its proof surface, and where the proof runs.

- [ ] No production request can return an edit from `fix_unused_import` or
  equivalent PL700 prose/whole-line authority.
  Proof: the route arm and the fix function are deleted; provider-level
  containment tests assert no action and no line-deleting edit for every
  falsifier shape below; `no_production_route_references_the_withdrawn_pl700_edit`
  source-scan guard fails if `fix_unused_import` reappears under any
  `crates/*/src` path or if `diagnostic_routes.rs` mentions `UnusedImport`
  again (the routing file is the mutation surface).

- [ ] Direct, filtered, resolve, command, and compatibility bypasses fail closed.
  Proof: exact-process fixture `crates/perllsp/tests/lsp_pl700_withdrawal_process.rs`
  drives the real `perllsp --stdio` binary with an unfiltered request, a
  `context.only: ["quickfix"]` request, a request whose context carries a
  producer-shaped PL700 diagnostic, and a forged `codeAction/resolve`; none may
  return an import-removal edit. Provider-level tests cover direct dispatch.
  Command surfaces: route inventory records no command reaches this family.
  Compatibility providers: verified non-routes in context.md.

- [ ] Disabled/omitted behavior is truthful and carries no executable edit or
  command.
  Proof: no action of any kind is returned for the PL700 family — not an empty
  edit, not a no-op, not a disabled stub carrying data. Clients with and without
  `codeAction.disabledSupport` receive the same omission (process fixture pins
  both initialize capability shapes).

- [ ] The PL700 diagnostic remains explicitly non-fixable and cannot be
  promoted as exact unusedness.
  Proof: diagnostic producer untouched (`lints/unused_imports.rs` diff-free);
  diagnostic presentation snapshots unchanged; only edit authority is removed.

- [ ] Explicit-symbol and complete-load replacement ownership stays separate.
  Proof: #1719/#8322 named as owners in spec packet and PR body; no detector,
  assessment, planner, mapper, or replacement operation enters the diff.

- [ ] Unrelated diagnostic quick fixes and actions remain behaviorally unchanged.
  Proof: provider-level control test keeps PL102 unused-variable rename/remove
  working alongside a refused PL700 diagnostic; existing code-action suites pass
  unmodified; process fixture asserts "Add 'use strict'" pragma action still
  reachable over stdio while import edits are refused.

- [ ] Capability/provider-contract/status/docs surfaces do not advertise
  automatic PL700 removal.
  Proof: inventory found no surface advertising it (features.toml QuickFix prose
  names variables/strict/warnings/deprecated patterns only; LSP_FEATURES_OVERVIEW
  likewise); checklist records the verification. No snapshot changes expected —
  quickfix kind stays advertised for surviving families.

- [ ] The route regression prevents diagnostic prose, action title, line shape,
  or raw range from becoming import-edit authority again.
  Proof: source-scan guard plus behavioral falsifiers (whole-line deletion of a
  use-with-import-list line, comment loss, prose retargeting, multiline range
  expansion) each fail against a restored route by construction.

- [ ] No new detector, assessment, source planner, range mapper, or replacement
  operation enters this PR.
  Proof: diff scope review — production change is deletion-only (route arm +
  fix function); everything else is tests, spec packet, and docs truthfulness.
