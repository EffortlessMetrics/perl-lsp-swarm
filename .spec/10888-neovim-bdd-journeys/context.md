# Context: #10888 — native Neovim LSP user journeys and evidence boundaries in the BDD/spec ledger

## Problem

The native Neovim programme (#7739 product controller, #10501 train controller
candidate, `.spec/11392-native-neovim-train-graph/` topology) begins its
executable graph at the #10502 activation/root envelope. Without an earlier
checked expression of product semantics, every later agent must infer from
fixture bytes and controller prose whether a journey is baseline, deep, or
merely harness-convenient — and fixture authors, host-driver authors,
reviewers, receipt fan-in, support projection, and documentation would each
carry a private copy of "what the native Neovim journey means".

Neovim is also the single editor most at risk of subject confusion in this
repository. Three different registered clients can all be described loosely as
"Neovim": the built-in LSP client (`neovim` tier), Coc running on a Neovim host
(`coc_nvim` tier, ledger `coc.neovim.bdd.*`), and Vim + vim-lsp (`vim.bdd.*`)
which shares much of the same user vocabulary. Without one stable namespace and
an explicit non-substitution law, a receipt from one rail can silently satisfy
another rail's claim.

This bundle encodes one checked normative baseline for the canonical
`Neovim built-in LSP client + perllsp` user journeys so downstream leaves bind
stable scenario IDs instead of re-deriving meaning. It owns behavior wording,
scenario identity, claim-profile membership, and evidence boundaries only.

## Why this approach (ledger-format evolution record)

Issue #10888 names "repository Gherkin + acceptance/spec ledger + generated
`docs/feature_status.md`" and commands `cargo xtask bdd` / `ac-status` /
`docs-check` / `ac-status --check --require-coverage`. Current main has no
Gherkin `.feature` runner surface, no `docs/feature_status.md`, and none of
those xtask subcommands; the repository's existing, shipped BDD/spec-ledger
authority is the `.spec/` packet system governed by
[`docs/reference/SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md) and
#3983 conventions.

Per the issue's own evolution clause ("Run and commit the canonical
projections, including **current equivalents** of ..."), this packet projects
the Gherkin-style Feature/Scenario organization into that existing spec-ledger
system: features map to journey families, scenarios to stable-ID ledger rows,
and step-level executable truth stays downstream. Introducing a new parallel
`.feature` format or a feature-status generator here is out of scope and would
create a second authority.

This is the fourth editor-journey packet to make that projection, following the
landed precedents `.spec/11371-vim-bdd-journeys/` (PR #12070),
`.spec/10815-coc-bdd-journeys/` (PR #12892), and
`.spec/11178-lite-xl-bdd-journeys/`. The same evolution record was accepted in
each.

Keep distinct:

```text
BDD/spec ledger (this packet)
  what the user/editor must observe
  which propositions are core, deep, or distribution-stage
  which evidence stage/subject may satisfy them

#10502-governed fixture and subject envelope (later leaf)
  exact source bytes, anchors, root markers, canonical expectations,
  the pinned client subject, and false-subject mechanics

#10504–#10507 host leaves (later leaves)
  what an actual Neovim process running perllsp actually did
```

A Lua snippet is not the product specification. A ledger sentence is not
executable evidence. They bind through the stable scenario IDs defined below.

## Subject substrate (consumed by reference; nothing re-pinned here)

Unlike the Vim rail — whose #11369/PR #12050 packet already landed
`.ci/editor-clients/vim-vim-lsp-{subject,configuration,public-surface}.v1.json`
— **no native-Neovim subject, configuration, or public-surface artifact exists
in `.ci/editor-clients/` on current main.** Its owner (#10502) is open.

Consequences, deliberately honest rather than convenient:

- no commit, tree, or asset digest is pre-stated anywhere in this packet;
- `neovim.bdd.attach.02` binds the client subject **by reference** to #10502
  and is re-checked when that envelope lands;
- this packet must not mint a second subject pin to fill the gap.

The documented user-visible configuration this ledger describes is the native
built-in shape recorded in `docs/EDITORS/NEOVIM_SETUP.md`. The block below is a
synthesis of that document's separate `lsp/perllsp.lua` and settings snippets,
shown together for orientation; the document remains the authority:

```lua
vim.lsp.config('perllsp', {
  cmd = { 'perllsp', '--stdio' },
  filetypes = { 'perl' },
  root_markers = { { '.perl-lsp.toml', 'Makefile.PL', 'Build.PL', 'cpanfile', 'dist.ini' }, '.git' },
  settings = { perl = { workspace = { includePaths = { 'lib' } } } },
})
vim.lsp.enable('perllsp')
```

This is `vim.lsp.config` / `vim.lsp.enable` notation, **not** nvim-lspconfig
plugin notation. Upstream `nvim-lspconfig` registration and Mason availability
are separate, currently unearned stages (`support.03`, `opt.01`, `opt.02`).

## Stable scenario ID namespace

```text
neovim.bdd.<family>.<nn>
```

`neovim` denotes the Neovim **built-in** LSP client only. The namespace is
deliberately disjoint from the other registered rails:

```text
neovim.bdd.*        Neovim built-in LSP client        (this packet, tier `neovim`)
coc.neovim.bdd.*    Coc on a Neovim host              (.spec/10815, tier `coc_nvim`)
vim.bdd.*           Vim + vim-lsp                     (.spec/11371, tier `vim`)
lite_xl.bdd.*       Lite XL                           (.spec/11178)
```

Three-subject law: a receipt from any one of these rails can never satisfy a
row in another, even when the user-visible sentence reads identically. IDs are
immutable once published; retirement or renumbering routes through a #10888
revision, never silent reuse.

## Journey inventory (baseline = 41 scenarios, optional = 6)

### Feature: Neovim attaches the built-in LSP client to the intended Perl project

```text
neovim.bdd.attach.01–06  native filetype observed before override; built-in
                         client is the attached subject; exact `perllsp --stdio`;
                         governed root wins; sibling/outer root never satisfies;
                         single-file/no-marker disposition explicit and
                         activation implies no POD/XS/template semantic support
```

### Feature: Neovim returns useful current core facts

```text
neovim.bdd.core.01–10    expected diagnostic appears; editing clears exactly it;
                         completion offers and applies the intended item; hover
                         identifies the intended entity; definition opens the
                         intended target; references exclude wrong-root decoys;
                         settings change an independent semantic result;
                         formatting is canonical and idempotent; normal quit
                         orphans no perllsp; unsafe include paths stay governed
```

### Feature: Neovim obeys the selected text-sync envelope (conditional on #8129)

```text
neovim.bdd.sync.01–05    branch A `atomic_incremental`: encoding/advertisement,
                         actual ranged traffic, sequential multi-change,
                         invalid-notification desync, explicit recovery
neovim.bdd.sync.06–10    branch B `full_document_utf16`: UTF-16 selection,
                         actual full-document traffic, ranged refusal,
                         no partial mutation, explicit recovery
```

Both groups are published so each has a stable owner regardless of how the
selection resolves. The selection is **not this packet's to make**: it is
governed by the `nv_release_scope_decision` selecting authority in #11392,
whose `allowed_values` are exactly `full_document_utf16` and
`atomic_incremental` and whose controller node is `nv_ctrl_release_decision`.

That authority records **no current value**. `selected_value` appears in #11392
only as per-node *gate* metadata — `full_document_utf16` qualifies the bounded
release gate, `atomic_incremental` qualifies the deep atomic branch — and a
gate's qualifying condition is not a ruling. Publishing both groups therefore
asserts neither. When the governed selection is established, exactly one group
becomes applicable and the other `not_applicable` by that authority, never by
an assertion here; a stale selection fails closed.

### Feature: Neovim preserves parser and lifecycle truth

```text
neovim.bdd.lifecycle.01–07  non-BMP ranges land on the intended target;
                            executable/data-only/marker-boundary fallback stay
                            distinct; complete-artifact hits keep recovery and
                            limitations; admitted strategy ≠ full fallback;
                            pending initial open is not fake current;
                            superseded parse/effects never publish; same-URI
                            reopen is a new instance and shutdown releases the
                            exact owned work and state
```

### Feature: Neovim support claims remain subject- and stage-bound

```text
neovim.bdd.support.01–08  0.11 floor ≠ current stable (#10508);
                          Linux/macOS/Windows separate (#10508); manual vs
                          nvim-lspconfig vs Mason registry separate (#10511/
                          #10514, independence law #7730); stable vs nightly vs
                          dev-pin channels separate and never satisfied by an
                          aggregate receipt (#10516/#10518/#10520, #7770); exact-source ≠ public-installed; prepared ≠
                          submitted ≠ accepted ≠ released ≠ public (#10511/
                          #10514); virtual documents may stay upstream-dependent
                          without failing core; DAP is a nonblocking sidecar
                          (#10523)
```

### Optional and stronger-profile inputs (never baseline blockers)

```text
neovim.bdd.opt.01–06  upstream nvim-lspconfig registration, Mason package,
                      public release archive replay, extended version/platform
                      matrix rows, virtual-document/upstream-dependent
                      features, nvim-dap sidecar
```

Later leaves may extend this BDD authority through separate stable families via
revision here; they do not cram unimplemented breadth into baseline examples.

## Claim profiles

These are **not new IDs.** They are the six governed claim profiles already
defined in `.spec/11392-native-neovim-train-graph/train.manifest.json` (#11392),
consumed verbatim. Renaming one here, or inventing a parallel set, would fork
the programme vocabulary and prevent downstream evidence from composing across
the journey and train surfaces.

```text
native_neovim_configuration       documented native setup + canonical filetype/root envelope
                                  + settings schema; substrate; proves no behavior; aligns
                                  with the registered `neovim` tier in
                                  policy/lsp-client-support.toml
native_neovim_core                exactly attach.01–06 + core.01–10 (16 rows)
release_v0_18_bounded             the full-document branch sync.06–10, which qualifies under
                                  nv_release_scope_decision, plus only the cells the bounded
                                  public claim requires
native_neovim_deep_lifecycle      core + lifecycle.01–07 + the atomic branch sync.01–05
                                  (nv_deep_atomic_branch); never the full-document branch
native_neovim_first_class         the distribution/public-stage rows support.01–08;
                                  not core and not deep
native_neovim_programme_closeout  fan-in over terminal child propositions
```

Laws:

1. A stronger profile never erases a narrower valid one.
2. Optional rows join only as `consumes_if_available`; absent optionals reduce
   a profile to its core, never gate it.
3. No stage may silently broaden; a missing chain link is `not_proven`.
4. Only Neovim built-in LSP receipts satisfy `neovim.bdd.*`.
5. No scenario contributes to a support tier until its exact-host chain passes.

The registered `neovim` tier is consumed unchanged. This packet mutates no
support registry value; today's tier honestly records that the current UX trace
is a hand-authored Neovim-shaped capability profile that does not launch
Neovim.

## Evidence boundaries and chain

```text
BDD scenario ID (this packet)
  → #10502 fixture/expectation + activation-root cell
  → #10504 core-loop / #10505 sync-branch / #10506 parser-artifact /
    #10507 concurrency-and-cleanup host observation
  → generic `editor_client_compat.v1` cell within #10527/#7777 bounds
  → #10508 version/platform rows and #10516/#10518/#10520 channel rows
  → #10522 / #7122 support projection
```

Every arrow is a different owner; none may widen the proposition it receives.
`#10511`/`#10514` own the external submission and acceptance stages, and
`#10523` owns the DAP sidecar exclusion. `#10858` supplies the typed
edge/profile vocabulary; `#10894` supplies generic host-reliability and cleanup
semantics.

Evidence-stage vocabulary is consumed from the existing generic surfaces
(`editor_client_compat.v1` and the #7122 support dimensions). This packet mints
no Neovim-only verdict scalar and no second evidence vocabulary.

## Security boundary

Neovim can transmit an arbitrary `settings` table to the server. That capacity
is not a licence to normalize unsafe input:

- ordinary positive rows (`core.07`) use only workspace-contained relative
  include paths admitted by the #6736 catalog;
- absolute and traversal include paths are a guard row (`core.10`), governed or
  rejected per #4998, never demonstrated as ordinary behavior;
- no row defines a fixture that activates project-owned capabilities or process
  execution merely because the client can send the field.

## Authority and ownership

| Object | Owner | This packet |
| --- | --- | --- |
| Product controller | #7739 | consumed |
| Train controller / topology | #10501, #11392 (`.spec/11392-native-neovim-train-graph/`) | consumed; claim-profile IDs and the #8129 selecting authority adopted verbatim; no node added or renamed |
| Text-sync envelope ruling | #8129 via `nv_release_scope_decision` (#11392) | consumed as a governed selection; not decided here |
| Activation/root + subject envelope | #10502 | consumed by reference; artifact absent on main |
| Host observation leaves | #10504, #10505, #10506, #10507 | named as owners |
| Version/platform rows | #10508 | named as owner |
| Install channels | #10516, #10518, #10520 | named as owners |
| External packet stages | #10511, #10514 | named as owners |
| Support import/projection | #10522, #7122 | named as owners |
| DAP sidecar exclusion | #10523 | named as owner |
| Receipt semantics | #10527, #7777 | consumed |
| Include-path security | #4998 | consumed |
| Workspace-field admission | #6736 | consumed |
| Root contract | #7762 | consumed |
| Executable identity | #7691 | consumed |
| Registered client tier | `policy/lsp-client-support.toml` (`neovim`, owner issue #6739) | consumed unchanged |

## Stable versus mutable information

Durable bytes here carry only stable semantics: scenario identity, behavior
wording, profile membership, owner issue numbers, and boundary laws. They carry
no commit SHA, PR number, check status, writer identity, review state, branch
name, or wall-clock timestamp. Runtime and transaction state stays in GitHub.

## Alternatives rejected

- **Author `.feature` files and a Gherkin runner**: rejected — would create a
  second BDD authority beside the shipped `.spec/` ledger, and the issue's
  non-goals forbid new runner infrastructure.
- **Add `cargo xtask bdd` / `ac-status` / `docs-check` and generate
  `docs/feature_status.md`**: rejected — those subcommands and that document do
  not exist on main; inventing them here would build a generator whose only
  consumer is this packet, and the issue explicitly scopes out implementation.
- **Fold native-Neovim rows into the existing `coc.neovim.bdd.*` ledger**:
  rejected — different client, different tier, different capability surface;
  merging them is exactly the substitution hazard this packet exists to block.
- **Pin a native-Neovim subject digest now**: rejected — #10502 owns that
  artifact and it is not on main; pre-stating a digest would manufacture
  authority and go stale silently.
- **Assert one #8129 branch to reduce row count**: rejected — the decision is
  not ours, and asserting a branch would make the ledger wrong for half the
  possible rulings.
- **Mirror the ledger into a Rust catalog in this PR**: rejected — the Vim rail
  gained `xtask/src/vim_lsp_cell_catalog/scenario_ledger.rs` only *after*
  #11371 merged, under a separate consumer's ownership. Doing it here would
  widen a spec claim into implementation.
- **Author the structural checker in PowerShell like the coc precedent**:
  rejected — `pwsh` is not available in this repository's Linux toolchain, so
  the precedent's checker cannot actually be executed here. A Python 3 checker
  is portable, present, and genuinely runnable; the deviation is recorded in
  `checklist.md`.

## Prior art / duplicates

- `.spec/11371-vim-bdd-journeys/` — Vim + vim-lsp, 30 IDs. Same discipline,
  different subject. Its `vim.bdd.*` namespace is disjoint from ours by the
  three-subject law.
- `.spec/10815-coc-bdd-journeys/` — Coc on Vim and Neovim hosts, 42 IDs.
  Establishes the host-qualified namespace pattern this packet follows and the
  precedent for binding a subject by reference while its pin authority is open.
- `.spec/11178-lite-xl-bdd-journeys/` — Lite XL, 73 IDs. Source of the
  support-family "subject- and stage-bound" row style.
- #11392 `.spec/11392-native-neovim-train-graph/` — native Neovim node/proposition
  topology and its `cargo xtask check-native-neovim-train` validator. It
  defines train nodes, not user journeys, and does not enumerate `.spec/`
  trees; no overlap and no manifest change is required.
- Searched for an existing `neovim.bdd.*` namespace, native-Neovim `.feature`
  file, or native-Neovim journey ledger on main: none found.

## Links

- Issue: #10888
- Product controller: #7739 — native Neovim product outcome
- Train controller candidate: #10501
- Release sync-envelope decision: #8129
- Activation/root and subject envelope: #10502
- Host leaves: #10504, #10505, #10506, #10507
- Version/platform: #10508
- Install channels: #10516, #10518, #10520
- External packet stages: #10511, #10514
- Support import/projection: #10522, #7122
- DAP sidecar exclusion: #10523
- Receipt semantics: #10527, #7777
- Typed edge/profile vocabulary: #10858
- Host reliability: #10894
- Security and configuration: #4998, #6736, #7762, #7691
- Registered tier owner: #6739
- Spec convention: `docs/reference/SPEC_TEMPLATE.md`, #3983
- Documented setup: `docs/EDITORS/NEOVIM_SETUP.md`

## Scope boundary

In scope: `context.md`, `acceptance.md`, and `checklist.md` under
`.spec/10888-neovim-bdd-journeys/`, plus the generated
`docs/policy/NON_RUST_INVENTORY.md` refresh that tracking three new documents
requires. That file is generated output produced by
`cargo xtask non-rust inventory --write` and is never hand-edited; see
`checklist.md` Step 4 for why its regenerated content also carries one row
already stale on main under #14203/#14161.

Out of scope: Lua, Rust, or shell host implementation; fixture bytes; host
automation or provisioning; semantic oracle generation; the #8129 release
choice; subject-artifact pinning; receipts; support registry values; train
manifest nodes; CI workflows; external upstream submission; and any new BDD
runner or feature-status generator.
