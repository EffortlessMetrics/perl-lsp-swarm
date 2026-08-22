# Acceptance: #10690 — missing-import containment

Each acceptance property, its proof surface, and where the proof runs.

- [ ] No production request can return a source edit from
  `guess_module_for_function` or equivalent hard-coded affinity.
  Proof: the table, both route functions, the enhanced dispatch block, and the
  PL109 routing extension are deleted; provider-level containment tests assert
  no import action for every mapped spelling; the source-scan guard
  `no_production_route_references_the_withdrawn_affinity_routes` fails if any
  banned symbol reappears under any `crates/*/src` path.

- [ ] Both the enhanced global and PL109 diagnostic bypasses are unreachable.
  Proof: `add_missing_imports` + `find_undefined_functions` (and their file)
  are gone from `enhanced/mod.rs`; `diagnostic_routes.rs` keeps only the
  quoting/filehandle `fix_bareword` arm for PL109. Enhanced-provider tests
  assert no "Add missing imports" action over undefined-function sources;
  provider-level PL109 tests assert no import action while quote actions
  survive.

- [ ] Direct, filtered, resolve, command, completion, and compatibility routes
  fail closed.
  Proof: exact-process fixture `crates/perllsp/tests/
  lsp_missing_import_withdrawal_process.rs` drives the real `perllsp --stdio`
  binary with an unfiltered request, a `context.only: ["quickfix"]` request,
  a minimal client without `codeAction.disabledSupport`, a forged
  `codeAction/resolve`, and a fabricated `workspace/executeCommand` probe.
  Provider-level direct dispatch covers the unfiltered path. Completion has no
  table consumer (inventory in context.md); compat placeholder is removed.

- [ ] Disabled/omitted behavior is truthful and carries no executable edit or
  command.
  Proof: no action of any kind keyed to the withdrawn families returns — not an
  empty edit, not a no-op rewrite, not a disabled stub carrying data. Stand-in
  rejection assertions run at provider level and over stdio.

- [ ] PL109's unrelated proven fixes and other action families remain
  behaviorally unchanged.
  Proof: control assertions keep "Quote … with single quotes",
  "Quote … with double quotes", and uppercase-filehandle declaration reachable
  while import edits are refused; existing quick-fix suites pass unmodified;
  process fixture asserts the pragma family stays reachable over stdio.

- [ ] Capability/provider/completion/status/docs surfaces no longer advertise
  automatic missing-import insertion.
  Proof: inventory found exactly one advertising surface — the vision bullet in
  `docs/project/PERL_LSP_VISION.md` — which is rewritten truthfully;
  features.toml never advertised it. Checklist records the verification.

- [ ] #790/#8948 remain the sole replacement owners for exact candidate
  discovery and insertion semantics.
  Proof: spec packet and PR body name them; no detector, exporter search,
  planner, plan type, network/package install, or source mutation enters the
  diff (deletion-only production change).

- [ ] Route/architecture tests prevent a name map, diagnostic message,
  `container_name`, or byte-zero insertion from returning as exact authority.
  Proof: source-scan guard bans the withdrawn symbols workspace-wide; behavioral
  falsifiers cover spelling→module authorization, local/imported collisions,
  wrong-package insertion geometry, and prose/presentation retargeting; any
  restoration attempt trips the focused gate by construction.

- [ ] No new unresolved-call detector, exporter search, insertion planner, plan
  type, network/package install, or source mutation enters this PR.
  Proof: diff scope review — production change is deletion-only; everything
  else is tests, spec packet, and one doc truthfulness edit.
