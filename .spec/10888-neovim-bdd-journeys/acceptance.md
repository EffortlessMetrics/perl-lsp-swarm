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
| `neovim.bdd.attach.01` | An ordinary Perl buffer activates the Perl language server through Neovim's native Perl filetype detection, before any user override applies | core; actual host required; subject = Neovim built-in LSP | #10502 cell → core slice observation, executed by #10503 `nv_thin_native_host_adapter` → #10504 `nv_core_fanin_exact_subject_receipts` composition (composes terminal children; manufactures nothing) → `editor_client_compat.v1` cell → #10508 rows → #10522/#7122 support |
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

Both branch groups are published as stable IDs because the #8129 selection can
move and its IDs must already exist when it does. Publication is not a claim of
ownership and not a claim of truth: exactly one group becomes applicable, the
other becomes `not_applicable` by that ruling, and applicability is a separate
question from whether any owner can supply the evidence.

Branch A has a governed behavior owner, #10505 `nv_deep_atomic_branch`, and
becomes current once #8129 selects it and #10505 observes it.

Branch B has **no governed behavior-observation owner in #11392**. #8531
`nv_release_bounded_process_evidence` assembles process/host evidence and by its
own ceiling never flips behavior propositions;
`nv_release_bounded_v0_18_envelope` composes already-typed cells only; and no
other node in the graph observes full-document synchronization. Selecting
`full_document_utf16` therefore makes branch B *applicable* without making it
*provable*: all five rows stay `not_proven` as behavior until #11392 adds such
an owner. Selection confers applicability, never evidence. Neither branch's
owner observes the other.

### Branch A — `atomic_incremental`

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `neovim.bdd.sync.01` | The advertised position encoding and change-sync kind match the selected atomic-incremental envelope | deep (atomic branch); active only while #8129 selects branch A | #8129 ruling → #10505 branch observation → `editor_client_compat.v1` cell |
| `neovim.bdd.sync.02` | Actual ranged change traffic from the built-in client is observed, not merely advertised | deep (atomic branch); conditional branch A | same chain |
| `neovim.bdd.sync.03` | Sequential multi-change edits apply in order and leave the server on the intended final text | deep (atomic branch); conditional branch A | same chain |
| `neovim.bdd.sync.04` | An invalid change notification desynchronizes explicitly instead of silently accepting partial state | deep (atomic branch); conditional branch A | same chain |
| `neovim.bdd.sync.05` | Explicit full-source resend or document reopen recovers a desynchronized document | deep (atomic branch); conditional branch A | same chain |

### Branch B — `full_document_utf16`

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `neovim.bdd.sync.06` | The advertised position encoding is UTF-16 and the change-sync kind matches the selected full-document envelope | bounded release (full-document branch); active only while #8129 selects branch B | #8129 ruling → #8531 `nv_release_bounded_process_evidence` supplies process/host evidence only → **no governed behavior-observation owner exists for this branch**, so the `editor_client_compat.v1` behavior cell has no producer and the row is permanently `not_proven` until #11392 adds one → `nv_release_bounded_v0_18_envelope` would compose already-typed cells, adding no behavior cell of its own; #10505 owns the atomic branch and never observes this one |
| `neovim.bdd.sync.07` | Actual full-document change traffic from the built-in client is observed, not merely advertised | bounded release (full-document branch); conditional branch B | same chain |
| `neovim.bdd.sync.08` | A ranged change notification is refused under the full-document envelope rather than partially applied | bounded release (full-document branch); conditional branch B | same chain |
| `neovim.bdd.sync.09` | A refused notification leaves no partial mutation of server document state | bounded release (full-document branch); conditional branch B | same chain |
| `neovim.bdd.sync.10` | Explicit full-source resend or document reopen recovers a desynchronized document | bounded release (full-document branch); conditional branch B | same chain |

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
| `neovim.bdd.support.02` | Linux, macOS, and Windows remain separate rows | distribution; stage law | platform-cell layout #10508; #11392 governs `nv_platform_linux` and `nv_platform_macos` only, so Windows has **no governed platform owner** and is permanently `not_proven` until one exists — it is never satisfied by another platform's evidence |
| `neovim.bdd.support.03` | Manual native configuration, upstream `nvim-lspconfig` registration, and Mason registry availability remain separate rows | distribution; stage law | manual route is `native_neovim_configuration` substrate; #10511 nvim-lspconfig track; #10514 Mason registry track; independence law #7730 |
| `neovim.bdd.support.04` | The stable, nightly, and dev-pin public install channels remain separate rows, and an aggregate installed-binary receipt never substitutes for a per-channel one | distribution; stage law | #7730 channel contract; #10516/#10518/#10520 per channel; #7770 first-mile aggregate |
| `neovim.bdd.support.05` | Exact-source behavior is never public-installed proof | distribution; stage law | #10508 + #10522/#7122 projection |
| `neovim.bdd.support.06` | A prepared external packet is not submitted, accepted, released, or publicly available; each remains its own stage | distribution; stage law | #10511/#10514 external packet stages |
| `neovim.bdd.support.07` | Virtual-document behavior may remain upstream-dependent without failing core support | distribution; optional-dependency law | #10522/#7122; never a core blocker |
| `neovim.bdd.support.08` | DAP is a nonblocking preview sidecar; a DAP result never satisfies an LSP row and never blocks or promotes LSP support | distribution; rail separation | #10523 DAP sidecar exclusion |

## §Behavior — optional and stronger-profile inputs (never baseline blockers)

| Scenario ID | Proposition | Profile relation | Boundary note |
| --- | --- | --- | --- |
| `neovim.bdd.opt.01` | Upstream `nvim-lspconfig` ships the registration | external checkpoint input | submission/acceptance stages owned by #10511; internal merge cannot satisfy them |
| `neovim.bdd.opt.02` | Mason publishes an installable package | external channel input | owner #10514 Mason registry track; availability never a core floor |
| `neovim.bdd.opt.03` | Public release archive replay reproduces the journeys | stronger-profile input | **no governed archive-replay owner exists in #11392**; permanently `not_proven` until one does. #10508 pins version/platform cells only and never supplies replay evidence; local evidence never relabels upward |
| `neovim.bdd.opt.04` | Maintained version/platform matrix rows hold beyond the support floor | stronger-profile input | rows owned by #10508 |
| `neovim.bdd.opt.05` | Virtual-document / upstream-dependent features work end-to-end | `consumes_if_available` input | existence cannot block or satisfy the core |
| `neovim.bdd.opt.06` | `nvim-dap` debugging works against `perl-dap` | separate protocol rail | behavior owned by #7773 `nv_dap_preview_receipt_adjacent`; #10523 is the nonblocking exclusion gate, not the evidence producer; DAP evidence never fills an LSP scenario |

## Claim profiles (ledger membership)

| Profile | Membership rule | Ceiling |
| --- | --- | --- |
| `native_neovim_configuration` | the documented native setup (`docs/EDITORS/NEOVIM_SETUP.md`) together with the canonical filetype/root envelope and settings schema (`nv_config_canonical_root`, `nv_settings_schema_generic`) | substrate only; proves no behavior; matches the registered `neovim` tier |
| `native_neovim_core` | exactly the 16 rows of `attach.*` + `core.*` | bounded core; nothing else blocks or widens it |
| `native_neovim_deep_lifecycle` | core + `lifecycle.01–07` + the atomic branch `sync.01–05` (`nv_deep_atomic_branch`, #10505); the full-document branch never enters this profile, whatever #8129 selects | never a prerequisite of core |
| `native_neovim_first_class` | the distribution and public-stage rows `support.01–08`; #11392's members are the version/platform, install-channel, upstream-track, progressive-support, status-projection and documentation stages, so this profile does **not** require core or deep | public stages require their own direct evidence |
| `release_v0_18_bounded` | the full-document branch `sync.06–10`, which is the branch qualifying under `nv_release_scope_decision`, plus only the cells the bounded public claim requires, which include the exact-subject core receipts (`nv_core_fanin_exact_subject_receipts`) | no current selection is recorded by that authority; the qualifying value on a gate is not a ruling, and a stale selection fails closed |
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
| Typed edge/profile vocabulary | #10858 | the six #11392-governed programme profiles consumed with membership rules, ceilings, `consumes_if_available` class |
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
| `native_neovim_configuration` | claim profile ID | documented native setup, canonical root envelope, settings schema | **governed by #11392**; consumed verbatim, not declared here |
| `native_neovim_core` | claim profile ID | membership = the 16 attach/core rows | **governed by #11392**; consumed verbatim |
| `release_v0_18_bounded` | claim profile ID | the full-document sync branch, which qualifies under `nv_release_scope_decision`, plus the cells the bounded public claim requires | **governed by #11392**; consumed verbatim |
| `native_neovim_deep_lifecycle` | claim profile ID | core + lifecycle + the atomic sync branch | **governed by #11392**; consumed verbatim |
| `native_neovim_first_class` | claim profile ID | the distribution and public-stage rows; not core or deep | **governed by #11392**; consumed verbatim |
| `native_neovim_programme_closeout` | claim profile ID | programme fan-in | **governed by #11392**; consumed verbatim |

### Evidence-stage vocabulary (consumed, never minted)

#10888 requires these distinctions to stay machine-visible. Each is consumed
from an existing surface; this packet mints no Neovim-only verdict scalar and
establishes no Neovim-owned emitter, validator, or adapter. Where the issue's
prose name differs from the shipped wire value, the wire value governs and is
named here so a downstream receipt producer needs no translation table.

| Term | Where it already exists | Disposition for `neovim.bdd.*` |
| --- | --- | --- |
| `configuration_documented` | registered tier in `policy/lsp-client-support.toml` | the current `neovim` tier value; substrate only |
| exact-source actual host | `EvidenceStage::ExactSourceLocal`, wire value `exact_source_local` (`xtask/src/editor_client_compat.rs`) | stage for #10504–#10507 host observations; #10888 names this distinction `exact_source_actual_host` in prose, but the schema value is `exact_source_local` and receipts record that |
| release-candidate actual host | `EvidenceStage::ReleaseCandidate`, wire value `release_candidate` | no Neovim row claims it yet; owner #10508 |
| public-artifact actual host | `EvidenceStage::PublicArtifact`, wire value `public_artifact` | reachable only via `opt.03`; never inherited from exact-source |
| `package_manager_public` | install-channel rows (#10516/#10518/#10520) | `support.03`/`support.04` keep channels separate |
| `external_submission` / `external_acceptance` | external packet stages (#10511/#10514) | `support.06` keeps prepared ≠ submitted ≠ accepted |
| `released_public_availability` | release/support projection (#10522/#7122) | `support.05`; exact-source is never public-installed proof |
| server capability vs client capability | LSP capability exchange, observed by #10505 | advertisement alone never satisfies an actual-host row (F11) |
| actual request/consumption vs applied semantic result | generic host-observation cells | `core.03`/`core.08` require application, not response (F5/F9) |
| default-visible / opt-in available / opt-in behavior proven | generic capability ladder | `opt.*` rows are `consumes_if_available`; availability is not behavior |
| `unsupported` | `ObservationResult::Unsupported`, wire value `unsupported` | terminal for a row the subject cannot express |
| `upstream_dependency` | generic limitation vocabulary | `support.07`; may hold without failing core |
| `not_proven` | `ObservationResult::NotProven`, wire value `not_proven` | the default state of all 47 rows in this packet |
| `instrument_failed` | a **receipt-level failure class** (#7777), rendered `instrument_failed(<class>)`; it is **not** an `ObservationResult` variant | measurement-surface failure, never silently product failure; a cell records the limitation token `instrument_incomplete` and the receipt carries the failure class |

Generic vocabulary only. A downstream leaf records an allowed schema value plus
limitation text until an owner proves the mapping.

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
| `docs/policy/NON_RUST_INVENTORY.md` | Three new tracked docs rows | Regenerated here by `cargo xtask non-rust inventory --write`, which the `policy` shard's `non_rust_inventory_check` gate requires; the run also carries one unrelated row already stale on main under #14203/#14161 |
| Generated status/docs surfaces | None exist for this ledger today | Any future generator must derive from these IDs |
| Product/runtime crates | None | Must-not-touch |

Must-not-touch: `crates/`, `xtask/src/`, `.github/workflows/`, `.ci/`,
hand-edits to any generated file, other `docs/` prose, fixture sources, host
harnesses, receipts, support registry values, and external upstream surfaces.

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

- **In scope:** the three files of `.spec/10888-neovim-bdd-journeys/`, plus the
  generated `docs/policy/NON_RUST_INVENTORY.md` refresh those tracked files
  require. The inventory is generated output, never hand-edited.
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
