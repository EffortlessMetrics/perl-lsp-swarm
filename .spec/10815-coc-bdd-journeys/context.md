# Context: #10815 — checked Coc user journeys and evidence boundaries in the spec ledger

## Problem

The Coc integration train (#8949 product controller, #10658 train controller)
currently begins its executable graph downstream of a checked expression of
product semantics that does not exist in the repository yet. Every consumer
chain — #10674 fixture manifest, #11102 specialized journeys, #8962/#8978 host
evidence programs, #8992/#7122 support projection — starts at "#10815
baseline", but until journey and evidence identities exist as checked
repository data, builders, fixture authors, host-driver authors, reviewers,
receipt fan-in, and support projection each carry a private copy of "what the
Coc journey means".

This bundle encodes one checked normative baseline for the Coc user journeys so
downstream leaves bind stable scenario IDs instead of re-deriving meaning. It
owns behavior wording, scenario identity, claim-profile membership, and
evidence boundaries only. It is a packet-local Markdown contract, not a
generated repository projection or executable behavior oracle.

It owns two distinct host subjects, and host identity is load-bearing:

```text
Vim + coc.nvim
Neovim + coc.nvim
```

A third subject is explicitly outside this ledger: native Neovim LSP (the
built-in client) is neither rail, and its evidence can never satisfy a
`coc.neovim.bdd.*` row. No scenario may imply both hosts pass because one
examples-table row passes. A test script is not the product spec; a Gherkin
sentence is not an executable semantic oracle. Each consumes the other through
the stable scenario IDs defined below.

## Why this approach (ledger-format evolution record)

Issue #10815 names "repository Gherkin + acceptance/spec ledger + generated
`docs/feature_status.md`". Current main has no Gherkin `.feature` runner
surface, no generated feature-status document, and none of those xtask
subcommands; the repository's existing, shipped BDD/spec-ledger authority is
the `.spec/` packet system governed by
[`docs/reference/SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md) and
#3983 conventions. The landed precedent packets `.spec/11371-vim-bdd-journeys/`
(Vim + vim-lsp), `.spec/11178-lite-xl-bdd-journeys/`, and
`.spec/11717-emacs-train-specs/specs.ledger.json` exercise exactly this shape.
Per the issue's own evolution clause ("If command names have evolved, use
current equivalents and record them in the PR review map"), this packet
projects the Gherkin-style Feature/Scenario organization into that existing
spec-ledger system: features map to journey families, scenarios to stable-ID
ledger rows, and step-level executable truth stays downstream. Because current
main has no generator or status projection, this PR does not claim to satisfy
the issue's generated-output acceptance; that remains an explicit follow-up or
source-truth ruling.
Introducing a new parallel `.feature` format or a feature-status generator
here is out of scope.

Keep distinct:

```text
BDD/spec ledger (this packet)
  what the user/editor must observe
  which propositions are baseline, specialized, optional, or stronger-profile
  which evidence stage/subject may satisfy them

#10674 fixture manifest (later leaf)
  exact source bytes, anchors, layouts, canonical expectation refs,
  false-subject mechanics, and machine identities used to prove it
```

## Subject substrate (consumed by reference; no pin exists yet)

The pinned subject/config/root contract for Coc is owned by open issue #8956;
no `.ci/editor-clients/coc-*` manifest is landed on current main. This packet
therefore records **no** subject digest and pins nothing. It consumes, by
reference only:

```text
#8956                    exact upstream coc.nvim subject pin + one configuration/root contract
policy/lsp-client-support.toml   registered client tier id coc_nvim, integration_mode coc_language_server,
                         tier configuration_documented, requires_actual_client_receipt = true,
                         synthetic_profile = false, documented evidence docs/EDITORS/COC_NEOVIM_SETUP.md
docs/EDITORS/COC_NEOVIM_SETUP.md setup prose consumed as documentation provenance only
#7762 / #7743            Vim / Neovim native Perl filetype activation and root-selection authorities
#6739                    registry row owner for the existing configuration_documented tier
```

Every behavioral statement below traces to a named authority above or carries
an explicit boundary note. When #8956 lands its pinned manifests, reviewed
re-binding of attach rows to those exact digests happens through this owning
issue's revision path; this bundle never clones or pre-states subject bytes.

## Stable scenario ID namespace

Stable downstream-consumable IDs are host-qualified and use the form:

```text
coc.vim.bdd.<family>.<nn>       Vim + coc.nvim rail
coc.neovim.bdd.<family>.<nn>    Neovim + coc.nvim rail
```

Sibling packets establish the `<client-prefix>.bdd.<family>.<nn>` convention
(`vim.bdd.*`, `lite_xl.bdd.*`). For Coc the plugin is shared across two hosts,
so the host segment inside the prefix is load-bearing: it is what makes
cross-host relabeling structurally impossible to express. Families, in fixed
order:

```text
attach      Coc attaches to the intended Perl project
nav         Coc provides ordinary completion and navigation
edit        Coc applies server edits and configuration safely
lifecycle   Coc preserves position, synchronization, currentness, and lifecycle correctness
```

IDs are immutable once published; retirement requires a new revision through
this owning issue, never silent reuse. Unqualified forms such as
`coc.bdd.*` without the host segment do not exist and must never be minted:
cross-host relabeling is an explicit falsifier (§Test-Grid).

## Journey inventory (baseline = 42 scenarios: 21 per host rail)

Full normative wording, profile membership, and per-scenario evidence owners
are the §Behavior ledger in `acceptance.md`. Summary, identical family shape
across both rails with per-host subjects:

### Feature: Coc attaches to the intended Perl project

```text
<host>.bdd.attach.01  native host Perl filetype detection is observed before any override applies
<host>.bdd.attach.02  the session runs behind the exact governed coc.nvim subject (#8956 pin when landed)
<host>.bdd.attach.03  the exact configured Coc service launches exactly `perllsp --stdio`
<host>.bdd.attach.04  nested Perl root wins according to #8956's one root contract
<host>.bdd.attach.05  same-named sibling/outer projects cannot satisfy a root proof
<host>.bdd.attach.06  a diagnostic appears for the exact source defect
<host>.bdd.attach.07  editing the defect produces current diagnostic state
```

### Feature: Coc provides ordinary navigation and completion

```text
<host>.bdd.nav.01     completion at a real code target selects a server item
<host>.bdd.nav.02     a snippet-capable item applies through Coc without literal placeholders
<host>.bdd.nav.03     hover identifies the intended symbol
<host>.bdd.nav.04     definition resolves the intended project entity
<host>.bdd.nav.05     references identify declared sites, not same-name false subjects
```

### Feature: Coc applies server edits and configuration safely

```text
<host>.bdd.edit.01    code action has an explicit applied/disabled/no-applicable disposition
<host>.bdd.edit.02    rename applies the complete intended workspace edit only
<host>.bdd.edit.03    formatting reaches the canonical result and is idempotent
<host>.bdd.edit.04    relative workspace include path affects resolution in the intended root
<host>.bdd.edit.05    absolute/traversal client include paths stay governed by #4998, never assumed supported
```

### Feature: Coc preserves position, synchronization, and lifecycle correctness

```text
<host>.bdd.lifecycle.01  operation after non-BMP text targets the intended symbol/range
<host>.bdd.lifecycle.02  actual client edit reaches current server document state
<host>.bdd.lifecycle.03  wire edit shape is observed rather than inferred from parser strategy
<host>.bdd.lifecycle.04  host shutdown leaves no bound Coc Node/perllsp orphan
```

This packet defines no `opt.` family rows. Specialized journeys (freshness,
save-triggered formatting, recovery/restart, reopen and repeated sessions,
expanded activation families) extend this BDD authority through the separate
owned extension #11102 under this namespace law; they never cram unimplemented
breadth into the baseline, and their absence cannot block the bounded core.

## Claim profiles

Consume #10858's shared typed profile semantics (programme-owned profile IDs,
required proposition sets, terminal limitation states, claim ceilings). The
Coc programme defines:

```text
coc_configuration_documented   substrate only: the registered coc_nvim tier
                               (configuration_documented) plus documented setup;
                               proves no behavior
coc_actual_client_core         exactly the 42 baseline scenarios above,
                               addressable per host rail
first_class_coc_host           the core plus an optional join point for
                               #11102-lineage specialized cells joining as
                               consumes_if_available (named upstream); while no
                               joined cell has landed it reduces exactly to the
                               core, and is claimable once its joined cells land
                               and pass — never gated on absent optionals
coc_programme_closeout         programme-completion fan-in over independently terminal
                               child propositions, including explicit
                               unsupported / not_proven terminal dispositions
                               for host capability asymmetry; manufactures nothing
```

Profile laws:

1. A stronger profile never erases a narrower valid one.
2. Specialized and optional inputs join only as `consumes_if_available`
   (#10858 edge class); they cannot block or silently widen the bounded core.
3. No stage may silently broaden a proposition; a missing chain link is
   `not_proven`.
4. A source implementation, merged PR, synthetic peer, another editor client,
   a different coc.nvim build/platform, or any DAP receipt can never satisfy
   an actual Vim+Coc or Neovim+Coc scenario.
5. No scenario contributes to a support claim until its executable exact-host
   evidence chain passes.
6. Host capability asymmetry terminates explicitly (`unsupported` or
   `not_proven`) inside the owning leaf; it is never resolved by borrowing the
   other host's pass.
7. Subject separation is total: Vim+Coc, Neovim+Coc, and native Neovim LSP
   are three different identities; no pair may fill the other's rows.

## Evidence boundaries and chain

Each host rail binds its own chain following the issue's declared stage
mapping; each arrow is a different owner, and no owner may widen the
proposition it receives:

```text
coc.vim.bdd.<id>
→ #10674 governed fixture/expectation cell(s)
→ #10678 deterministic action/observation operation(s)
→ #10685-lineage focused Vim + coc.nvim host-leaf observation(s)
→ #10680 editor_client_compat.v1 cell(s) within #10527/#7777 receipt bounds
→ #8967 actual_client_core receipt fan-in
→ #8992/#7122 support projection

coc.neovim.bdd.<id>
→ #10674 governed fixture/expectation cell(s)
→ #10678 deterministic action/observation operation(s)
→ #10704-lineage focused Neovim + coc.nvim host-leaf observation(s)
→ #10680 editor_client_compat.v1 cell(s) within #10527/#7777 receipt bounds
→ #10717 actual_client_core receipt fan-in
→ #8992/#7122 support projection
```

The per-host convergence programs #8962 (Vim rail) and #8978 (Neovim rail)
consume these chains as their host-proof input; the flagged save-cohort
proofs #11125/#11127 emit host-qualified cells against these IDs.

Machine-visible distinction tags reuse existing ledger vocabularies rather
than inventing a second tag language:

| Distinction | Existing repository field/vocabulary consumed |
| --- | --- |
| configuration documented | `policy/lsp-client-support.toml` tier `configuration_documented`, client id `coc_nvim` |
| actual host required | `requires_actual_client_receipt = true`; evidence kind `actual_client` |
| subject = Vim + coc.nvim | integration_mode `coc_language_server`; host segment of the ID namespace (`coc.vim.*`) |
| subject = Neovim + coc.nvim | integration_mode `coc_language_server`; host segment of the ID namespace (`coc.neovim.*`) |
| exact-source evidence | actual-client observation cells on each rail (#10685/#10678/#10704 lineage) |
| public-artifact evidence where separately replayed | separate direct public replay stage feeding #8992 composition; local evidence never relabels upward |
| security-sensitive configuration | this packet's security boundary; #4998 authority |
| optional feature | #10858 `consumes_if_available` edge class |
| not-proven/unsupported allowed disposition | schema enum `editor_client_compat.v1` journeyCell `result ∈ {pass, fail, partial, not_proven, unsupported}`; registry vocabulary `not_proven_unsupported`; #11102 ladder extends with limited/instrument-stage terminations |

The host segments `coc.vim.*` / `coc.neovim.*` and the four profile IDs are
newly defined semantic terms of this packet (declared in §API-Shape of
`acceptance.md`); every other distinction above consumes an existing
registry/schema vocabulary. An `editor_client_compat.v1` schema
(`.ci/schemas/editor-client-compat.v1.schema.json`, #7777) is the single
receipt surface referenced here; no second schema or tag ontology exists
anywhere in this packet.

## Security boundary

The positive workspace-configuration scenarios (`*.edit.04`) admit only the
workspace-contained relative include-path shape already governed by #4998.
Absolute or traversal client include paths (`*.edit.05`) remain a separately
governed/rejected security proposition and can never become ordinary positive
behavior merely because a generic client could transmit them. coc.nvim-provided
configuration is never treated as trusted machine provenance.

## Authority and ownership

Consumed, never cloned: #8949/#10658 (product/train controllers), #8956
(subject/config/root pin authority, open), #7762/#7743 (activation/root
authorities), #6736 (configuration transactionality), #4998 (include-path
security), #10894 (shared host execution primitive), #10858 (typed
edge/profile vocabulary), #10527/#7777 (generic durable receipt semantics and
bounds), #3983 +
[`SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md) (spec method),
`policy/lsp-client-support.toml` via #6739 (registered tiers).

Owned downstream, named here as boundaries only:

```text
#10674 (+#11107 freshness fixtures)   fixture/expectation cells binding each scenario ID
#10678 / #11112                       deterministic shared driver action/observation operations
#10685 / #10704                       foundational vim/neovim Coc adapter leaves
#11125 / #11127                       flagged save-cohort Vim/Neovim actual-host proofs
#11302/#11307/#11314                  optional read/display/topology cells, Vim rail
#11303/#11309/#11317                  optional read/display/topology cells, Neovim rail
#10680                                editor_client_compat.v1 projection for Coc baseline observations
#8967 / #10717                        per-rail actual_client_core promotable-receipt fan-in
#8962 / #8978                         per-host actual-evidence convergence programs
#8992 / #7122                         support projection (registry authority)
#11102                                specialized journey extension owning future scenario families
```

This bundle creates no fixtures, provisions no editor, launches no coc Node or
perllsp process, implements no server/client behavior, produces no receipts,
awards no support, changes no registered tier, and submits nothing upstream.

## Stable versus mutable information

Durable bytes here carry stable identities only: scenario IDs, profile names,
authority references, evidence-stage vocabulary, and the registry tier record
as currently landed. Current main SHA, open PR numbers, check colours,
writers/models, live upstream state beyond recorded references, and wall-clock
readiness never enter these files.

## Alternatives rejected

- **Create a new `.feature`-file subsystem plus a feature-status generator:**
  rejected; no such ledger exists on current main, and inventing a parallel
  format beside the shipped `.spec/` ledger authority is exactly the
  second-authority drift the campaign forbids. The evolution is recorded.
- **Fold scenario definitions into #10674 fixtures:** rejected; the fixture
  would become the first expression of product semantics, the precise
  inversion this issue exists to prevent.
- **Encode scenario IDs only inside receipts:** rejected; receipts are
  downstream evidence, and the proposition being evidenced must pre-exist as
  a checked identity receipts can bind to.
- **Define unqualified `coc.bdd.*` IDs shared across hosts:** rejected; host
  identity is load-bearing per #10815, and a shared unqualified row lets one
  host's example table stand in for the other (falsifier).
- **Pre-state coc.nvim/perllsp digests before #8956 lands:** rejected; this
  packet pins nothing and consumes #8956 by reference so subject drift fails
  loudly there, once.
- **Make the specialized #11102 journeys part of this baseline:** rejected;
  unimplemented breadth must not block the bounded core (#10858 laws).
- **Reuse sibling namespaces (`vim.bdd.*`) for a Coc rail:** rejected;
  `vim.bdd.*` belongs to the Vim + vim-lsp programme; conflating plugin-
  distinct clients under one prefix would blur exactly the subject boundary
  this packet exists to enforce.

## Prior art / duplicates

- `.spec/11371-vim-bdd-journeys/` (closed #11371) — same journey-ledger
  discipline for Vim + vim-lsp; different plugin/subject, deliberately kept
  distinct so host-rail tags cannot blur clients. Referenced, not duplicated.
- `.spec/11178-lite-xl-bdd-journeys/` — Lite XL journey ledger; established
  the `<client-prefix>.bdd.<family>` namespace convention consumed here.
- `.spec/11717-emacs-train-specs/specs.ledger.json` — Emacs spec-ledger
  lineage; same checked-discipline ledger pattern.
- `docs/EDITORS/COC_NEOVIM_SETUP.md` — setup prose consumed as documentation
  provenance; carries no normative scenario semantics.
- `policy/lsp-client-support.toml` — registered support tiers; this packet
  feeds future Coc rows via #8992/#7122 and changes no tier itself.

No prior `.spec` packet encodes Coc user-journey scenarios; nothing here
duplicates an existing authority.

## Links

- Issue: [#10815](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10815)
- Family brief: [#10658](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10658) decomposition comment `[DECOMP:BRIEF:coc-receipt-cells]`
- Controllers: #8949 / #10658; subject/config/root pin authority: #8956
- Fixture consumer: #10674; specialized extension: #11102
- Host leaves: #10685 (Vim rail) / #10704 (Neovim rail); ops #10678/#11112
- Receipts/fan-in/support surfaces: #10680, #8967/#10717, #10527/#7777, #8992/#7122
- Shared profile vocabulary: #10858; spec method: #3983 and `docs/reference/SPEC_TEMPLATE.md`

## Scope boundary

In scope: exactly this directory's `context.md`, `acceptance.md`, and
`checklist.md`.

Out of scope: fixture sources/oracles (#10674-family), provisioning or running
Vim/Neovim/coc.nvim/perllsp, host drivers/runners, server/client behavior
changes, receipts or their schemas' semantics, support registry mutation
(`policy/lsp-client-support.toml`), the specialized #11102 journeys, docs
prose beyond generated spec/status outputs, CI workflow edits, external
upstream submission, and any new Gherkin runner/format infrastructure.
