# Acceptance Criteria: #11371 — canonical Vim + vim-lsp user journeys and evidence boundaries

This is a checked, declarative BDD/spec-ledger contract. It implements no
fixture, host driver, provisioning, server/client behavior, receipt, support
tier, or generated-status machinery. Executable truth for every row below is
owned by the downstream leaves named in its evidence boundary.

## §Behavior — baseline journey ledger (`vim_actual_client_core`)

Normative wording is from the user's/editor's observable perspective. Exact
Vimscript names, commands, JSON fields, polling mechanics, hashes, and paths
belong to #10938 and later leaves. Profile column: membership in
`vim_actual_client_core` (all 23 rows) plus named substrate prerequisites.

### Feature: Vim attaches vim-lsp to the intended Perl project

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `vim.bdd.attach.01` | An ordinary Perl buffer activates the Perl language server through Vim's native Perl filetype detection before any override applies | core; actual host required; subject = Vim + vim-lsp | #10938 cell → #10944/#10946/#10951/#10955/#10958 observation → editor_client_compat.v1 cell → #10962 fan-in → #10974/#7122 support → #10978 prose |
| `vim.bdd.attach.02` | The attached client plugin is exactly the pinned upstream vim-lsp subject recorded by the governed subject manifest (#12050); a different build or copy does not satisfy attachment | core; actual host required; exact-source evidence | same chain, exact-subject binding at #10962 |
| `vim.bdd.attach.03` | The session runs the Perl language server as exactly `perllsp --stdio` | core; actual host required; executable identity law #7691/#12050 | same chain |
| `vim.bdd.attach.04` | The project root the server answers from is the root selected by the governed root contract (#7762) for the opened buffer's project | core; actual host required | same chain |
| `vim.bdd.attach.05` | A same-named sibling or enclosing project outside the governed root never satisfies a root-sensitive answer | core; actual host required | same chain + #10938 false-subject mechanics |
| `vim.bdd.attach.06` | After opening a defective Perl buffer, the expected diagnostic becomes visible through the actual client diagnostic state | core; actual host required | same chain |
| `vim.bdd.attach.07` | Editing the defective source updates the visible diagnostic to the current state | core; actual host required | same chain |

### Feature: Vim applies ordinary completion and navigation through vim-lsp

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `vim.bdd.nav.01` | Completion is requested at a real code target and reaches the server through vim-lsp | core; actual host required | #10938 cell → host observations → editor_client_compat.v1 → #10962 → #10974/#7122 → #10978 |
| `vim.bdd.nav.02` | The expected server completion item is consumed and offered by vim-lsp in the insert-mode popup | core; actual host required | same chain |
| `vim.bdd.nav.03` | With snippet support disabled on the client path, accepted completion yields correct final plain Vim text with no literal placeholder syntax left behind | core; actual host required | same chain |
| `vim.bdd.nav.04` | Hover identifies the intended entity at the cursor | core; actual host required | same chain |
| `vim.bdd.nav.05` | Definition resolves the intended entity of this project | core; actual host required | same chain |
| `vim.bdd.nav.06` | References list contains the governed sites of this project and excludes wrong-root decoy matches | core; actual host required | same chain + #10938 decoys |

### Feature: Vim applies server edits and configuration effects

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `vim.bdd.edit.01` | Rename changes exactly the intended occurrences across exactly the intended files of this project, through vim-lsp edit application | core; actual host required | #10938 cell → host observations → editor_client_compat.v1 → #10962 → #10974/#7122 → #10978 |
| `vim.bdd.edit.02` | Formatting brings the buffer to the canonical formatted result via client-applied edits | core; actual host required | same chain |
| `vim.bdd.edit.03` | Workspace configuration reaches the server using the governed nested `perl.*` shape admitted by #6736/#4998 and encoded by #12050 | core; configuration documented substrate; security-sensitive configuration | same chain; field admission owned by #6736 catalog |
| `vim.bdd.edit.04` | Changing workspace configuration changes an independent semantic result (include-path resolution), not merely the settings echo | core; actual host required; security-sensitive configuration | same chain |
| `vim.bdd.edit.05` | Absolute or traversal include-path settings remain outside ordinary positive behavior: rejected or governed per #4998, never silently applied as a trusted channel | core (negative/guard); security-sensitive configuration | same chain; rejection semantics owned by #4998 |

### Feature: Vim preserves position, synchronization, currentness, and lifecycle correctness

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `vim.bdd.lifecycle.01` | An operation issued after non-BMP text lands on the intended range, not an adjacent one | core; actual host required | #10938 cell → host observations → editor_client_compat.v1 → #10962 → #10974/#7122 → #10978 |
| `vim.bdd.lifecycle.02` | Actual incremental change traffic from vim-lsp is observed under the current generic synchronization contract | core; actual host required; instrument-only seam per public-surface inventory | same chain; instrumentation bounded by #12050 surface classification |
| `vim.bdd.lifecycle.03` | A post-edit answer reflects the accepted current generation, never the pre-edit state | core; actual host required | same chain |
| `vim.bdd.lifecycle.04` | Closing and reopening the document does not restore stale document state | core; actual host required | same chain |
| `vim.bdd.lifecycle.05` | Normal host shutdown leaves no bound perllsp process behind | core; actual host required; cleanup observed independently | same chain; cleanup mechanics authority #10894/#8734-class conformance stays with host leaves |

## §Behavior — optional and stronger-profile inputs (never baseline blockers)

| Scenario ID | Proposition | Profile relation | Boundary note |
| --- | --- | --- | --- |
| `vim.bdd.opt.01` | Experimental workspace folders usable end-to-end | consumes_if_available input to future cells; default off | existence cannot block or satisfy the core; enablement owned by #10960 |
| `vim.bdd.opt.02` | Maintained version/build/platform rows hold | stronger-profile input | rows owned by #10966; upstream theoretical prerequisites are never a support floor |
| `vim.bdd.opt.03` | Public release archive replay reproduces the journeys | stronger-profile input feeding `vim_public_artifact` | separate direct stage owner #10970; local evidence never relabels upward |
| `vim.bdd.opt.04` | Upstream vim-lsp-settings ships the registration | external checkpoint input | external submission/acceptance stages owned by #7712; internal merge cannot satisfy them |
| `vim.bdd.opt.05` | Interactive UX recommendation adopted | optional recommendation | owner #7771; not a behavior proposition here |
| `vim.bdd.opt.06` | Alternate yegappan/lsp client generation works | separate exact-subject family | owner #7717; can never fill a Vim + vim-lsp scenario |
| `vim.bdd.opt.07` | Vimspector DAP debugging works | separate protocol rail | owner #7702; DAP evidence never fills an LSP scenario |

## Claim profiles (ledger membership)

| Profile | Membership rule | Ceiling |
| --- | --- | --- |
| `vim_configuration_documented` | documented setup + #12050 governed subject/config/surface contracts | substrate only; proves no behavior |
| `vim_actual_client_core` | exactly the 23 baseline rows above | bounded core; nothing else blocks or widens it |
| `vim_first_class_exact_source` | core + first-class exact-source evidence rows | exact-source evidence never becomes released/public |
| `vim_public_artifact` | first-class exact source + #10970 replay evidence | public stage requires its own direct evidence |
| `vim_programme_closeout` | fan-in over independently terminal child propositions | composes child results only; manufactures none |

Laws: a stronger profile never erases a narrower valid one; optional rows are
`consumes_if_available` only; missing chain links are `not_proven`; no
scenario supports any tier until its executable exact-host chain passes.

## §Hazards

| Class | Invariant | Surface | Required adversarial check |
| --- | --- | --- | --- |
| Subject identity substitution | Only pinned Vim + vim-lsp + `perllsp --stdio` satisfies an actual-host row | attach.02/attach.03 rows; F13 | another client/build/platform/substitute-subject mutation is rejected |
| Stage promotion | Fixture, observation, receipt cell, fan-in, support, prose stay distinct owners | every evidence-boundary cell; F11 | capability/receipt/prose presented as host observation is rejected |
| Root conflation | Governed root contract wins; siblings/outer projects never satisfy root-sensitive rows | attach.04/attach.05; F1 | wrong-sibling-root answer passing as correct is rejected |
| Currentness forgery | Post-edit answers belong to the accepted generation | lifecycle.03/lifecycle.04; F9 | stale pre-edit result after accepted edit is rejected |
| Range adjacency | Non-BMP offsets resolve to intended targets only | lifecycle.01; F10 | adjacent-range landing mutation is rejected |
| Application honesty | Client must actually apply server results | nav.02/nav.03/edit.01/edit.02; F3/F4/F6/F7 | response-exists-but-not-applied mutations are rejected |
| Configuration theater | Configuration objects must have independent semantic effect | edit.03/edit.04; F8 | settings-object-without-effect mutation is rejected |
| Security boundary | Workspace-contained relative include paths only; absolute/traversal stays governed/rejected | edit.05; #4998 | unsafe path promoted to positive behavior is rejected |
| Rail separation | DAP never fills an LSP row; alternate clients never fill each other | opt.06/opt.07; F13 | cross-rail satisfaction mutation is rejected |
| Profile erasure | Stronger profile never erases narrower valid one; optionals never block core | profile laws; opt.* rows | breadth-as-blocker mutation is rejected |
| Mutable leakage | No live SHA/PR/check/writer/wall-clock state in durable bytes | all three files | live-state injection scan fails closed |
| Determinism | Same tree yields identical checker output twice | `checklist.md` proof | second run byte-clean |

## §Contracts

| Contract | Authority consumed | How this bundle satisfies it |
| --- | --- | --- |
| Checked spec directory shape | [`SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md), #3983 | canonical three-file packet; ledger evolution recorded in `context.md` |
| Exact subject/config/public-surface substrate | #11369 merged as PR #12050 (`.ci/editor-clients/vim-vim-lsp-{subject,configuration,public-surface}.v1.json`) | every behavioral row traces to these artifacts or a named authority; no second pin |
| Executable identity | #7691/#7760 via #12050 command law | attach.03 wording binds `perllsp --stdio` |
| Filetype/root sole authority | #7762 | attach.01/attach.04 consume by reference; no marker copy |
| Workspace-field admission | #6736 catalog | edit.03 admits only catalog fields whose sources include workspace configuration |
| Include-path security | #4998 | edit.03/edit.04 positive shape workspace-contained relative; edit.05 guard row |
| Generic receipt semantics | #10527/#7777; generic `editor_client_compat.v1` cells | receipt stage consumed as boundary owner; no Vim-local receipt ontology |
| Typed edge/profile vocabulary | #10858 | five programme profiles defined with membership rules, ceilings, `consumes_if_available` class |
| Host reliability/cleanup | #10894 (generic), #8734-class conformance downstream | lifecycle.05 requires independent cleanup observation |
| Support registry tiers | `policy/lsp-client-support.toml`, projection owners #10974/#7122 | tags reuse registered tier vocabulary; no tier changes here |
| Fixture ownership | #10938 | fixture/oracle cells bind IDs downstream; zero fixture bytes here |
| Sibling precedent | `.spec/11716-emacs-support-architecture/`, `.spec/11709-zed-integration-architecture/` | same checked discipline; different programme, no overlap |

## §API-Shape

No Rust or public API is introduced. Semantic contract terms defined here:

| Item | Kind | Shape | Dup-risk / owner |
| --- | --- | --- | --- |
| `vim.bdd.<family>.<nn>` | stable scenario ID namespace | 30 IDs, fixed families/order, immutable once published | none found on main; this packet |
| `vim_actual_client_core` | claim profile ID | membership = the 23 baseline rows | none found; #11371 |
| `vim_first_class_exact_source` | claim profile ID | core + exact-source evidence family | none found; #11371 |
| `vim_public_artifact` | claim profile ID | exact source + #10970 replay | none found; #11371 |
| `vim_programme_closeout` | claim profile ID | programme fan-in | none found; #11371 |
| `vim_configuration_documented` | claim profile ID | aligns with registered tier name | tier pre-exists in registry; profile binding is new here |

## §Test-Grid

All thirteen issue falsifiers, fixed order. Each is a design-level negative
control: a candidate implementation, fixture, driver, receipt, or projection
is conformant only if every mutation fails deterministically in that leaf's
own negative controls.

| # | Scenario | Kind | Required verdict |
|---:|---|---|---|
| 1 | Wrong sibling/outer root returns the same symbol spelling and passes as root-correct | negative | reject; root-sensitive answers require the governed root (#7762) |
| 2 | Unrelated diagnostic exists while the expected one is absent, presented as pass | negative | reject; the expected diagnostic itself must appear (attach.06) |
| 3 | Completion response exists but vim-lsp did not apply/consume it | negative | reject; consumption through the client is the proposition (nav.01–02) |
| 4 | Literal snippet placeholders survive in the no-snippet buffer | negative | reject; final plain text must be correct (nav.03) |
| 5 | Hover/navigation result is non-empty but semantically wrong | negative | reject; identity of the answered entity is the proposition (nav.04–06) |
| 6 | Rename changes only some occurrences or touches the decoy root | negative | reject; exactly-the-intended-occurrences/files (edit.01) |
| 7 | Format request returns but actual buffer state is wrong or unchanged | negative | reject; canonical buffer result is the proposition (edit.02) |
| 8 | Configuration object exists but has no independent effect | negative | reject; independent semantic change required (edit.04) |
| 9 | Stale pre-edit result appears after an accepted edit | negative | reject; accepted-generation currentness (lifecycle.03) |
| 10 | Non-BMP request lands on an adjacent range | negative | reject; intended-target resolution (lifecycle.01) |
| 11 | Server capability or synthetic peer used instead of actual client traffic | negative | reject; only actual vim-lsp traffic satisfies actual-host rows |
| 12 | Client exit event occurs while perllsp survives | negative | reject; shutdown leaves no bound process (lifecycle.05) |
| 13 | Another client, build, platform, or evidence stage substituted for the pinned subject/stage | negative | reject; subject and stage non-substitution (F1–F12 substrate) |

## §Blast-Radius

| Consumer / surface | Impact | Required update |
| --- | --- | --- |
| #10938 fixture/oracle cells | Binds these IDs when authoring fixtures | Consume IDs; do not redefine propositions |
| #10944/#10946/#10951/#10955/#10958 host leaves | Observe against named scenarios | Bind IDs into raw observations |
| Generic `editor_client_compat.v1` producers | Cell provenance cites scenario IDs | Reference only; schema unchanged |
| #10962 fan-in / #10974 / #7122 / #10978 | Downstream projections cite stable IDs | Reference only |
| `policy/lsp-client-support.toml` | No change in this PR | Future Vim rows flow via #10974/#7122 once chains pass |
| Generated status/docs surfaces | None exist for this ledger today | Any future generator must derive from these IDs |
| Product/runtime crates | None | Must-not-touch |

Must-not-touch: `crates/`, `xtask/`, `.github/workflows/`, fixture sources,
host harnesses, receipts, support registry values, docs prose, external
upstream surfaces.

## §Coverage-Map

| #11371 acceptance item | Covered by |
| --- | --- |
| One checked BDD contract without implementation trivia | §Behavior wording rules + `context.md` keep-distinct split |
| Every baseline scenario has a stable consumable ID | §Behavior ID columns; checker enforces the exact ID set/order |
| False greens explicit across all seven demanded areas | §Test-Grid rows 1–13 fixed order |
| Optional/stronger/public/external/DAP stay separate | opt.* table + profile laws + rail-separation hazard |
| Stages/subjects cannot cross-inherit | evidence-chain owners + stage-promotion hazard + F11/F13 |
| Baseline versus optional/stronger table | profile membership tables |
| Authorities/security boundaries named | §Contracts; security boundary section |
| Generated outputs current/deterministic | `checklist.md` two-run structural proof (no generator exists; recorded) |
| No fixture/host/receipt/support/public claim created | scope boundary + blast radius + claim boundary below |

## Scope, rollback, and proof claims

- **In scope:** exactly the three files of `.spec/11371-vim-bdd-journeys/`.
- **Rollback:** revert this bundle's commit; issues retain full authority.
  Any downstream artifact already bound to these IDs reverts through its own
  owner, never by editing this bundle silently.
- **Transfer:** only with exact current evidence inventory and named
  receiving owner; otherwise `not_proven`.
- **Stop:** return to #11371 if a boundary above would need weakening to make
  a downstream check green, or current main contradicts a material decision.
- **Claim boundary:** proves a durable checked journey/evidence contract and
  deterministic structural inspection only. Proves no Vim behavior, host
  execution, fixture correctness, receipt, support tier, public artifact, or
  upstream state. All 30 scenarios remain `not_proven` as behavior until
  their executable exact-host chains pass under their owning leaves.
