# Implementation Checklist: #11369 — exact vim-lsp subject pin and canonical client configuration contract

## Change order (compiles/validates at each step)

### Step 1: Pin the exact upstream subject
- **File:** `.ci/editor-clients/vim-vim-lsp-subject.v1.json` (CREATE)
- **Change:** record repository, selected commit, tree digest, commit date,
  entry-file blob identities, load mode, theoretical prerequisites with
  never-a-support-floor disclaimer, capability/limitation metadata, pin
  governance, and the deliberate verification observation.
- **Details:** digests resolved live via `git ls-remote` plus a depth-1 fetch of
  `e10d186452743beb7b43d2b3427020832f930c2b`; tree
  `dd24cb8e10096c82766143c9fd058105637d72dc`.
- **Verify:** JSON parses; every blob SHA is 40-hex.

### Step 2: Canonical registration/configuration contract
- **File:** `.ci/editor-clients/vim-vim-lsp-configuration.v1.json` (CREATE)
- **Change:** encode command identity (`["perllsp","--stdio"]`), `perl`
  allowlist, root contract consumed from #7762 by reference, workspace
  configuration channel mapped to #6736/#4998 authority with a single
  workspace-contained relative positive example, experimental workspace folders
  default off, bounded instrument hooks, and the seven governing laws.
- **Depends on:** Step 1 (subject_reference).
- **Verify:** JSON parses; no marker list present.

### Step 3: Public surface inventory
- **File:** `.ci/editor-clients/vim-vim-lsp-public-surface.v1.json` (CREATE)
- **Change:** classify registration/root, lifecycle events, diagnostics state,
  request channel, completion conversion, edit application, config refresh,
  stop/status/log surfaces, didChange instrumentation seam, and experimental
  workspace folders; each row bound to pinned-tree blob+line evidence.
- **Depends on:** Step 1 (same pinned tree for evidence blobs).
- **Verify:** classifications use only the fixed vocabulary; instrument-only
  rows carry justification.

### Step 4: Offline validator
- **File:** `scripts/ux/validate_vim_vim_lsp_contract.py` (CREATE)
- **Change:** stdlib-only checks over Steps 1-3 plus cross-artifact rules:
  driver marker consumption vs #7762 manifest, smoke-script pin redirect,
  forbidden-key scan, evidence completeness, deterministic digest output.
- **Verify:** `python scripts/ux/validate_vim_vim_lsp_contract.py` exits 0.

### Step 5: Redirect the copied pin
- **File:** `scripts/ux/vim_vim_lsp_smoke.sh` (MODIFY)
- **Change:** drop the hard-coded `expected_vim_lsp_ref=<sha>` literal; extract
  the ref from the governed subject manifest via perl/JSON::PP after tool
  checks, failing closed when missing/malformed.
- **Details:** pin value unchanged (`e10d186…c2b`) — behavior-neutral redirect.
- **Verify:** `bash -n scripts/ux/vim_vim_lsp_smoke.sh`.

### Step 6: Spec packet + final verification
- **Files:** this directory (CREATE)
- **Verify:**
  - `python scripts/ux/validate_vim_vim_lsp_contract.py` twice → identical output;
  - negative-control mutations discriminated (NC1/4/5/7/8/9/10);
  - `git diff --check`;
  - no Rust touched → fmt/clippy/test suites unaffected.

## Callers and consumers

- `vim-vim-lsmoke.sh` consumes `vim-vim-lsp-subject.v1.json` (pin extraction).
- Later leaves (#10938/#10944/#7712/#10974/#10978) consume all three artifacts
  by reference.

## Scope boundary

Files IN scope: the three `.ci/editor-clients/vim-vim-lsp-*.v1.json` artifacts,
`scripts/ux/validate_vim_vim_lsp_contract.py`,
`scripts/ux/vim_vim_lsp_smoke.sh`, `.spec/11369-vim-lsp-subject-contract/`.

Files OUT of scope: everything else — explicitly production crates, CI
workflows, fixtures, receipts, support registry, docs prose, upstream surfaces.

## Flags for builder

- The pin coinciding with current upstream master at observation time is
  recorded as an observation, never as durability.
- Any future subject bump requires re-resolving commit AND tree bytes plus
  refreshing entry-file/blob evidence in all three artifacts together.
