# Acceptance Criteria: #11369 — exact vim-lsp subject pin and canonical client configuration contract

## §Behavior

| Input / Condition | Expected Result | Notes |
|---|---|---|
| Validator run on a conforming tree | exit 0, prints three stable content digests | `python scripts/ux/validate_vim_vim_lsp_contract.py` |
| Second validator run, unchanged tree | byte-identical output | determinism/offline law |
| Subject manifest with floating ref (e.g. `master`) as `selected_commit` | validation fails | negative control 1 |
| Selected ref resolving to different tree bytes without reviewed update | manifest must be edited; smoke script binds checkout HEAD to the governed pin and fails closed otherwise | negative control 2 |
| Theoretical upstream Vim minimum treated as support floor | forbidden: prerequisites block disclaims it; validator rejects support keys | negative control 3 |
| Registration argv other than `["perllsp","--stdio"]` | validation fails | negative control 4 |
| Root markers/filetype policy copied into config contract | forbidden; driver's inline marker use cross-checked against #7762 manifest | negative control 5 |
| Absolute/traversal path as positive include-path example | forbidden by contract law text the validator enforces | negative control 6 |
| `g:lsp_experimental_workspace_folders` enabled by inference | default must be literal `false`; enablement requires #10960 | negative control 7 |
| Version-sensitive API labelled stable without source evidence | every classification requires pinned-tree blob evidence rows | negative control 8 |
| Host behavior/support/public state key added to subject/config objects | validation fails via forbidden-key scan | negative control 9 |
| Host script keeping an independent conflicting pin/config copy | smoke redirect enforced; embedded commit literals rejected | negative control 10 |

All checks pass: `python scripts/ux/validate_vim_vim_lsp_contract.py`
No Rust code touched — fmt/clippy/test suites unaffected.
Whitespace clean: `git diff --check`.

## §Hazards

| Class | Invariant | Surface (specific file/fn this change touches) | Required adversarial test |
|---|---|---|---|
| Test-encodes-the-bug | validator mutations must fail for their own negative-control reason | `scripts/ux/validate_vim_vim_lsp_contract.py` | mutation checks executed pre-commit (NC1/4/5/7/8/9/10 all discriminated) |
| Coverage/measurement integrity | digest output stable across runs; no wall-clock/network inputs to validation | `validate_vim_vim_lsp_contract.py::canonical_digest` | double-run byte-equality check |
| Protocol-safety | executable identity exactly `perllsp --stdio` | `vim-vim-lsp-configuration.v1.json` registration.command_identity | NC4 mutation |
| ID/ref-space collision | schema_version namespaces (`vim_lsp_subject.v1`, `vim_lsp_configuration.v1`, `vim_lsp_public_surface.v1`) unique vs existing editor-client manifests | `.ci/editor-clients/*.v1.json` | grep of sibling manifests during authoring |

N/A — parser/DAP/LSP-server hazard classes: this change touches no production
Rust surface.

**Subsystem-specific defaults consulted**: docs/policy/FILE_POLICY.md (new
non-Rust files fall under existing `.ci/**`, `scripts/**`, `.spec/**` allowlist
globs).

## §Contracts

| Contract | Source document + section | How this change satisfies or extends it |
|---|---|---|
| Root/filetype authority consumed not copied | #7762 activation-root manifest | root_uri_contract references the manifest by path; carries no marker list; driver cross-check |
| Configuration field authority | #6736 catalog (`crates/perl-lsp-rs-core/src/configuration_authority/catalog.rs`) | workspace-configuration channel admits only catalog fields whose sources include WorkspaceConfiguration; `.perl-lsp.toml` stays preferred |
| Include-path security | #4998 | positive example is workspace-contained relative paths only |
| Executable identity | #7691/#7760 canonical launch `perllsp --stdio` | command identity law + validator equality check |
| Upstream subject identity model | lsp4ij released-fixture manifest pattern | commit + git-tree-sha1 + per-file blob digests |
| Claim-boundary honesty | zed public-registry-subject receipt shape | claim_boundary fields on all three artifacts |

## §API-Shape

N/A — no new public Rust API. New repository artifacts:

| Item | Kind | Shape | Dup-risk (grep result) | Caller count |
|---|---|---|---|---|
| `vim-vim-lsp-subject.v1.json` | checked authority | `schema_version: vim_lsp_subject.v1` | none found in `.ci/editor-clients/` | consumers from later leaves |
| `vim-vim-lsp-configuration.v1.json` | checked authority | `schema_version: vim_lsp_configuration.v1` | none found | same |
| `vim-vim-lsp-public-surface.v1.json` | checked inventory | `schema_version: vim_lsp_public_surface.v1` | none found | same |
| `scripts/ux/validate_vim_vim_lsp_contract.py` | offline validator | stdlib-only CLI | none found under `scripts/` | CI/lane proof |

## §Test-Grid

| Scenario | Kind | Check | Invariant discharged |
|---|---|---|---|
| Conforming artifacts | positive | validator exit 0 + digests | baseline holds |
| Deterministic generation twice | determinism | two runs, identical stdout | offline repeatability |
| Floating branch pin | adversarial | NC1 mutation failed as expected | pin is content-bound |
| Command drift | adversarial | NC4 mutation failed as expected | exact `perllsp --stdio` |
| Marker-policy copy/drift | adversarial | NC5 mutation failed as expected | #7762 sole authority |
| Workspace folders by inference | adversarial | NC7 mutation failed as expected | default off |
| Stable label without evidence | adversarial | NC8 mutation failed as expected | source-bound classifications |
| Support-state smuggling | adversarial | NC9 mutation failed as expected | subject purity |
| Independent pin copy returns | adversarial | NC10 mutations (regex + embed) failed as expected | single governed pin |

## §Blast-Radius

| Consumer | Path | Dependency type | Impact | Required update |
|---|---|---|---|---|
| vim/vim-lsmoke receipt producer | `scripts/ux/vim_vim_lsp_smoke.sh` | consumes pin from new manifest | pin value identical to previous literal (`e10d186…c2b`); behavior unchanged | done in this PR |
| activation/root smoke | `scripts/ux/vim_activation_root_smoke.sh` | untouched | none | none |
| Vim setup docs | `docs/EDITORS/VIM_SETUP.md` | prose only | none (docs prose out of scope) | none |
| Later host journeys/fixtures (#10938/#10944/#7712/#10974/#10978) | future leaves | consume artifacts by reference | unblocked | consume, do not copy |

Must-not-touch boundary: production crates, CI workflows, semantic fixtures,
support registry, `docs/**` prose, external upstream surfaces.
