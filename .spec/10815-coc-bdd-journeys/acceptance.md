# Acceptance Criteria: #10815 — checked Coc user journeys and evidence boundaries

This is a checked, declarative BDD/spec-ledger contract. It is a packet-local
Markdown contract, not a generated repository status projection. It implements no
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
observation, a native Neovim LSP observation, or any third subject.

### Feature: Vim + coc.nvim attaches to the intended Perl project

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.vim.bdd.attach.01` | An ordinary Perl buffer is detected through Vim's native Perl filetype detection before any override applies | core; actual host required; subject = Vim + coc.nvim | #10674 fixture/expectation cell → #10678 operation → #10685-lineage host-leaf observation → #10680 `editor_client_compat.v1` cell → #8967 actual_client_core fan-in → #8992/#7122 support projection |
| `coc.vim.bdd.attach.02` | The session runs behind the exact coc.nvim subject pinned by the governed subject contract (#8956, pin authority open); a substitute build or copy does not satisfy attachment | core; actual host required; exact-source evidence | same chain, exact-subject binding re-checked at #8956 landing |
| `coc.vim.bdd.attach.03` | The exact configured Coc service launches the Perl language server as exactly `perllsp --stdio` | core; actual host required; executable identity law #8956 | same chain |
| `coc.vim.bdd.attach.04` | The project root the server answers from is the nested Perl project selected by #8956's one root contract for the opened buffer | core; actual host required | same chain |
| `coc.vim.bdd.attach.05` | A same-named sibling or enclosing project outside the governed root never satisfies a root-sensitive answer | core; actual host required | same chain + #10674 false-subject mechanics |
| `coc.vim.bdd.attach.06` | After opening a defective Perl buffer, the expected diagnostic becomes visible through the actual client diagnostic state | core; actual host required | same chain |
| `coc.vim.bdd.attach.07` | Editing the defective source updates the visible diagnostic to the current state | core; actual host required | same chain |

### Feature: Vim + coc.nvim provides ordinary navigation and completion

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.vim.bdd.nav.01` | Completion at a real code target selects an item offered by the language server through coc.nvim | core; actual host required | #10674 cell → #10678 operation → #10685-lineage observation → #10680 cell → #8967 fan-in → #8992/#7122 support projection |
| `coc.vim.bdd.nav.02` | A snippet-capable completion item applies through Coc without leaving literal placeholder text in the buffer | core; actual host required | same chain |
| `coc.vim.bdd.nav.03` | Hover identifies the intended symbol at the cursor | core; actual host required | same chain |
| `coc.vim.bdd.nav.04` | Definition resolves the intended project entity of this project | core; actual host required | same chain |
| `coc.vim.bdd.nav.05` | References identify the declared sites of this project rather than same-name false subjects | core; actual host required | same chain + #10674 decoys |

### Feature: Vim + coc.nvim applies server edits and configuration safely

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.vim.bdd.edit.01` | A code action yields an explicit applied / disabled / no-applicable disposition in the editor, not a silent unknown | core; actual host required | #10674 cell → #10678 operation → #10685-lineage observation → #10680 cell → #8967 fan-in → #8992/#7122 support projection |
| `coc.vim.bdd.edit.02` | Rename applies the complete intended workspace edit only, across exactly this project's intended files | core; actual host required | same chain |
| `coc.vim.bdd.edit.03` | Formatting reaches the canonical result through client-applied edits, and repeating it changes no further content (idempotent) | core; actual host required | same chain |
| `coc.vim.bdd.edit.04` | A relative workspace include path affects resolution within the intended root | core; configuration documented substrate; security-sensitive configuration | same chain |
| `coc.vim.bdd.edit.05` | Absolute or traversal client include paths remain governed/rejected per #4998 rather than assumed supported | core (negative/guard); security-sensitive configuration | same chain; rejection semantics owned by #4998 |

### Feature: Vim + coc.nvim preserves position, synchronization, currentness, and lifecycle correctness

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.vim.bdd.lifecycle.01` | An operation issued after non-BMP text targets the intended symbol/range, not an adjacent one | core; actual host required | #10674 cell → #10678 operation → #10685-lineage observation → #10680 cell → #8967 fan-in → #8992/#7122 support projection |
| `coc.vim.bdd.lifecycle.02` | An actual client edit reaches current server document state before answers are served | core; actual host required | same chain |
| `coc.vim.bdd.lifecycle.03` | Wire edit shape is observed from actual traffic rather than inferred from parser strategy | core; actual host required; instrument-only seam | same chain; instrumentation bounded by #10527/#7777 receipt semantics |
| `coc.vim.bdd.lifecycle.04` | Host shutdown leaves no bound coc Node process or perllsp orphan behind | core; actual host required; cleanup observed independently of editor events | same chain; cleanup mechanics authority #10894 stays with host leaves |

### Feature: Neovim + coc.nvim attaches to the intended Perl project

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.neovim.bdd.attach.01` | An ordinary Perl buffer is detected through Neovim's native Perl filetype detection before any override applies | core; actual host required; subject = Neovim + coc.nvim | #10674 fixture/expectation cell → #10678 operation → #10704-lineage host-leaf observation → #10680 `editor_client_compat.v1` cell → #10717 actual_client_core fan-in → #8992/#7122 support projection |
| `coc.neovim.bdd.attach.02` | The session runs behind the exact coc.nvim subject pinned by the governed subject contract (#8956, pin authority open); a substitute build or copy does not satisfy attachment | core; actual host required; exact-source evidence | same chain, exact-subject binding re-checked at #8956 landing |
| `coc.neovim.bdd.attach.03` | The exact configured Coc service launches the Perl language server as exactly `perllsp --stdio` | core; actual host required; executable identity law #8956 | same chain |
| `coc.neovim.bdd.attach.04` | The project root the server answers from is the nested Perl project selected by #8956's one root contract for the opened buffer | core; actual host required | same chain |
| `coc.neovim.bdd.attach.05` | A same-named sibling or enclosing project outside the governed root never satisfies a root-sensitive answer | core; actual host required | same chain + #10674 false-subject mechanics |
| `coc.neovim.bdd.attach.06` | After opening a defective Perl buffer, the expected diagnostic becomes visible through the actual client diagnostic state | core; actual host required | same chain |
| `coc.neovim.bdd.attach.07` | Editing the defective source updates the visible diagnostic to the current state | core; actual host required | same chain |

### Feature: Neovim + coc.nvim provides ordinary navigation and completion

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.neovim.bdd.nav.01` | Completion at a real code target selects an item offered by the language server through coc.nvim | core; actual host required | #10674 cell → #10678 operation → #10704-lineage observation → #10680 cell → #10717 fan-in → #8992/#7122 support projection |
| `coc.neovim.bdd.nav.02` | A snippet-capable completion item applies through Coc without leaving literal placeholder text in the buffer | core; actual host required | same chain |
| `coc.neovim.bdd.nav.03` | Hover identifies the intended symbol at the cursor | core; actual host required | same chain |
| `coc.neovim.bdd.nav.04` | Definition resolves the intended project entity of this project | core; actual host required | same chain |
| `coc.neovim.bdd.nav.05` | References identify the declared sites of this project rather than same-name false subjects | core; actual host required | same chain + #10674 decoys |

### Feature: Neovim + coc.nvim applies server edits and configuration safely

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.neovim.bdd.edit.01` | A code action yields an explicit applied / disabled / no-applicable disposition in the editor, not a silent unknown | core; actual host required | #10674 cell → #10678 operation → #10704-lineage observation → #10680 cell → #10717 fan-in → #8992/#7122 support projection |
| `coc.neovim.bdd.edit.02` | Rename applies the complete intended workspace edit only, across exactly this project's intended files | core; actual host required | same chain |
| `coc.neovim.bdd.edit.03` | Formatting reaches the canonical result through client-applied edits, and repeating it changes no further content (idempotent) | core; actual host required | same chain |
| `coc.neovim.bdd.edit.04` | A relative workspace include path affects resolution within the intended root | core; configuration documented substrate; security-sensitive configuration | same chain |
| `coc.neovim.bdd.edit.05` | Absolute or traversal client include paths remain governed/rejected per #4998 rather than assumed supported | core (negative/guard); security-sensitive configuration | same chain; rejection semantics owned by #4998 |

### Feature: Neovim + coc.nvim preserves position, synchronization, currentness, and lifecycle correctness

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary (owner chain) |
| --- | --- | --- | --- |
| `coc.neovim.bdd.lifecycle.01` | An operation issued after non-BMP text targets the intended symbol/range, not an adjacent one | core; actual host required | #10674 cell → #10678 operation → #10704-lineage observation → #10680 cell → #10717 fan-in → #8992/#7122 support projection |
| `coc.neovim.bdd.lifecycle.02` | An actual client edit reaches current server document state before answers are served | core; actual host required | same chain |
| `coc.neovim.bdd.lifecycle.03` | Wire edit shape is observed from actual traffic rather than inferred from parser strategy | core; actual host required; instrument-only seam | same chain; instrumentation bounded by #10527/#7777 receipt semantics |
| `coc.neovim.bdd.lifecycle.04` | Host shutdown leaves no bound coc Node process or perllsp orphan behind | core; actual host required; cleanup observed independently of editor events | same chain; cleanup mechanics authority #10894 stays with host leaves |

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
| `first_class_coc_host` | `coc_actual_client_core` plus an optional join point for #11102-lineage specialized cells (`consumes_if_available`); while no joined cell has landed, membership reduces exactly to the core | claimable once its joined cells land and pass; absence or failure of a specialized cell reduces to the core and never erases a narrower valid row |
| `coc_programme_closeout` | fan-in over independently terminal child propositions incl. explicit unsupported/not_proven dispositions | composes child results only; manufactures none |

Laws: a stronger profile never erases a narrower valid one; specialized rows
join `consumes_if_available` only; missing chain links are `not_proven`; host
asymmetry ends explicitly unsupported/not_proven inside the owning leaf; no
scenario supports any tier until its executable exact-host chain passes.

## Outcome vocabulary (allowed non-pass dispositions)

Every scenario has an exact claim ceiling and may terminate non-pass without
being hidden. This packet mints no Coc-only scalar verdict; allowed outcomes
consume existing repository vocabularies, preserving equivalents of:

```text
pass                exact observed satisfaction inside the claim ceiling
fail                observed contradiction of the proposition
partial             some sub-propositions hold; named limitation recorded
limited             holds only within a narrowed envelope (recorded)
client_not_exposed  the pinned client cannot express or expose the proposition
unsupported         capability/feature absent on that host rail by fact
not_proven          missing, partial, stale, or contradictory evidence
instrument_failed   the measurement surface failed (never silently product failure)
reporting_failed    projection/report stage failed after upstream truth existed
cleanup_failed      terminal session ended without required cleanup truth
```

Vocabulary presence is not Coc provenance (emission, validation, or adapter
ownership). The current repository evidence is deliberately separated here:

| Term or set | Generic repository presence | Coc journey ownership/emission/validation/adapter provenance in this packet |
| --- | --- | --- |
| `pass \| fail \| partial \| not_proven \| unsupported` | Allowed by the `editor_client_compat.v1` journeyCell `result` enum; process-cleanup facts are bounded to `pass \| fail \| not_proven` (#7777 schema; #10527 bounds). | These are the only result values this Coc packet may claim as schema-compatible; a Coc owner must still provide the exact receipt cell and source/link evidence. |
| `limited`, `client_not_exposed` | Present in shipped editor-cell ladders and editor documentation (including the vim-lsp catalog and IntelliJ DAP/IDEA ladders). | Generic vocabulary only. No Coc-owned emitter, validator, or adapter is established here. Coc leaves record an allowed schema value plus limitation text until an owner proves the mapping. |
| `instrument_failed` | Present in shipped host-compat/receipt schemas, including the Zed v1 receipt family. | Generic vocabulary only. No Coc-owned emitter, validator, or adapter is established here. |
| `reporting_failed` | Issue-named design vocabulary from #10815; no current Coc-owned receipt-schema enum or emitter is identified. | Design-level terminal disposition only. A future owner must prove the projection/report source and link it before a Coc machine cell may emit it. |
| `cleanup_failed` | Current non-Coc surfaces exist in `.spec/11178-lite-xl-bdd-journeys/` and `xtask/src/bin/ci-route-plan.rs`. | No current Coc-owned receipt-schema enum or emitter is identified. Until a Coc owner lands and proves that schema/emitter, Coc leaves use the nearest allowed value (`not_proven`) plus limitation text; they never silently promote a cleanup failure. |

Registry tiers add `configuration_documented` / `not_proven_unsupported`.
The structural checker below verifies vocabulary text and packet shape only; it
does not validate Coc provenance, source links, emitters, validators, adapters,
or generated projections.

Every non-pass termination is owned by the stage that terminates it, always
with a recorded limitation — never relabeled into a pass downstream.

## §Hazards

| Class | Invariant | Surface | Required adversarial check |
| --- | --- | --- | --- |
| Profile conflation | Bounded core stays bounded; specialized cells join only as `consumes_if_available` edges | profile laws; #11102 boundary table; F1 | core-row-widened-to-first-class mutation is rejected |
| Save identity collapse | Manual formatting success is never proof of save-triggered behavior | extension boundary; F2 | manual-bytes-as-save-proof mutation is rejected |
| Terminal disposition honesty | Capability asymmetry ends unsupported/not_proven explicitly, never by borrowing the other host | outcome vocabulary; F3/F22 | missing-disposition and unknown-cleanup-as-pass mutations are rejected |
| Cross-host relabeling | Vim rail rows require Vim + coc.nvim observations; Neovim rail likewise; native Neovim LSP satisfies neither | every ledger row; F4/F20 | substituted-host/native-client satisfaction mutations are rejected |
| ID stability/qualification | Published IDs match exactly the host-qualified form | namespace law; F5 | unqualified/unstable ID reuse is rejected |
| Log-line theater | Registration events/log lines/settings echoes are provenance, never the user-visible result | attach/nav/edit rows; F6 | log-line-as-result mutation is rejected |
| Root conflation | #8956's one root contract wins; siblings/outer projects never satisfy root-sensitive rows | attach.04/attach.05; F9 | wrong-sibling-root answer passing as correct is rejected |
| Subject substitution | Only the exact governed subject/service launch satisfies attachment rows | attach.02/attach.03; F7 | substitute build/service mutation is rejected |
| Activation observation | Native detection precedes override and is observed, never manufactured | attach.01; F8 | manufactured-filetype-timing mutation is rejected |
| Diagnostic identity | The expected diagnostic itself must appear; unrelated presence never passes | attach.06/attach.07; F10 | non-empty-list-as-pass mutation is rejected |
| Application honesty | coc.nvim must actually apply/consume server results end-to-end | nav rows; F11 | raw-request-plus-independent-insertion mutation is rejected |
| Item identity | Server item identity (kind/text-edit), not label spelling, is what must apply | nav.01/nav.02; F12 | same-label-wrong-item mutation is rejected |
| Answered-entity identity | Hover/definition/reference entities are the proposition, not non-emptiness | nav.03–nav.05; F13 | plausible-wrong-entity mutation is rejected |
| Applied-state honesty | Requests succeed only together with the exact resulting state | edit.01–edit.03; F14 | request-success-without-state mutation is rejected |
| Configuration theater | Configuration objects must have root-specific semantic effect | edit.04; F15 | settings-presence-without-effect mutation is rejected |
| Security boundary | Workspace-contained relative include paths only; absolute/traversal stays governed/rejected | edit.04/edit.05; F16 | unsafe path promoted to positive behavior is rejected |
| Range adjacency | Non-BMP offsets resolve to character-aligned intended targets only | lifecycle.01; F17 | byte/wrong-side-of-astral mutation is rejected |
| Wire-shape honesty | Synchronization claims require observed didChange traffic | lifecycle.02/lifecycle.03; F18 | zero-capture-as-traffic-claim mutation is rejected |
| Currentness forgery | Post-edit answers belong to accepted generation | lifecycle.02/lifecycle.03; F19 | stale-pre-edit-result mutation is rejected |
| Mutable leakage | No live SHA/PR/check/writer/wall-clock state in durable bytes | all three files | live-state injection scan fails closed |
| Determinism | Same tree yields identical checker output twice | `checklist.md` proof | second run byte-clean |

## §Contracts

| Contract | Authority consumed | How this bundle satisfies it |
| --- | --- | --- |
| Checked spec directory shape | [`SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md), #3983 | three-file packet shape; ledger evolution recorded in `context.md` |
| Exact subject/config/root contract | #8956 (open) consumed by reference; no digest recorded | attach rows name the binding authority without pre-stating pins |
| Registered support tier substrate | `policy/lsp-client-support.toml` coc_nvim row via #6739 | configuration_documented tags reuse registry vocabulary; no tier changes here |
| Executable identity law | #8956 command contract; setup prose `docs/EDITORS/COC_NEOVIM_SETUP.md` | attach.03 wording binds exact configured Coc service launching `perllsp --stdio` |
| Filetype activation authorities | #7762 (Vim), #7743 (Neovim) | attach.01 consumes per-host native detection by reference |
| Include-path security | #4998 | edit.04 positive shape workspace-contained relative; edit.05 guard row |
| Generic receipt semantics | `.ci/schemas/editor-client-compat.v1.schema.json` (#7777); #10527 bounds; #10680 projection | receipt stage consumed as boundary owner; no Coc-local receipt ontology |
| Typed edge/profile vocabulary | #10858 | four programme profiles with membership rules, ceilings, `consumes_if_available` class |
| Host reliability/cleanup | #10894 (generic), conformance downstream | lifecycle.04 requires independent OS-process cleanup observation |
| Fixture/driver ownership | #10674 fixtures; #10678/#11112 shared driver operations | fixture/oracle cells bind IDs downstream; zero fixture bytes here |
| Per-rail fan-in ownership | #8967 (Vim rail), #10717 (Neovim rail) | chains terminate in promotable-receipt owners per rail; citation-only |
| Support projection | #8992/#7122 | boundary owners named; citation-only |
| Sibling precedent | `.spec/11371-vim-bdd-journeys/`, `.spec/11178-lite-xl-bdd-journeys/`, `.spec/11717-emacs-train-specs/` | same checked discipline; different programme/plugin, no overlap |

## §API-Shape

No Rust or public API is introduced. Semantic contract terms defined here:

| Item | Kind | Shape | Dup-risk / owner |
| --- | --- | --- | --- |
| `coc.vim.bdd.<family>.<nn>` / `coc.neovim.bdd.<family>.<nn>` | stable host-qualified scenario ID namespaces | 42 IDs total, fixed families/order, immutable once published | none found on main; this packet |
| `coc_actual_client_core` | claim profile ID | membership = the 42 baseline rows | none found; #10815 |
| `first_class_coc_host` | claim profile ID | core + #11102-lineage specialized cells (name preserved from #11102) | term originates upstream; profile definition lands here |
| `coc_programme_closeout` | claim profile ID | programme fan-in incl. terminal dispositions | none found; #10815 |
| `coc_configuration_documented` | claim profile ID | aligns with registered coc_nvim tier vocabulary | tier pre-exists in registry; profile binding is new here |
| Outcome ladder terms | vocabulary consumption | pass/fail/partial/limited/client_not_exposed/unsupported/not_proven/instrument_failed/reporting_failed/cleanup_failed mapped to existing stages | schema/registry vocabularies consumed; no new verdict scalar minted |

## §Test-Grid

All twenty-two normative falsifiers, fixed order: F1–F6 are the family
decomposition brief's required falsifiers ([DECOMP:BRIEF:coc-receipt-cells]
on #10658); F7–F22 are the issue's baseline false-green enumeration in its
exact order. Each is a design-level negative control: a candidate
implementation, fixture, driver, receipt, or projection is conformant only if
every mutation fails deterministically in that leaf's own negative controls.

| # | Scenario | Kind | Required verdict |
|---:|---|---|---|
| 1 | A core row silently widens into a first_class_coc_host prerequisite | negative | reject; the core stays bounded and specialized cells join only as consumes_if_available inputs (#10858) |
| 2 | Manual textDocument/formatting success is offered as proof of save-triggered behavior | negative | reject; save-triggered propositions belong to #11102/#8092 and manual formatting never satisfies them |
| 3 | A host capability asymmetry passes without an explicit unsupported/not_proven disposition | negative | reject; asymmetry terminates explicitly inside the owning leaf and is never borrowed from the other rail |
| 4 | A Vim + coc.nvim row is satisfied by a Neovim + coc.nvim observation, or the reverse | negative | reject; host identity is load-bearing and rows never cross rails |
| 5 | A scenario ID drifts from the published form or drops its host qualification | negative | reject; published IDs remain exactly coc.vim.bdd.<family>.<nn> or coc.neovim.bdd.<family>.<nn> |
| 6 | A registration event, launch log line, or settings echo stands in for the user-visible result | negative | reject; the observable editor-side semantic result is the proposition (attach/nav/edit rows) |
| 7 | An ambient or wrong perllsp, coc.nvim, Node, or editor subject satisfies an attachment row | negative | reject; only the exact governed subject/service launch (#8956) satisfies attach rows |
| 8 | Native filetype detection is manufactured or asserted without being observed before any override | negative | reject; observed-before-override is the proposition (attach.01) |
| 9 | The outer CWD or a same-named sibling root returns the same symbol spelling and passes as root-correct | negative | reject; root-sensitive answers require the governed root contract (#8956) |
| 10 | Any non-empty diagnostic list is accepted while the expected diagnostic is absent | negative | reject; the expected diagnostic itself must appear (attach.06) |
| 11 | Raw completion succeeds plus an independent snippet insertion, bypassing Coc application | negative | reject; consumption through coc.nvim is the proposition (nav.01–nav.02) |
| 12 | A completion item with only the same label but different kind/text-edit is accepted as applied | negative | reject; the intended server item identity is the proposition (nav.01) |
| 13 | Hover/definition/references return plausible-but-wrong entities, including wrong-project symbols or decoy sites | negative | reject; entity/site identity is the proposition (nav.03–nav.05) |
| 14 | Code action, rename, or format request succeeds while buffer/file state does not reach the exact resulting state | negative | reject; applied exact state is the proposition (edit.01–edit.03) |
| 15 | Configuration presence or log lines are accepted without root-specific semantic effect | negative | reject; independent semantic effect within the governed root is the proposition (edit.04) |
| 16 | An absolute/traversal client include path is used to make a fixture pass as ordinary behavior | negative | reject; unsafe paths stay governed/rejected per #4998 (edit.05) |
| 17 | A Unicode operation targets bytes or lands on the wrong side of an astral character | negative | reject; character-aligned intended-target resolution (lifecycle.01) |
| 18 | Zero captured didChange traffic is interpreted as a synchronization claim | negative | reject; observed wire edit shape is the proposition (lifecycle.03) |
| 19 | A stale generation/result survives an accepted edit and is served as current | negative | reject; accepted-generation currentness (lifecycle.02) |
| 20 | Built-in native Neovim LSP or another service supplies a Neovim + coc.nvim cell | negative | reject; native Neovim LSP is a distinct subject and never satisfies coc.neovim.bdd rows |
| 21 | A host/client shutdown event substitutes for OS process evidence of cleanup | negative | reject; shutdown leaves no bound child process, observed independently (lifecycle.04) |
| 22 | A stale receipt, timeout, missing instrument, or unknown cleanup becomes pass | negative | reject; instrument/cleanup failure stays an explicit terminal disposition, never silent pass |

## §Blast-Radius

| Consumer / surface | Impact | Required update |
| --- | --- | --- |
| #11102 specialized BDD extension | Becomes spawn-ready against these baseline IDs/families | Consume IDs; mint new families per namespace law; do not redefine baseline propositions |
| #10674 (+#11107) fixture/expectation cells | Bind these IDs when authoring fixtures | Consume IDs; do not redefine propositions |
| #10678/#11112 shared driver operations | Operationalize scenarios downstream | Reference only |
| #10685 / #10704 adapter leaves | Observe against named scenarios | Bind IDs into raw observations |
| #8967 / #10717 per-rail fan-in | Compose promotable receipts citing stable IDs | Reference only |
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
| Distinct host subjects preserved (Vim+Coc, Neovim+Coc) | host-qualified namespaces; cross-host and native-LSP falsifiers |
| Scenarios covering all listed bullets of the four features | §Behavior ledgers (21 rows per rail) |
| Machine-visible distinction tags using existing conventions | `context.md` tag mapping into registry/schema vocabularies |
| Stable IDs bindable by #10674/#11102/#11125/#11127 | §API-Shape + immutability law |
| Explicit dispositions where applicable | outcome vocabulary table + terminal-disposition laws + closeout ceiling |
| Security governance of include paths retained | #4998-consumed edit rows + hazard row + F16 |
| False greens explicit across issue and family-brief sets | §Test-Grid rows 1–22 fixed order |
| Baseline versus specialized/optional separation | extension-boundary table + profile laws + F1 |
| Authorities/security boundaries named | §Contracts; security boundary section |
| Checker-only packet proof | `checklist.md` two-run structural proof; no generated projection is produced or claimed |
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
  deterministic structural inspection only. It does not claim the controlling
  issue's generated BDD/status projections or full acceptance, because current
  main provides no authoritative generator for them. It proves no Coc behavior,
  host execution, fixture correctness, receipt, support tier, public artifact,
  upstream state, or Coc provenance for generic vocabulary terms. The checker
  does not validate source/link reachability or emitter/validator/adapter
  ownership. All 42 scenarios remain `not_proven` as behavior until
  their executable exact-host chains pass under their owning leaves.
