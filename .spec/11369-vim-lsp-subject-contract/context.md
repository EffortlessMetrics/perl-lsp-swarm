# Context: #11369 — exact vim-lsp subject pin and canonical client configuration contract

## Problem

Every Vim-side builder currently selects its own plugin bytes and client
registration shape. `scripts/ux/vim_vim_lsp_smoke.sh` hard-codes one git ref as
a script-local constant while taking `VIM_LSP_DIR`, `VIM`, and `PERLLSP` from
the caller, the activation/root manifest (`.ci/editor-clients/
vim-vim-lsp-activation-root.v1.json`, #7799/#7810/#7762) identifies the client
product but not the selected plugin tree, and no checked artifact records which
public vim-lsp surfaces later drivers may consume. That leaves each future host
PR free to assume different upstream bytes, private APIs, or a second config
schema inside its own lane.

This claim creates one deterministic repository-owned authority answering only:

```text
which exact prabirshrestha/vim-lsp bytes do our receipts target?
which exact Vim-side perllsp registration/configuration do tests and docs consume?
which public vim-lsp action/capability surfaces may later host drivers rely on?
```

## What landed

Three governed artifacts under `.ci/editor-clients/` plus one offline
stdlib validator:

| Artifact | Answers | Key content |
| --- | --- | --- |
| `vim-vim-lsp-subject.v1.json` (`vim_lsp_subject.v1`) | exact upstream subject | commit `e10d186452743beb7b43d2b3427020832f930c2b`, tree digest `dd24cb8e10096c82766143c9fd058105637d72dc` (authored 2026-08-10), entry-file blob identities, load mode, theoretical prerequisites, capability/limitation metadata, pin governance |
| `vim-vim-lsp-configuration.v1.json` (`vim_lsp_configuration.v1`) | canonical registration/config | `perllsp --stdio` command identity law, `perl` allowlist, root contract consumed from #7762 by reference, workspace-configuration channel mapped to #6736/#4998 field authority, experimental workspace folders off, bounded instrument hooks |
| `vim-vim-lsp-public-surface.v1.json` (`vim_lsp_public_surface.v1`) | consumable API/event inventory | every surface classified stable / version-sensitive / instrument-only / not-exposed / unknown, each bound to pinned-tree blob+line evidence |
| `scripts/ux/validate_vim_vim_lsp_contract.py` | deterministic enforcement | schema, pin format, command identity, consumed-not-copied policy, evidence completeness, redirect checks; prints stable content digests |

The copied-pin surface is redirected: `vim_vim_lsp_smoke.sh` now extracts the
expected ref from the governed subject manifest instead of embedding its own
literal, failing closed when the manifest is missing or malformed.

## Pinned subject provenance

Observed live on 2026-08-23 via `git ls-remote
https://github.com/prabirshrestha/vim-lsp.git HEAD refs/heads/master refs/tags/*`
plus a depth-1 fetch resolving commit and tree locally:

```text
selected commit : e10d186452743beb7b43d2b3427020832f930c2b
tree digest     : dd24cb8e10096c82766143c9fd058105637d72dc
commit date     : 2026-08-10T16:38:59+09:00 (#1685 registerCapability fix)
upstream master : e10d186452743beb7b43d2b3427020832f930c2b (coincidence at observation instant)
latest tag      : v0.1.4 -> 3bca7e8c8a794fde38075e7df9d14c286d055a84
                  (2021-01-13 commit; the pinned bytes correspond to no released tag)
```

At the observation instant the pin coincided with upstream master. That
coincidence carries no durability: the pin binds commit plus tree bytes, a
newer master head does not move it, and no floating branch satisfies offline
validation.

## Evidence boundaries (what this claims vs defers)

Claims here:

```text
exact byte identity of the pinned upstream subject;
the canonical registration/configuration shape and its governing laws;
classification and source location of public client surfaces.
```

Deliberately unproven until actual-host leaves own them:

```text
any editor behavior (later journeys/fixtures);
Vim provisioning or host execution (later runners);
maintained/tested Vim version rows (#10966);
workspace-folder support cells (#10960);
receipts, support tiers, registry/docs projections, upstream submissions.
```

The subject object carries upstream *theoretical* prerequisites (Vim 8.1.1035 /
Neovim 0.3 from `doc/vim-lsp.txt`; optional lua acceleration) explicitly marked
as never-a-maintained-support-floor. The validator rejects support/host/public
state keys on these artifacts outright.

## Why this approach

One content-bound packet consumed by reference beats three plausible
alternatives: per-host re-research (every builder rediscovers upstream current
state), vendoring the plugin tree into this repository (a repository-owned Vim
LSP plugin is forbidden by the issue and would fork upstream identity), and
trusting floating branch names (cannot satisfy ordinary offline validation).
The configuration contract consumes #7762's marker/filetype policy and #6736's
field-authority catalog rather than copying them, so drift fails validation
instead of silently diverging.

## Alternatives rejected

- **Keep the pin inside the smoke script:** rejected; a script-local constant
  is invisible to other consumers and was already the exact copied-config
  hazard the issue names (negative control 10). The script now consumes the
  governed manifest.
- **Vendor the pinned vim-lsp tree like lsp4ij's upstream snapshot:** rejected
  for this claim; lsp4ij vendors template bytes it projects into receipts,
  while vim-lsp is executed wholesale from a caller-supplied clean checkout, so
  blob/tree identity plus checkout rules give byte-exactness without owning a
  second copy of upstream. A vendor snapshot remains possible later if an
  offline fixture leaf ever needs it.
- **Fold everything into one JSON file:** rejected; subject identity, client
  configuration, and surface inventory change under different authorities and
  cadences. Separate schema-versioned artifacts let consumers depend on exactly
  what they need.
- **Copy root markers into the config contract for readability:** rejected;
  #7762 stays the sole filetype/root authority. The validator cross-checks the
  driver's marker use against the activation-root manifest so even the
  existing inline consumption cannot drift silently.
- **Encode supported Vim rows now:** rejected; that is #10966's ownership. The
  packet only retains upstream's own compatibility statements as metadata.

## Prior art / duplicates

- `.spec/11716-emacs-support-architecture/` — sibling Emacs architecture
  bundle; referenced, not duplicated. This packet is the Vim subject/config
  substrate leaf in the same programme family (#10906 train controller,
  #7760 product controller).
- `integrations/lsp4ij/upstream/0.20.1/manifest.json` — established pattern for
  pinning upstream subjects with commit/tree/blob digests; reused as the
  digest-evidence model.
- `.ci/fixtures/zed-perl-upstream/receipts/public-registry-subject.v1.json` —
  claim-boundary-first subject receipt shape; reused for honesty fields.
- `docs/reference/SPEC_TEMPLATE.md` and #3983 conventions govern packet shape.

## Links

- Issue: [#11369](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11369)
- Campaign: [#11869](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11869)
- Product/train/controllers: #7760 / #10906 / #7691 / #7762
- Configuration/security authorities consumed: #6736 / #4998
- Landed substrate: #7799 / #7810
- Support/workspace-folder successors: #10966 / #10960
- Direct consumers: #10938 / #10944 / #7712 / #10974 / #10978

## Scope boundary

In scope: `.ci/editor-clients/vim-vim-lsp-{subject,configuration,public-surface}.v1.json`,
`scripts/ux/validate_vim_vim_lsp_contract.py`, the copied-pin redirect in
`scripts/ux/vim_vim_lsp_smoke.sh`, and this directory.

Out of scope: host provisioning/execution, semantic fixtures, LSP behavior,
actual-editor receipts, support registry, docs prose beyond generated/reference
material, external upstream mutation, CI workflow changes.
