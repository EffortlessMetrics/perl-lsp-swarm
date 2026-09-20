# Context: #11371 — canonical Vim + vim-lsp user journeys and evidence boundaries in the BDD/spec ledger

## Problem

The Vim support programme (#7760 product controller, #10906 train controller,
#7691 canonical actual-host controller) currently begins its executable graph at
the #10938 fixture. Without an earlier checked expression of product semantics,
every later agent must infer from fixture bytes whether a source shape is
baseline, optional, or merely harness-convenient — and builders, fixture
authors, host-driver authors, reviewers, receipt fan-in, support projection,
and generated documentation would each carry a private copy of "what the Vim
journey means".

This bundle encodes one checked normative baseline for the canonical
`Vim + prabirshrestha/vim-lsp + perllsp` user journeys so downstream leaves
bind stable scenario IDs instead of re-deriving meaning. It owns behavior
wording, scenario identity, claim-profile membership, and evidence boundaries
only.

## Why this approach (ledger-format evolution record)

Issue #11371 names "repository Gherkin + acceptance/spec ledger + generated
`docs/feature_status.md`" and commands `cargo xtask bdd` / `ac-status` /
`docs-check`. Current main has no Gherkin `.feature` runner surface, no
generated feature-status document, and none of those xtask subcommands; the
repository's existing, shipped BDD/spec-ledger authority is the `.spec/`
packet system governed by [`docs/reference/SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md)
and #3983 conventions, most recently exercised by the sibling architecture
bundles `.spec/11716-emacs-support-architecture/` (Emacs) and
`.spec/11709-zed-integration-architecture/` (Zed). Per the issue's own
evolution clause ("If command names have evolved, use current canonical
equivalents and record them in the PR review map"), this packet projects the
Gherkin-style Feature/Scenario organization into that existing spec-ledger
system: features map to journey families, scenarios to stable-ID ledger rows,
and step-level executable truth stays downstream. Introducing a new parallel
`.feature` format or a feature-status generator here is out of scope.

Keep distinct:

```text
BDD/spec ledger (this packet)
  what the user/editor must observe
  which propositions are baseline, optional, or stronger-profile
  which evidence stage/subject may satisfy them

#10938-governed fixture (later leaf)
  exact source bytes, anchors, layouts, canonical expectations,
  and false-subject mechanics used to prove each proposition
```

A script is not the product specification. A ledger sentence is not executable
evidence. They bind through the stable scenario IDs defined below.

## Subject substrate (consumed, never re-pinned)

Exact subject/config authority is today's merged #11369 packet (PR #12050):

```text
.ci/editor-clients/vim-vim-lsp-subject.v1.json          pinned commit e10d186452743beb7b43d2b3427020832f930c2b, tree dd24cb8e10096c82766143c9fd058105637d72dc
.ci/editor-clients/vim-vim-lsp-configuration.v1.json    command law ["perllsp","--stdio"], allowlist perl, #7762 root by reference, #6736/#4998 field admission
.ci/editor-clients/vim-vim-lsp-public-surface.v1.json   classified public/version-sensitive/instrument-only/not-exposed surfaces
```

Every behavioral statement below traces to those artifacts, to a named issue
authority, or carries an explicit boundary note. This bundle does not pin a
second copy of subject bytes; replacement of the upstream pin follows #12050's
governance and would require reviewed re-binding here.

## Stable scenario ID namespace

Stable downstream-consumable IDs use the form:

```text
vim.bdd.<family>.<nn>
```

Families, in fixed order:

```text
vim.bdd.attach      Vim attaches vim-lsp to the intended Perl project
vim.bdd.nav         Vim applies ordinary completion and navigation through vim-lsp
vim.bdd.edit        Vim applies server edits and configuration effects
vim.bdd.lifecycle   Vim preserves position, synchronization, currentness, and lifecycle correctness
vim.bdd.opt         optional and stronger-profile inputs (never core blockers)
```

IDs are immutable once published; retirement requires a new revision through
this owning issue, never silent reuse.

## Journey inventory (baseline = 23 scenarios, optional = 7)

Full normative wording, profile membership, and per-scenario evidence owners
are the §Behavior ledger in `acceptance.md`. Summary:

### Feature: Vim attaches vim-lsp to the intended Perl project

```text
vim.bdd.attach.01  native Perl filetype activation precedes any override
vim.bdd.attach.02  exact pinned vim-lsp subject loads
vim.bdd.attach.03  exact perllsp --stdio process is selected
vim.bdd.attach.04  #7762 intended nested/root contract wins
vim.bdd.attach.05  same-name sibling/outer project cannot satisfy root proof
vim.bdd.attach.06  expected diagnostic appears through actual vim-lsp state
vim.bdd.attach.07  editing the exact defect yields current diagnostic state
```

### Feature: Vim applies ordinary completion and navigation through vim-lsp

```text
vim.bdd.nav.01     completion is requested at a real code target
vim.bdd.nav.02     expected server item is consumed through vim-lsp
vim.bdd.nav.03     no-snippet client path produces correct final plain Vim text
vim.bdd.nav.04     hover identifies the intended entity
vim.bdd.nav.05     definition resolves the intended project entity
vim.bdd.nav.06     references contain the governed sites and exclude wrong-root decoys
```

### Feature: Vim applies server edits and configuration effects

```text
vim.bdd.edit.01    rename changes exactly the intended occurrences/files through vim-lsp
vim.bdd.edit.02    formatting reaches the canonical result through client edit application
vim.bdd.edit.03    workspace configuration uses the governed nested perl.* shape
vim.bdd.edit.04    configuration changes an independent semantic result
vim.bdd.edit.05    unsafe external include-path behavior remains governed by #4998
```

### Feature: Vim preserves position, synchronization, currentness, and lifecycle correctness

```text
vim.bdd.lifecycle.01  an operation after non-BMP text reaches the intended target
vim.bdd.lifecycle.02  actual vim-lsp didChange traffic is observed under the current generic contract
vim.bdd.lifecycle.03  post-edit answer belongs to the accepted current generation
vim.bdd.lifecycle.04  close/reopen does not reuse stale document state
vim.bdd.lifecycle.05  normal host shutdown leaves no bound perllsp process
```

### Optional and stronger-profile inputs (never baseline blockers)

```text
vim.bdd.opt.01     experimental workspace folders (#10960)
vim.bdd.opt.02     maintained version/build/platform rows (#10966)
vim.bdd.opt.03     public release archive replay (#10970)
vim.bdd.opt.04     upstream vim-lsp-settings availability (#7712)
vim.bdd.opt.05     interactive UX recommendation (#7771)
vim.bdd.opt.06     alternate yegappan/lsp client (#7717)
vim.bdd.opt.07     Vimspector DAP rail (#7702, separate protocol)
```

Later first-class freshness/save/recovery/reopen/expanded-activation issues may
extend this BDD authority through separate stable scenario families; they do
not cram unimplemented breadth into baseline examples.

## Claim profiles

Consume #10858's shared typed profile semantics (programme-owned profile IDs,
required proposition sets, terminal limitation states, claim ceilings). The
Vim programme defines:

```text
vim_configuration_documented   substrate only: documented setup plus the governed
                               subject/config/public-surface contracts (#12050);
                               already the registered lsp-client-support.v1 tier;
                               proves no behavior
vim_actual_client_core         exactly the 23 baseline scenarios above
vim_first_class_exact_source   vim_actual_client_core plus first-class exact-source
                               host-evidence rows owned by the #10946-lineage leaves
vim_public_artifact            vim_first_class_exact_source plus public release archive
                               replay evidence owned by #10970; separate direct stage
vim_programme_closeout         programme-completion fan-in over independently terminal
                               child propositions; manufactures nothing
```

Profile laws:

1. A stronger profile never erases a narrower valid one.
2. Optional scenarios exist as `consumes_if_available` inputs only; their
   absence cannot block or silently extend the bounded core (#10858 edge
   class), and their existence cannot become a core blocker.
3. No stage may silently broaden a proposition; a missing chain link is
   `not_proven`.
4. A source implementation, merged PR, synthetic peer, another client, another
   Vim build/platform, or a DAP receipt can never satisfy an actual
   Vim/vim-lsp scenario.
5. No scenario contributes to a support claim until its executable exact-host
   evidence chain passes.

## Evidence boundaries and chain

Every baseline scenario binds one downstream evidence chain; each arrow is a
different owner, and no owner may widen the proposition it receives:

```text
vim.bdd.<id>
→ #10938 governed fixture/oracle cell(s)
→ #10944/#10946/#10951/#10955/#10958 raw host observation(s)
→ generic editor_client_compat.v1 cell(s)
→ #10962 exact-subject receipt fan-in
→ #10974/#7122 support cell/profile
→ #10978 generated prose
```

Machine-visible distinction tags reuse existing ledger vocabularies rather
than inventing a Vim-specific tag language:

| Distinction | Existing repository field/vocabulary consumed |
| --- | --- |
| configuration documented | `policy/lsp-client-support.toml` tier `configuration_documented` |
| actual host required | `requires_actual_client_receipt = true`; evidence kind `actual_client` |
| subject = Vim + vim-lsp | client id `vim`; integration_mode `vim_lsp_plugin` |
| exact-source evidence | #10946-lineage exact-source observation rows |
| public-artifact evidence | #10970 public replay stage (separate direct owner) |
| baseline core | profile membership `vim_actual_client_core` |
| optional / consumes-if-available | #10858 `consumes_if_available` edge class |
| security-sensitive configuration | this packet's security boundary; #4998 authority |
| not-proven / unsupported | tier `not_proven_unsupported` |
| not exposed by pinned client | `vim-vim-lsp-public-surface.v1.json` classification values incl. `unknown_not_proven` |

## Security boundary

The positive workspace-configuration scenarios (`vim.bdd.edit.03`,
`vim.bdd.edit.04`) admit only the workspace-contained relative path shape
already governed by #4998 and encoded in the #12050 configuration contract
(single positive example `perl.workspace.includePaths = lib`). Absolute or
traversal client settings remain a separately governed/rejected security
proposition and can never become ordinary positive behavior merely because a
generic client could transmit them. vim-lsp-provided configuration is never
treated as trusted machine provenance.

## Authority and ownership

Consumed, never cloned: #7760/#10906/#7691 (product/train/host controllers),
#11369 via PR #12050 artifacts (subject/config/surface substrate), #7762
(filetype/root sole authority), #6736 (workspace-field admission catalog),
#4998 (include-path security), #10527/#7777 (generic durable receipt
semantics), #10858 (typed edge/profile vocabulary), #3983 +
[`SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md) (spec method).

Owned downstream, named here as boundaries only:

```text
#10938  fixture/oracle cells binding each scenario ID
#10944/#10946/#10951/#10955/#10958  raw actual-host observations
editor_client_compat.v1 producers   generic receipt cells
#10962  exact-subject receipt fan-in
#10974 / #7122  support cell/profile projection (registry authority)
#10978  generated prose
#10960  experimental workspace-folder cell (default stays off)
#10966  maintained Vim version/build/platform rows
#10970  public release archive replay
#7712   upstream vim-lsp-settings availability (external checkpoint)
#7771   interactive UX recommendation
#7717   alternate yegappan/lsp client generation (separate exact subject)
#7702   Vimspector DAP (separate protocol rail)
```

This bundle creates no fixtures, provisions no Vim, launches no vim-lsp,
implements no server/client behavior, produces no receipts, awards no
support, and submits nothing upstream.

## Stable versus mutable information

Durable bytes here carry stable identities only: scenario IDs, profile names,
authority references, evidence-stage vocabulary, and the already-pinned
subject digests as recorded in the #12050 manifests. Current main SHA, open
PR numbers, check colours, writers/models, live upstream state beyond the
recorded pins, and wall-clock readiness never enter these files.

## Alternatives rejected

- **Create a new `.feature`-file subsystem plus a feature-status generator:**
  rejected; no such ledger exists on current main, and inventing a parallel
  format beside the shipped `.spec/` ledger authority is exactly the second-
  authority drift the campaign forbids. The evolution is recorded instead.
- **Fold scenario definitions into the #10938 fixture:** rejected; the fixture
  would become the first expression of product semantics, the precise
  inversion this issue exists to prevent.
- **Encode scenario IDs only inside receipts:** rejected; receipts are
  downstream evidence, and the proposition being evidenced must pre-exist as
  a checked identity receipts can bind to.
- **Make optional/stronger scenarios part of the baseline blocker set:**
  rejected; unimplemented breadth must not block the bounded core, and a
  stronger profile must never erase a narrower valid one (#10858 laws).
- **Re-pin or copy subject bytes into the journey packet:** rejected; #12050
  artifacts are consumed by reference so subject drift fails there, loudly.

## Prior art / duplicates

- `.spec/11716-emacs-support-architecture/` (PR #11811 lineage) — sibling
  Emacs architecture/evidence-boundary bundle; same checked-discipline shape
  (identity tables, evidence ceilings, falsifier grid, deterministic
  structural proof). Referenced, not duplicated; that packet owns Emacs
  cohorts, not Vim journeys.
- `.spec/11709-zed-integration-architecture/` (PR #12051) — Zed decision-map
  bundle merged the same day; same SPEC_TEMPLATE projection pattern.
- `.spec/10894-editor-host-reliability/` (PR #11811) — generic host reliability
  authority projected as a spec bundle; consumed by the host-observation
  owners above.
- `policy/lsp-client-support.toml` — registered support tiers; this packet
  feeds its future Vim rows via #10974/#7122 and changes no tier itself.

No prior `.spec` packet encodes Vim user-journey scenarios; nothing here
duplicates an existing authority.

## Links

- Issue: [#11371](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11371)
- Campaign: [#11869](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11869)
- Subject/config authority: [#11369](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11369) merged as PR #12050 (`.ci/editor-clients/vim-vim-lsp-*.v1.json`)
- Controllers: #7760 / #10906 / #7691; root authority: #7762
- Fixture consumer: #10938; host proof consumers: #10944/#10946/#10951/#10955/#10958
- Receipts/fan-in/support/prose: #10527/#7777, #10962, #10974/#7122, #10978
- Shared profile vocabulary: #10858; spec method: #3983 and `docs/reference/SPEC_TEMPLATE.md`

## Scope boundary

In scope: exactly this directory's `context.md`, `acceptance.md`, and
`checklist.md`.

Out of scope: fixture sources/oracles (#10938), Vim/vim-lsp provisioning or
execution, host drivers/runners, server/client behavior changes, receipts or
their schemas' semantics, support registry mutation (`policy/lsp-client-support.toml`),
docs prose beyond generated spec/status outputs, CI workflow edits, external
upstream submission, and any new Gherkin runner/format infrastructure.
