# Acceptance Criteria: #10888 — native Neovim LSP user journeys and evidence boundaries

This is a checked, declarative BDD/spec-ledger contract. It implements no
fixture, host driver, provisioning, Lua, server/client behavior, receipt,
support tier, or generated-status machinery. Executable truth for every row
below is owned by the downstream leaves named in its evidence boundary.

Subject law: every row below is about **Neovim's built-in LSP client**
(`vim.lsp.config` / `vim.lsp.enable`) driving `perllsp --stdio`. Vim + vim-lsp
(`vim.bdd.*`), Coc on Neovim (`coc.neovim.bdd.*`), and Lite XL
(`lite_xl.bdd.*`) are different subjects and can never satisfy a row here.

## §Behavior — bounded core journey ledger (`native_neovim_core`)

Normative wording is from the user's/editor's observable perspective. Exact
Lua names, autocommands, polling mechanics, JSON fields, hashes, and paths
belong to #10502 and the later host leaves. Profile column: membership in
`native_neovim_core` (all 16 rows) plus named substrate prerequisites.

### Feature: Neovim attaches the built-in LSP client to the intended Perl project

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `neovim.bdd.attach.01` | An ordinary Perl buffer activates the Perl language server through Neovim's native Perl filetype detection, before any user override applies | core; actual host required; subject = Neovim built-in LSP | #10502 cell → #10504 observation → `editor_client_compat.v1` cell → #10508 rows → #10522/#7122 support |
| `neovim.bdd.attach.02` | The attached client is exactly Neovim's built-in LSP client, enabled by the documented native configuration; a plugin-provided Perl client does not satisfy attachment | core; actual host required; exact-subject binding | same chain; subject pin owned by #10502 (no artifact on main today) |
| `neovim.bdd.attach.03` | The session runs the Perl language server as exactly `perllsp --stdio` | core; actual host required; executable identity law #7691 | same chain |
| `neovim.bdd.attach.04` | The project root the server answers from is the root selected by the governed root contract for the opened buffer's project | core; actual host required | same chain; root authority #7762/#10502 |
| `neovim.bdd.attach.05` | A same-named sibling or enclosing project outside the governed root never satisfies a root-sensitive answer | core; actual host required | same chain + #10502 false-subject mechanics |
| `neovim.bdd.attach.06` | A single Perl file with no project marker resolves to the explicit documented disposition rather than an invented root, and activation alone claims no semantic support for POD, XS, or template buffers | core; actual host required; boundary row | same chain; disposition owned by #10502 |

### Feature: Neovim returns useful current core facts

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `neovim.bdd.core.01` | After opening a defective Perl buffer, the expected diagnostic becomes visible through the actual client diagnostic state | core; actual host required | #10502 cell → #10504 observation → `editor_client_compat.v1` cell → #10522/#7122 support |
| `neovim.bdd.core.02` | Editing away the defect replaces or clears exactly that diagnostic for the current document, rather than leaving a stale entry | core; actual host required | same chain |
| `neovim.bdd.core.03` | Completion at a real code target offers the expected server item, and accepting it applies the intended final buffer text | core; actual host required | same chain |
| `neovim.bdd.core.04` | Hover identifies the intended entity at the cursor | core; actual host required | same chain |
| `neovim.bdd.core.05` | Definition resolves the intended entity of this project, opening the intended target content under the governed root | core; actual host required | same chain |
| `neovim.bdd.core.06` | References list contains the governed sites of this project and excludes wrong-root decoy matches | core; actual host required | same chain + #10502 decoys |
| `neovim.bdd.core.07` | Server-native settings supplied through the documented native `settings.perl` table change an independent semantic result (include-path resolution), not merely the settings echo | core; actual host required; security-sensitive configuration | same chain; field admission owned by #6736 catalog |
| `neovim.bdd.core.08` | Formatting brings the buffer to the canonical formatted bytes through client-applied edits, and a second format changes nothing further | core; actual host required | same chain |
| `neovim.bdd.core.09` | Normal Neovim quit leaves no bound `perllsp` process behind | core; actual host required; cleanup observed independently | same chain; cleanup mechanics #10507 |
| `neovim.bdd.core.10` | Absolute or traversal include-path settings remain outside ordinary positive behavior: rejected or governed, never silently applied as a trusted channel | core (negative/guard); security-sensitive configuration | same chain; rejection semantics owned by #4998 |

## §Behavior — conditional text-sync envelope (`#8129` selects exactly one branch)

Both branch groups are published as stable IDs so the selected branch has an
owner the moment #8129 rules. Publishing both does **not** claim both: exactly
one group becomes applicable, the other becomes `not_applicable` by that
ruling. Neither group is current until #8129 selects it and #10505 observes it.

### Branch A — `atomic_incremental`

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `neovim.bdd.sync.01` | The advertised position encoding and change-sync kind match the selected atomic-incremental envelope | deep; conditional on #8129 selecting branch A | #8129 ruling → #10505 branch observation → `editor_client_compat.v1` cell |
| `neovim.bdd.sync.02` | Actual ranged change traffic from the built-in client is observed, not merely advertised | deep; conditional branch A | same chain |
| `neovim.bdd.sync.03` | Sequential multi-change edits apply in order and leave the server on the intended final text | deep; conditional branch A | same chain |
| `neovim.bdd.sync.04` | An invalid change notification desynchronizes explicitly instead of silently accepting partial state | deep; conditional branch A | same chain |
| `neovim.bdd.sync.05` | Explicit full-source resend or document reopen recovers a desynchronized document | deep; conditional branch A | same chain |

### Branch B — `full_document_utf16`

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `neovim.bdd.sync.06` | The advertised position encoding is UTF-16 and the change-sync kind matches the selected full-document envelope | deep; conditional on #8129 selecting branch B | #8129 ruling → #10505 branch observation → `editor_client_compat.v1` cell |
| `neovim.bdd.sync.07` | Actual full-document change traffic from the built-in client is observed, not merely advertised | deep; conditional branch B | same chain |
| `neovim.bdd.sync.08` | A ranged change notification is refused under the full-document envelope rather than partially applied | deep; conditional branch B | same chain |
| `neovim.bdd.sync.09` | A refused notification leaves no partial mutation of server document state | deep; conditional branch B | same chain |
| `neovim.bdd.sync.10` | Explicit full-source resend or document reopen recovers a desynchronized document | deep; conditional branch B | same chain |

## §Behavior — deep lifecycle truth (`native_neovim_deep_lifecycle`)

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `neovim.bdd.lifecycle.01` | An operation issued after non-BMP text lands on the intended range, not an adjacent one | deep; actual host required | #10506 cell → #10507 observation → `editor_client_compat.v1` cell |
| `neovim.bdd.lifecycle.02` | An executable edit, a data-only edit, and a marker-boundary fallback remain distinguishable in the answer the user sees | deep; actual host required | #10506 owner chain |
| `neovim.bdd.lifecycle.03` | A complete-artifact hit preserves its recovery state and declared limitations rather than presenting as unqualified success | deep; actual host required | #10506 owner chain |
| `neovim.bdd.lifecycle.04` | An admitted parse strategy and a full fallback remain distinct in what the user is told | deep; actual host required | #10506 owner chain |
| `neovim.bdd.lifecycle.05` | A document whose initial open is still pending is not presented as current | deep; actual host required | #10507 owner chain |
| `neovim.bdd.lifecycle.06` | A superseded parse or effect never publishes over a newer accepted generation | deep; actual host required | #10507 owner chain |
| `neovim.bdd.lifecycle.07` | Closing and reopening the same URI with identical version and bytes creates a new document instance and restores no stale state; shutdown releases the exact owned work and state, not merely the process | deep; actual host required | #10507 owner chain |

## §Behavior — support claims remain subject- and stage-bound (`native_neovim_first_class`)

These are separation laws, not user gestures. Each states a distinctness that
any downstream projection must preserve.

| Scenario ID | Proposition | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `neovim.bdd.support.01` | The maintained support floor (Neovim 0.11.3, per `docs/EDITORS/NEOVIM_SETUP.md`) and the current stable Neovim version are separate rows; neither substitutes for the other, and no receipt certifies a broad `0.11+` matrix | distribution; stage law | #10508 version/platform rows; exact version cells owned by #7716 |
| `neovim.bdd.support.02` | Linux, macOS, and Windows remain separate rows | distribution; stage law | #10508 |
| `neovim.bdd.support.03` | Manual native configuration, upstream `nvim-lspconfig` registration, and Mason availability remain separate rows | distribution; stage law | #10516/#10518/#10520 install channels |
| `neovim.bdd.support.04` | Release-archive, Cargo, Homebrew, and Mason installations remain separate rows | distribution; stage law | #10516/#10518/#10520 |
| `neovim.bdd.support.05` | Exact-source behavior is never public-installed proof | distribution; stage law | #10508 + #10522/#7122 projection |
| `neovim.bdd.support.06` | A prepared external packet is not submitted, accepted, released, or publicly available; each remains its own stage | distribution; stage law | #10511/#10514 external packet stages |
| `neovim.bdd.support.07` | Virtual-document behavior may remain upstream-dependent without failing core support | distribution; optional-dependency law | #10522/#7122; never a core blocker |
| `neovim.bdd.support.08` | DAP is a nonblocking preview sidecar; a DAP result never satisfies an LSP row and never blocks or promotes LSP support | distribution; rail separation | #10523 DAP sidecar exclusion |

## §Behavior — optional and stronger-profile inputs (never baseline blockers)

| Scenario ID | Proposition | Profile relation | Boundary note |
| --- | --- | --- | --- |
| `neovim.bdd.opt.01` | Upstream `nvim-lspconfig` ships the registration | external checkpoint input | submission/acceptance stages owned by #10511/#10514; internal merge cannot satisfy them |
| `neovim.bdd.opt.02` | Mason publishes an installable package | external channel input | owner #10520; availability never a core floor |
| `neovim.bdd.opt.03` | Public release archive replay reproduces the journeys | stronger-profile input | direct stage owner #10508; local evidence never relabels upward |
| `neovim.bdd.opt.04` | Maintained version/platform matrix rows hold beyond the support floor | stronger-profile input | rows owned by #10508 |
| `neovim.bdd.opt.05` | Virtual-document / upstream-dependent features work end-to-end | `consumes_if_available` input | existence cannot block or satisfy the core |
| `neovim.bdd.opt.06` | `nvim-dap` debugging works against `perl-dap` | separate protocol rail | owner #10523; DAP evidence never fills an LSP scenario |

## Claim profiles (ledger membership)

| Profile | Membership rule | Ceiling |
| --- | --- | --- |
| `native_neovim_configuration` | documented native setup only (`docs/EDITORS/NEOVIM_SETUP.md`) | substrate only; proves no behavior; matches the registered `neovim` tier |
| `native_neovim_core` | exactly the 16 rows of `attach.*` + `core.*` | bounded core; nothing else blocks or widens it |
| `native_neovim_deep_lifecycle` | core + `lifecycle.01–07` + the one `sync.*` branch selected by #8129 | never a prerequisite of core |
| `native_neovim_first_class` | deep + `support.01–08` stage laws | public stages require their own direct evidence |
| `release_v0_18_bounded` | the one `sync.*` branch selected by `nv_release_scope_decision` plus only the cells the bounded public claim requires | selection is re-evaluated whenever either branch materially lands; a stale selection fails closed |
| `native_neovim_programme_closeout` | fan-in over independently terminal child propositions | composes child results only; manufactures none |

Laws: a stronger profile never erases a narrower valid one; optional rows are
`consumes_if_available` only; missing chain links are `not_proven`; no scenario
supports any tier until its executable exact-host chain passes; only Neovim
built-in LSP receipts satisfy `neovim.bdd.*`.

## §Hazards

| Class | Invariant | Surface | Required adversarial check |
| --- | --- | --- | --- |
| Subject identity substitution | Only Neovim built-in LSP + `perllsp --stdio` satisfies an actual-host row | attach.02/attach.03; F11/F13 | Vim, Coc-on-Neovim, Lite XL, or plugin-client substitution is rejected |
| Stage promotion | Fixture, observation, receipt cell, version rows, support projection stay distinct owners | every evidence-boundary cell; F14/F18 | receipt or projection presented as host observation is rejected |
| Root conflation | Governed root wins; siblings/outer projects never satisfy root-sensitive rows | attach.04/attach.05; F2 | wrong-sibling-root answer passing as correct is rejected |
| Activation forgery | Filetype must be observed, not pre-set by the harness | attach.01; F1 | pre-set `filetype` before observation is rejected |
| Currentness forgery | Post-edit answers belong to the accepted generation | core.02/lifecycle.06; F4/F10 | stale pre-edit result after an accepted edit is rejected |
| Range adjacency | Non-BMP offsets resolve to intended targets only | lifecycle.01 | adjacent-range landing mutation is rejected |
| Application honesty | Client must actually apply server results | core.03/core.08; F5/F9 | response-exists-but-not-applied mutation is rejected |
| Configuration theater | Settings must have independent semantic effect | core.07; F8 | settings-object-without-effect mutation is rejected |
| Security boundary | Workspace-contained relative include paths only; absolute/traversal stays governed/rejected | core.10; #4998; F24 | unsafe path promoted to positive behavior is rejected |
| Profile collapse | Core, deep lifecycle, and distribution stay separately closable | profile table; F16 | deep proposition used as a core prerequisite is rejected |
| Conditional-envelope honesty | Exactly one #8129 branch is applicable; publishing both claims neither | sync.* groups; F17 | both-branches-current or no-branch-owner is rejected |
| Rail separation | DAP never fills an LSP row | support.08/opt.06; F15/F22 | cross-rail satisfaction mutation is rejected |
| Weak oracle | A boolean/non-null observation is not a semantic result | core.04/core.05; F6/F7/F25 | non-null-but-wrong answer passing as correct is rejected |
| Mutable leakage | No live SHA/PR/check/writer/wall-clock state in durable bytes | all three files | live-state injection scan fails closed |
| Determinism | Same tree yields identical checker output twice | `checklist.md` proof | second run byte-clean |

## §Contracts

| Contract | Authority consumed | How this bundle satisfies it |
| --- | --- | --- |
| Checked spec directory shape | [`SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md), #3983 | canonical three-file packet; ledger evolution recorded in `context.md` |
| Documented native configuration | `docs/EDITORS/NEOVIM_SETUP.md` (`vim.lsp.config`/`vim.lsp.enable`, `cmd = { 'perllsp', '--stdio' }`) | attach/core wording matches the documented native shape; no nvim-lspconfig notation assumed |
| Executable identity | #7691 | attach.03 wording binds `perllsp --stdio` |
| Filetype/root authority | #7762, envelope owner #10502 | attach.01/attach.04 consume by reference; no marker copy |
| Workspace-field admission | #6736 catalog | core.07 admits only catalog fields whose sources include workspace configuration |
| Include-path security | #4998 | core.07 positive shape workspace-contained relative; core.10 guard row |
| Generic receipt semantics | #10527/#7777; generic `editor_client_compat.v1` cells | receipt stage consumed as boundary owner; no Neovim-local receipt ontology |
| Typed edge/profile vocabulary | #10858 | five programme profiles defined with membership rules, ceilings, `consumes_if_available` class |
| Host reliability/cleanup | #10894 (generic), #10507 downstream | core.09/lifecycle.07 require independent cleanup observation |
| Text-sync envelope selection | #8129 | sync.* published as two conditional groups; neither asserted current |
| Support registry tiers | `policy/lsp-client-support.toml` registered `neovim` tier (`configuration_documented`, `requires_actual_client_receipt = true`) | tags reuse registered vocabulary; no tier change here |
| Programme topology | #7739 product controller, #10501 train controller, `.spec/11392-native-neovim-train-graph/` | consumed by reference; no manifest node added or renamed |
| Sibling precedent | `.spec/11371-vim-bdd-journeys/`, `.spec/10815-coc-bdd-journeys/`, `.spec/11178-lite-xl-bdd-journeys/` | same checked discipline; different subjects, no overlap |

## §API-Shape

No Rust or public API is introduced. Semantic contract terms defined here:

| Item | Kind | Shape | Dup-risk / owner |
| --- | --- | --- | --- |
| `neovim.bdd.<family>.<nn>` | stable scenario ID namespace | 47 IDs, fixed families/order, immutable once published | none found on main; this packet |
| `native_neovim_configuration` | claim profile ID | documented native setup substrate | aligns with registered `neovim` tier name; binding is new here |
| `native_neovim_core` | claim profile ID | membership = the 16 attach/core rows | none found; #10888 |
| `native_neovim_deep_lifecycle` | claim profile ID | core + lifecycle + selected sync branch | none found; #10888 |
| `native_neovim_first_class` | claim profile ID | deep + support stage laws | none found; #10888 |
| `native_neovim_programme_closeout` | claim profile ID | programme fan-in | none found; #10888 |

## §Test-Grid

All twenty-five falsifiers in fixed order. F1–F15 are the controlling issue's
false-green classes in its own enumeration order; F16–F25 are its
spec-check laws in its own enumeration order. Each is a design-level negative
control: a candidate fixture, driver, receipt, or projection is conformant only
if every mutation fails deterministically in that leaf's own negative controls.

| # | Scenario | Kind | Required verdict |
|---:|---|---|---|
| 1 | Native filetype was pre-set before observation | negative | reject; activation must be observed, not arranged (attach.01) |
| 2 | Wrong parent/sibling root contains an equivalent symbol and passes as root-correct | negative | reject; root-sensitive answers require the governed root (attach.04–05) |
| 3 | Unrelated diagnostic exists while the required diagnostic is absent | negative | reject; the expected diagnostic itself must appear (core.01) |
| 4 | Diagnostic fingerprint changes but the defect remains current | negative | reject; clearing requires the defect to be gone (core.02) |
| 5 | Any completion item exists but the expected item or its application is absent | negative | reject; the intended item and applied text are the proposition (core.03) |
| 6 | Hover is non-null but semantically empty or about another symbol | negative | reject; identity of the answered entity is the proposition (core.04) |
| 7 | Definition request succeeds but opens the wrong target | negative | reject; intended target content under the governed root (core.05) |
| 8 | `workspace/configuration` appears in a trace but the setting has no behavior effect | negative | reject; independent semantic change required (core.07) |
| 9 | Formatting returns but bytes remain wrong or a second run changes again | negative | reject; canonical bytes and idempotence (core.08) |
| 10 | Previous-generation fact satisfies a post-edit query | negative | reject; accepted-generation currentness (core.02/lifecycle.06) |
| 11 | Capability is advertised but actual request or application is unobserved | negative | reject; only actual built-in-client traffic satisfies actual-host rows |
| 12 | Forced process kill is recorded as graceful shutdown | negative | reject; normal quit leaves no bound process (core.09) |
| 13 | One version, platform, or channel receipt is substituted for another | negative | reject; stage and subject non-substitution (support.01–04) |
| 14 | Exact-source or local packet is promoted to public distribution | negative | reject; public stages need their own evidence (support.05–06) |
| 15 | A DAP result satisfies an LSP scenario | negative | reject; rail separation (support.08/opt.06) |
| 16 | Core and deep profiles are collapsed into one definition of done | negative | reject; profiles close independently (profile table) |
| 17 | Both #8129 branches are active, or neither branch owner is represented | negative | reject; exactly one branch applies once #8129 rules (sync.*) |
| 18 | An actual-host scenario carries no executable evidence owner | negative | reject; every row names a downstream owner chain |
| 19 | One version, platform, or channel is allowed to satisfy another | negative | reject; separation laws (support.01–04) |
| 20 | A scenario ID is absent from the fixture, receipt, or support mapping | negative | reject; the scenario→consumer map must be total |
| 21 | An optional or upstream-dependent feature is made baseline | negative | reject; optionals are `consumes_if_available` (opt.*) |
| 22 | DAP enters the LSP blocking profile | negative | reject; sidecar never blocks or promotes core (support.08) |
| 23 | Generated feature or status output is stale | negative | reject; no generator exists on main, so the two-run structural proof discharges this |
| 24 | An unsafe client setting is presented as ordinary positive behavior | negative | reject; governed/rejected per #4998 (core.10) |
| 25 | A semantic scenario is satisfied by a boolean-only observation | negative | reject; semantic identity is the proposition, not liveness |

## §Blast-Radius

| Consumer / surface | Impact | Required update |
| --- | --- | --- |
| #10502 fixture/oracle and activation-root cells | Binds these IDs when authoring fixtures and the subject artifact | Consume IDs; do not redefine propositions |
| #10504/#10505/#10506/#10507 host leaves | Observe against named scenarios | Bind IDs into raw observations |
| Generic `editor_client_compat.v1` producers | Cell provenance cites scenario IDs | Reference only; schema unchanged |
| #10508 version/platform rows; #10516/#10518/#10520 channels | Cite `support.*` separation laws | Reference only |
| #10511/#10514 external packet stages; #10522/#7122 projection | Downstream projections cite stable IDs | Reference only |
| #10523 DAP sidecar | Exclusion law cited | Reference only |
| `policy/lsp-client-support.toml` | No change in this PR | Future Neovim rows flow via #10522/#7122 once chains pass |
| `.spec/11392-native-neovim-train-graph/train.manifest.json` | No node added, renamed, or retired | None; validator does not read `.spec/` trees |
| `docs/policy/NON_RUST_INVENTORY.md` | Three new tracked docs rows once committed | None here; regeneration is owned by the post-merge `non-rust inventory --write` job, and the committed snapshot is already stale on main under #14203/#14161 |
| Generated status/docs surfaces | None exist for this ledger today | Any future generator must derive from these IDs |
| Product/runtime crates | None | Must-not-touch |

Must-not-touch: `crates/`, `xtask/src/`, `.github/workflows/`, `.ci/`,
`docs/`, fixture sources, host harnesses, receipts, support registry values,
and external upstream surfaces.

## §Coverage-Map

| #10888 acceptance item | Covered by |
| --- | --- |
| One checked BDD contract describing core, deep lifecycle, and distribution profiles separately | profile membership table + `context.md` keep-distinct split |
| Atomic-incremental and full-document UTF-16 branches conditional on #8129 rather than simultaneously claimed | conditional sync.* groups + F17 |
| Every downstream Neovim leaf owns stable scenario IDs | §Behavior ID columns; checker enforces the exact ID set/order |
| Semantic usefulness/currentness and learned false-green classes explicit | §Test-Grid F1–F15 in the issue's fixed order |
| Version, platform, install, external, virtual-document, DAP boundaries independent | support.01–08 + opt.* + rail-separation hazard |
| Scenario IDs map to fixture, receipt, and support cells without broadening | evidence-boundary columns + §Blast-Radius + F20 |
| Generated feature/status outputs current and deterministic | `checklist.md` two-run structural proof (no generator exists; recorded) |
| No implementation, host run, release decision, external write, or support promotion | scope boundary + blast radius + claim boundary below |

## Scope, rollback, and proof claims

- **In scope:** exactly the three files of `.spec/10888-neovim-bdd-journeys/`.
- **Rollback:** revert this bundle's commit; issues retain full authority. Any
  downstream artifact already bound to these IDs reverts through its own owner,
  never by editing this bundle silently.
- **Transfer:** only with exact current evidence inventory and named receiving
  owner; otherwise `not_proven`.
- **Stop:** return to #10888 if a boundary above would need weakening to make a
  downstream check green, or current main contradicts a material decision.
- **Claim boundary:** proves a durable checked journey/evidence contract and
  deterministic structural inspection only. Proves no Neovim behavior, host
  execution, fixture correctness, receipt, support tier, public artifact, or
  upstream state. All 47 scenarios remain `not_proven` as behavior until their
  executable exact-host chains pass under their owning leaves.
