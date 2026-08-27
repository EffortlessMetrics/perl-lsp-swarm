# Acceptance Criteria: #10815 — canonical Coc user journeys and evidence boundaries

This is a checked, declarative BDD/spec-ledger contract. It implements no
fixture, host driver, provisioning, server/client behavior, receipt, support
tier, or generated-status machinery. Executable truth for every row below is
owned by the downstream leaves named in its evidence boundary.

## §Behavior — baseline journey ledger (`coc_actual_client_core`)

Normative wording is from the user's/editor's observable perspective. Exact
coc.nvim setting names, JSON fields, Node process mechanics, hashes, and paths
belong to #10674 and later leaves; exact subject bytes belong to #8956 once
landed. Profile column: membership in `coc_actual_client_core` (all 42 rows,
addressable per host rail) plus named substrate prerequisites. Host identity is
load-bearing: a Vim + coc.nvim row is never satisfied by a Neovim + coc.nvim
observation, and vice versa.

### Feature: Vim + coc.nvim attaches to the intended Perl project

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.vim.bdd.attach.01` | An ordinary Perl buffer is detected through Vim's native Perl filetype detection before any override applies | core; actual host required; subject = Vim + coc.nvim | #10674 cell → #10685-lineage observation → editor_client_compat.v1 cell → #8962 convergence → #8992 fan-in → #7122/#8992 support projection |
| `coc.vim.bdd.attach.02` | The session runs behind the exact coc.nvim subject pinned by the governed subject contract (#8956, pin authority open); a substitute build or copy does not satisfy attachment | core; actual host required; exact-source evidence | same chain, exact-subject binding re-checked at #8956 landing |
| `coc.vim.bdd.attach.03` | The exact configured Coc service launches the Perl language server as exactly `perllsp --stdio` | core; actual host required; executable identity law #8956 | same chain |
| `coc.vim.bdd.attach.04` | The project root the server answers from is the nested Perl project selected by #8956's one root contract for the opened buffer | core; actual host required | same chain |
| `coc.vim.bdd.attach.05` | A same-named sibling or enclosing project outside the governed root never satisfies a root-sensitive answer | core; actual host required | same chain + #10674 false-subject mechanics |
| `coc.vim.bdd.attach.06` | After opening a defective Perl buffer, the expected diagnostic becomes visible through the actual client diagnostic state | core; actual host required | same chain |
| `coc.vim.bdd.attach.07` | Editing the defective source updates the visible diagnostic to the current state | core; actual host required | same chain |

### Feature: Vim + coc.nvim provides ordinary navigation and completion

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.vim.bdd.nav.01` | Completion at a real code target selects an item offered by the language server through coc.nvim | core; actual host required | #10674 cell → rail observations → editor_client_compat.v1 → #8962 → #8992 → #7122/#8992 support projection |
| `coc.vim.bdd.nav.02` | A snippet-capable completion item applies through Coc without leaving literal placeholder text in the buffer | core; actual host required | same chain |
| `coc.vim.bdd.nav.03` | Hover identifies the intended symbol at the cursor | core; actual host required | same chain |
| `coc.vim.bdd.nav.04` | Definition resolves the intended project entity of this project | core; actual host required | same chain |
| `coc.vim.bdd.nav.05` | References identify the declared sites of this project rather than same-name false subjects | core; actual host required | same chain + #10674 decoys |

### Feature: Vim + coc.nvim applies server edits and configuration safely

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.vim.bdd.edit.01` | A code action yields an explicit applied / disabled / no-applicable disposition in the editor, not a silent unknown | core; actual host required | #10674 cell → rail observations → editor_client_compat.v1 → #8962 → #8992 → #7122/#8992 support projection |
| `coc.vim.bdd.edit.02` | Rename applies the complete intended workspace edit only, across exactly this project's intended files | core; actual host required | same chain |
| `coc.vim.bdd.edit.03` | Formatting reaches the canonical result through client-applied edits, and repeating it changes no further content (idempotent) | core; actual host required | same chain |
| `coc.vim.bdd.edit.04` | A relative workspace include path affects resolution within the intended root | core; configuration documented substrate; security-sensitive configuration | same chain |
| `coc.vim.bdd.edit.05` | Absolute or traversal client include paths remain governed/rejected per #4998 rather than assumed supported | core (negative/guard); security-sensitive configuration | same chain; rejection semantics owned by #4998 |

### Feature: Vim + coc.nvim preserves position, synchronization, currentness, and lifecycle correctness

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.vim.bdd.lifecycle.01` | An operation issued after non-BMP text targets the intended symbol/range, not an adjacent one | core; actual host required | #10674 cell → rail observations → editor_client_compat.v1 → #8962 → #8992 → #7122/#8992 support projection |
| `coc.vim.bdd.lifecycle.02` | An actual client edit reaches current server document state before answers are served | core; actual host required | same chain |
| `coc.vim.bdd.lifecycle.03` | Wire edit shape is observed from actual traffic rather than inferred from parser strategy | core; actual host required; instrument-only seam | same chain; instrumentation bounded by #10527/#7777 receipt semantics |
| `coc.vim.bdd.lifecycle.04` | Host shutdown leaves no bound coc Node process or perllsp orphan behind | core; actual host required; cleanup observed independently | same chain; cleanup mechanics authority #10894 stays with host leaves |

### Feature: Neovim + coc.nvim attaches to the intended Perl project

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.neovim.bdd.attach.01` | An ordinary Perl buffer is detected through Neovim's native Perl filetype detection before any override applies | core; actual host required; subject = Neovim + coc.nvim | #10674 cell → #10704-lineage observation → editor_client_compat.v1 cell → #8978 convergence → #8992 fan-in → #7122/#8992 support projection |
| `coc.neovim.bdd.attach.02` | The session runs behind the exact coc.nvim subject pinned by the governed subject contract (#8956, pin authority open); a substitute build or copy does not satisfy attachment | core; actual host required; exact-source evidence | same chain, exact-subject binding re-checked at #8956 landing |
| `coc.neovim.bdd.attach.03` | The exact configured Coc service launches the Perl language server as exactly `perllsp --stdio` | core; actual host required; executable identity law #8956 | same chain |
| `coc.neovim.bdd.attach.04` | The project root the server answers from is the nested Perl project selected by #8956's one root contract for the opened buffer | core; actual host required | same chain |
| `coc.neovim.bdd.attach.05` | A same-named sibling or enclosing project outside the governed root never satisfies a root-sensitive answer | core; actual host required | same chain + #10674 false-subject mechanics |
| `coc.neovim.bdd.attach.06` | After opening a defective Perl buffer, the expected diagnostic becomes visible through the actual client diagnostic state | core; actual host required | same chain |
| `coc.neovim.bdd.attach.07` | Editing the defective source updates the visible diagnostic to the current state | core; actual host required | same chain |

### Feature: Neovim + coc.nvim provides ordinary navigation and completion

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.neovim.bdd.nav.01` | Completion at a real code target selects an item offered by the language server through coc.nvim | core; actual host required | #10674 cell → rail observations → editor_client_compat.v1 → #8978 → #8992 → #7122/#8992 support projection |
| `coc.neovim.bdd.nav.02` | A snippet-capable completion item applies through Coc without leaving literal placeholder text in the buffer | core; actual host required | same chain |
| `coc.neovim.bdd.nav.03` | Hover identifies the intended symbol at the cursor | core; actual host required | same chain |
| `coc.neovim.bdd.nav.04` | Definition resolves the intended project entity of this project | core; actual host required | same chain |
| `coc.neovim.bdd.nav.05` | References identify the declared sites of this project rather than same-name false subjects | core; actual host required | same chain + #10674 decoys |

### Feature: Neovim + coc.nvim applies server edits and configuration safely

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.neovim.bdd.edit.01` | A code action yields an explicit applied / disabled / no-applicable disposition in the editor, not a silent unknown | core; actual host required | #10674 cell → rail observations → editor_client_compat.v1 → #8978 → #8992 → #7122/#8992 support projection |
| `coc.neovim.bdd.edit.02` | Rename applies the complete intended workspace edit only, across exactly this project's intended files | core; actual host required | same chain |
| `coc.neovim.bdd.edit.03` | Formatting reaches the canonical result through client-applied edits, and repeating it changes no further content (idempotent) | core; actual host required | same chain |
| `coc.neovim.bdd.edit.04` | A relative workspace include path affects resolution within the intended root | core; configuration documented substrate; security-sensitive configuration | same chain |
| `coc.neovim.bdd.edit.05` | Absolute or traversal client include paths remain governed/rejected per #4998 rather than assumed supported | core (negative/guard); security-sensitive configuration | same chain; rejection semantics owned by #4998 |

### Feature: Neovim + coc.nvim preserves position, synchronization, currentness, and lifecycle correctness

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.neovim.bdd.lifecycle.01` | An operation issued after non-BMP text targets the intended symbol/range, not an adjacent one | core; actual host required | #10674 cell → rail observations → editor_client_compat.v1 → #8978 → #8992 → #7122/#8992 support projection |
| `coc.neovim.bdd.lifecycle.02` | An actual client edit reaches current server document state before answers are served | core; actual host required | same chain |
| `coc.neovim.bdd.lifecycle.03` | Wire edit shape is observed from actual traffic rather than inferred from parser strategy | core; actual host required; instrument-only seam | same chain; instrumentation bounded by #10527/#7777 receipt semantics |
| `coc.neovim.bdd.lifecycle.04` | Host shutdown leaves no bound coc Node process or perllsp orphan behind | core; actual host required; cleanup observed independently | same chain; cleanup mechanics authority #10894 stays with host leaves |

## §Behavior — extension boundary (owned elsewhere; no rows defined here)

Specialized journey families extend this BDD authority under the namespace law
in `context.md`; each joins as `consumes_if_available` inputs and never blocks
the bounded core:

| Specialized group | Relation to this ledger | Owning issue |
| --- | --- | --- |
| external source/configuration freshness | separate future scenario family(s), specialized tier | #11102 with #7938 reload authority; fixture extension #11107 |
| save-triggered formatting ownership | separate future scenario family(s); manual formatting rows here never satisfy them | #11102 with #8092 owner decision |
| recovery/restart and late-generation rejection | separate future scenario family(s) | #11102 with #10019 workspace/runtime recovery authority |
| host reopen and repeated sessions | separate future scenario family(s) | #11102 |
| expanded file-family/exact-name/shebang activation | consumes #7762/#7743 activation authorities | #11102 |
| optional read/display/topology receipt cells | consume-if-available subset receipts on each host rail | #11302/#11307/#11314 (Vim), #11303/#11309/#11317 (Neovim) |

No `opt.` scenario IDs exist in this packet. Any future optional family mints
host-qualified IDs through its owning issue revision, never silently.

## Claim profiles (ledger membership)

| Profile | Membership rule | Ceiling |
| --- | --- | --- |
| `coc_configuration_documented` | documented setup + registered coc_nvim tier; #8956 contracts when landed | substrate only; proves no behavior |
| `coc_actual_client_core` | exactly the 42 baseline rows above, addressable per host rail | bounded core; nothing else blocks or widens it |
| `first_class_coc_host` | core + applicable specialized cells from #11102 lineage joining as `consumes_if_available` inputs | claimable only after those cells land and pass; never erases the narrower core |
| `coc_programme_closeout` | fan-in over independently terminal child propositions incl. explicit unsupported/not_proven dispositions | composes child results only; manufactures none |

Laws: a stronger profile never erases a narrower valid one; specialized rows
join `consumes_if_available` only; missing chain links are `not_proven`; host
asymmetry ends explicitly unsupported/not_proven inside the owning leaf; no
scenario supports any tier until its executable exact-host chain passes.

## §Hazards

| Class | Invariant | Surface | Required adversarial check |
| --- | --- | --- | --- |
| Profile conflation | Bounded core stays bounded; specialized cells join only as `consumes_if_available` edges | profile laws; #11102 boundary table; F1 | core-row-widened-to-first-class mutation is rejected |
| Save identity collapse | Manual formatting success is never proof of save-triggered behavior | extension boundary; F2 | manual-bytes-as-save-proof mutation is rejected |
| Terminal disposition honesty | Capability asymmetry ends unsupported/not_proven explicitly, never by borrowing the other host | profile laws; F3 | missing-disposition mutation is rejected |
| Cross-host relabeling | Vim rail rows require Vim + coc.nvim observations; Neovim rail likewise | every ledger row; F4 | substituted-host satisfaction mutation is rejected |
| ID stability/qualification | Published IDs match exactly the host-qualified form | namespace law; F5 | unqualified/unstable ID reuse is rejected |
| Log-line theater | Registration events/log lines are provenance, never the user-visible result | attach/nav/edit rows; F6 | log-line-as-result mutation is rejected |
| Root conflation | #8956's one root contract wins; siblings/outer projects never satisfy root-sensitive rows | attach.04/attach.05; F7 | wrong-sibling-root answer passing as correct is rejected |
| Subject substitution | Only the exact governed subject/service launch satisfies attachment rows | attach.02/attach.03; F8 | substitute build/service mutation is rejected |
| Application honesty | coc.nvim must actually apply/consume server results | nav rows; F9 | response-exists-but-not-applied mutations are rejected |
| Edit completeness | Workspace edits are complete and nothing more | edit rows; F10 | partial/decoy-touching rename mutation is rejected |
| Format canonicity/idempotence | Canonical output; a second pass changes nothing | edit rows; F11 | drifting-format mutation is rejected |
| Currentness forgery | Post-edit answers belong to accepted generation; wire shape observed | lifecycle rows; F12 | stale/inferred-shape mutation is rejected |
| Range adjacency | Non-BMP offsets resolve to intended targets only | lifecycle.01; F13 | adjacent-range landing mutation is rejected |
| Security boundary | Workspace-contained relative include paths only; absolute/traversal stays governed/rejected | edit.04/edit.05; #4998 | unsafe path promoted to positive behavior is rejected |
| Mutable leakage | No live SHA/PR/check/writer/wall-clock state in durable bytes | all three files | live-state injection scan fails closed |
| Determinism | Same tree yields identical checker output twice | `checklist.md` proof | second run byte-clean |

## §Contracts

| Contract | Authority consumed | How this bundle satisfies it |
| --- | --- | --- |
| Checked spec directory shape | [`SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md), #3983 | canonical three-file packet; ledger evolution recorded in `context.md` |
| Exact subject/config/root contract | #8956 (open) consumed by reference; no digest recorded | attach rows name the binding authority without pre-stating pins |
| Registered support tier substrate | `policy/lsp-client-support.toml` coc_nvim row via #6739 | configuration_documented tags reuse registry vocabulary; no tier changes here |
| Executable identity law | #8956 command contract; setup prose `docs/EDITORS/COC_NEOVIM_SETUP.md` | attach.03 wording binds exact configured Coc service launching `perllsp --stdio` |
| Filetype activation authorities | #7762 (Vim), #7743 (Neovim) | attach.01 consumes per-host native detection by reference |
| Include-path security | #4998 | edit.04 positive shape workspace-contained relative; edit.05 guard row |
| Generic receipt semantics | `.ci/schemas/editor-client-compat.v1.schema.json` (#7777); #10527 bounds; #10680 projection | receipt stage consumed as boundary owner; no Coc-local receipt ontology |
| Typed edge/profile vocabulary | #10858 | four programme profiles with membership rules, ceilings, `consumes_if_available` class |
| Host reliability/cleanup | #10894 (generic), conformance downstream | lifecycle.04 requires independent cleanup observation |
| Fixture/driver ownership | #10674 fixtures; #10678/#11112 shared driver | fixture/oracle cells bind IDs downstream; zero fixture bytes here |
| Support projection | #8992/#7122 | boundary owners named; citation-only |
| Sibling precedent | `.spec/11371-vim-bdd-journeys/`, `.spec/11717-emacs-train-specs/` | same checked discipline; different programme/plugin, no overlap |

## §API-Shape

No Rust or public API is introduced. Semantic contract terms defined here:

| Item | Kind | Shape | Dup-risk / owner |
| --- | --- | --- | --- |
| `coc.vim.bdd.<family>.<nn>` / `coc.neovim.bdd.<family>.<nn>` | stable host-qualified scenario ID namespaces | 42 IDs total, fixed families/order, immutable once published | none found on main; this packet |
| `coc_actual_client_core` | claim profile ID | membership = the 42 baseline rows | none found; #10815 |
| `first_class_coc_host` | claim profile ID | core + #11102-lineage specialized cells (name preserved from #11102) | term originates upstream; profile definition lands here |
| `coc_programme_closeout` | claim profile ID | programme fan-in incl. terminal dispositions | none found; #10815 |
| `coc_configuration_documented` | claim profile ID | aligns with registered coc_nvim tier vocabulary | tier pre-exists in registry; profile binding is new here |

## §Test-Grid

All thirteen falsifiers, fixed order. Each is a design-level negative control:
a candidate implementation, fixture, driver, receipt, or projection is
conformant only if every mutation fails deterministically in that leaf's own
negative controls.

| # | Scenario | Kind | Required verdict |
|---:|---|---|---|
| 1 | A core row silently widens into a first_class_coc_host prerequisite | negative | reject; the core stays bounded and specialized cells join only as consumes_if_available inputs (#10858) |
| 2 | Manual textDocument/formatting success is offered as proof of save-triggered behavior | negative | reject; save-triggered propositions belong to #11102/#8092 and manual formatting never satisfies them |
| 3 | A host capability asymmetry passes without an explicit unsupported/not_proven disposition | negative | reject; asymmetry terminates explicitly inside the owning leaf |
| 4 | A Vim + coc.nvim row is satisfied by a Neovim + coc.nvim observation, or the reverse | negative | reject; host identity is load-bearing and rows never cross hosts |
| 5 | A scenario ID drifts from the published form or drops its host qualification | negative | reject; published IDs must remain exactly of the form coc.<host>.bdd.<family>.<nn> |
| 6 | A registration event or server log line is presented as the user-visible result | negative | reject; the observable editor-side result is the proposition |
| 7 | Wrong sibling/outer root returns the same symbol spelling and passes as root-correct | negative | reject; root-sensitive answers require the governed root contract (#8956) |
| 8 | A substitute coc.nvim build/copy or service mutation satisfies attachment | negative | reject; only the exact governed subject/service launch (#8956) satisfies attach rows |
| 9 | Completion/action response exists but Coc did not apply it, or literal snippet placeholders survive | negative | reject; client application through coc.nvim is the proposition (nav rows) |
| 10 | Rename applies fewer or more occurrences/files than intended | negative | reject; complete-intended-edit-only is the proposition (edit rows) |
| 11 | Formatting diverges from canonical output or a second pass changes bytes again | negative | reject; canonical idempotent result is the proposition (edit rows) |
| 12 | A post-edit answer reflects pre-edit state, or wire edit shape is inferred instead of observed | negative | reject; accepted-generation currentness and observed wire shape are propositions (lifecycle rows) |
| 13 | A non-BMP operation lands on an adjacent range | negative | reject; intended-target resolution is the proposition (lifecycle rows) |

## §Blast-Radius

| Consumer / surface | Impact | Required update |
| --- | --- | --- |
| #11102 specialized BDD extension | Becomes spawn-ready against these baseline IDs/families | Consume IDs; mint new families per namespace law; do not redefine baseline propositions |
| #10674 (+#11107) fixture/oracle cells | Bind these IDs when authoring fixtures | Consume IDs; do not redefine propositions |
| #10678/#11112 shared driver | Operationalizes scenarios downstream | Reference only |
| #10685 / #10704 adapter leaves | Observe against named scenarios | Bind IDs into raw observations |
| #8962 / #8978 convergence programs | Compose per-host evidence citing stable IDs | Reference only |
| #11125 / #11127 flagged host proofs | Emit host-qualified receipt cells bound to IDs | Reference only |
| #11302/#11307/#11314, #11303/#11309/#11317 optional leaves | Subset receipts cite per-rail IDs | Reference only |
| #10680 producers; #10527/#7777 schema bounds | Cell provenance cites scenario IDs | Reference only; schema unchanged |
| #8992 / #7122 / `policy/lsp-client-support.toml` | No change in this PR | Future Coc rows flow via #8992/#7122 once chains pass |
| Generated status/docs surfaces | None exist for this ledger today | Any future generator must derive from these IDs |
| Product/runtime crates | None | Must-not-touch |

Must-not-touch: `crates/`, `xtask/`, `.github/workflows/`, fixture sources,
host harnesses, receipts, support registry values, docs prose, external
upstream surfaces.

## §Coverage-Map

| #10815 acceptance item | Covered by |
| --- | --- |
| Small number of cohesive features rather than one scenario per RPC | four fixed families × two rails |
| Distinct host subjects preserved (Vim+Coc, Neovim+Coc) | host-qualified namespaces; cross-host relabeling falsifier |
| Scenarios covering all listed bullets of the four features | §Behavior ledgers (21 rows per rail) |
| Machine-visible distinction tags using existing conventions | `context.md` tag mapping into registry/schema vocabularies |
| Stable IDs bindable by #10674/#11102/#11125/#11127 | §API-Shape + immutability law |
| Explicit dispositions where applicable | terminal-disposition laws + closeout profile ceiling |
| Security governance of include paths retained | #4998-consumed edit rows + hazard row |
| False greens explicit | §Test-Grid rows 1–13 fixed order |
| Baseline versus specialized/optional separation | extension-boundary table + profile laws |
| Authorities/security boundaries named | §Contracts; security boundary section |
| Generated outputs current/deterministic | `checklist.md` two-run structural proof (no generator exists; recorded) |
| No fixture/host/receipt/support claim created | scope boundary + blast radius + claim boundary below |

## Scope, rollback, and proof claims

- **In scope:** exactly the three files of `.spec/10815-coc-bdd-journeys/`.
- **Rollback:** revert this bundle's commit; issues retain full authority.
  Any downstream artifact already bound to these IDs reverts through its own
  owner, never by editing this bundle silently.
- **Transfer:** only with exact current evidence inventory and named
  receiving owner; otherwise `not_proven`.
- **Stop:** return to #10815 if a boundary above would need weakening to make
  a downstream check green, or current main contradicts a material decision.
- **Claim boundary:** proves a durable checked journey/evidence contract and
  deterministic structural inspection only. Proves no Coc behavior, host
  execution, fixture correctness, receipt, support tier, public artifact, or
  upstream state. All 42 scenarios remain `not_proven` as behavior until
  their executable exact-host chains pass under their owning leaves.
