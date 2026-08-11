# May 13, 2026 security scan reconciliation

> This is an audit ledger for the original 60 report rows. It is not a security score,
> a claim that the scan covered every repository surface, or an automatic issue-closure mechanism.

## Observation boundary

- Repository: `EffortlessMetrics/perl-lsp-swarm`
- Current-main observation: `c47491b936ac744b3044250ec5ed6218c739b262` on `2026-08-08`
- Source report: `2026-05-13T13:32:50.221Z`; 59 files analyzed; 60 findings

## Verdict counts

| Verdict | Count |
| --- | ---: |
| `false_or_stale_premise` | 0 |
| `landed_not_proven` | 3 |
| `open` | 57 |
| `partially_landed` | 0 |
| `proven_closed` | 0 |
| `transferred` | 0 |

Aggregate counts describe ledger state only. They do not establish repository security.

## Findings

| ID | Severity | Source | Finding | Canonical owner | Candidate / landed state | Verdict | Residual owner |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `MAY2026-HIGH-001` | `HIGH` | `.github/actions/rust-checks/action.yml:92,105,119` | Composite action inputs are injected into shell commands | #5969 | #6096 / `candidate` / `—` | `open` | #5969 |
| `MAY2026-HIGH-002` | `HIGH` | `.github/actions/upload-receipt/action.yml:52,54,57,81,90` | Receipt path input is interpolated into Bash and Python code | #5969 | — / `none` / `—` | `open` | #6124 |
| `MAY2026-HIGH-003` | `HIGH` | `.github/workflows/aggregate-ci-lane-history.yml:24,25,35,37,108` | Privileged workflow runs mutable action refs with a write token | #5995 | #6133 / `candidate` / `—` | `open` | #5995 |
| `MAY2026-HIGH-004` | `HIGH` | `.github/workflows/badge-endpoints.yml:10,11,12,22,24,26,30,31,37` | Badge refresh runs mutable supply-chain code with repository write permissions | #6001 | #6141 / `candidate` / `—` | `open` | #6001 |
| `MAY2026-HIGH-005` | `HIGH` | `.github/workflows/brew-bump.yml:40,241,252,255,296` | Homebrew tap token is exposed in a job that uses mutable checkout actions | #6013 | #6016 / `candidate` / `—` | `open` | #6013 |
| `MAY2026-HIGH-006` | `HIGH` | `.github/workflows/docker-publish.yml:43,266,273` | Workflow dispatch version input is interpolated directly into shell commands | #5987 | #5988 / `candidate` / `—` | `open` | #5987 |
| `MAY2026-HIGH-007` | `HIGH` | `.github/workflows/flake-detection.yml:7,8,13,14,15,16,52,53,54` | workflow_dispatch input is interpolated into bash before execution | #5979 | #5980 / `candidate` / `—` | `open` | #5979 |
| `MAY2026-HIGH-008` | `HIGH` | `.github/workflows/merge-ready-reconciler.yml:10,15,23,26,48,64,69,70` | pull_request runs PR-controlled xtask code with label-write token | #5205 | #5626 / `merged` / `c77cfab5fde0` | `landed_not_proven` | #6124 |
| `MAY2026-HIGH-009` | `HIGH` | `.github/workflows/pipeline-labels.yml:57,58,153,160,167,172` | merge-ready can be granted without current-head gate evidence | #5205 | #5628 / `merged` / `3cd7147aefe5` | `landed_not_proven` | #6124 |
| `MAY2026-HIGH-010` | `HIGH` | `.github/workflows/pipeline-labels.yml:15,16,20,37,39,42,60,62,65` | Mutable github-script action runs with label-write permissions | #5205 | #5628 / `merged` / `3cd7147aefe5` | `landed_not_proven` | #6124 |
| `MAY2026-HIGH-011` | `HIGH` | `.github/workflows/post-merge-corpus-ratchet.yml:29,30,39,41,63,94,104` | Mutable actions execute in a contents-write scheduled job | #6014 | #6017 / `candidate` / `—` | `open` | #6014 |
| `MAY2026-HIGH-012` | `HIGH` | `.github/workflows/post-merge-status.yml:23,24,33,35,77` | Mutable checkout action runs in a repository-write job | #6012 | #6143 / `candidate` / `—` | `open` | #6012 |
| `MAY2026-HIGH-013` | `HIGH` | `.github/workflows/pr-title-check.yml:4,7,8,9,16` | Unpinned github-script runs under pull_request_target with write permissions | #5973 | #5974 / `candidate` / `—` | `open` | #5973 |
| `MAY2026-HIGH-014` | `HIGH` | `.github/workflows/publish-crates.yml:43,242,285,454` | Mutable checkout action can compromise the crates.io publish path | #5993 | #5994 / `candidate` / `—` | `open` | #5993 |
| `MAY2026-HIGH-015` | `HIGH` | `.github/workflows/publish-extension.yml:53,54,64,89,105,119,160,168,201` | Workflow dispatch version input is interpolated into shell before validation | #5985 | #5986 / `candidate` / `—` | `open` | #5985 |
| `MAY2026-HIGH-016` | `HIGH` | `.github/workflows/publish-extension.yml:117,119,134,135,148,160,167,168,184,185,196,201` | Marketplace publish tokens are exposed to unpinned global npm installs | #5985 | #5986 / `candidate` / `—` | `open` | #5985 |
| `MAY2026-HIGH-017` | `HIGH` | `.github/workflows/release-orchestration.yml:34,35,36,37,38,58,59,68,71,74` | High-privilege release workflow interpolates version input into shell before validation | #5989 | #5990 / `candidate` / `—` | `open` | #5989 |
| `MAY2026-HIGH-018` | `HIGH` | `.github/workflows/release.yml:127,129,145,146,196,197,222,267,280,282,284` | Release binaries are built with mutable external tool installs | #5991 | #5992 / `candidate` / `—` | `open` | #5991 |
| `MAY2026-HIGH-019` | `HIGH` | `.github/workflows/ux-regression-gate.yml:11,30,32,33,47,50,116,123,154,158,161,338,363` | Untrusted PR code runs before write-capable tokens and secrets are used | #6027 | #6144 / `candidate` / `—` | `open` | #6027 |
| `MAY2026-HIGH-020` | `HIGH` | `.github/workflows/version-bump.yml:28,29,30,100,103,106,107` | workflow_dispatch version input is interpolated into bash before validation | #5983 | #5984 / `candidate` / `—` | `open` | #5983 |
| `MAY2026-HIGH-021` | `HIGH` | `.github/workflows/version-bump.yml:28,29,30,54,57,63,70,71,74,77,80,86,93,94,97,125,132,146,152` | Privileged release workflow executes latest third-party binaries without pinned integrity | #5983 | #5984 / `candidate` / `—` | `open` | #5983 |
| `MAY2026-HIGH-022` | `HIGH` | `.github/workflows/winget-bump.yml:19,20,21,38,47,48,55,63,68,69,78,95,96,97` | Unvalidated release tag is interpolated into privileged PowerShell steps | #5977 | #5978 / `candidate` / `—` | `open` | #5977 |
| `MAY2026-MEDIUM-001` | `MEDIUM` | `.github/actions/setup-perl-lsp/action.yml:154,161,249` | Reusable action depends on mutable action refs | #5975 | #5976 / `candidate` / `—` | `open` | #5975 |
| `MAY2026-MEDIUM-002` | `MEDIUM` | `.github/actions/setup-rust/action.yml:56,64,72,83,89` | Rust setup action uses mutable third-party action refs | #5999 | #6000 / `candidate` / `—` | `open` | #5999 |
| `MAY2026-MEDIUM-003` | `MEDIUM` | `.github/actions/upload-receipt/action.yml:62` | Artifact upload action is referenced by mutable tag | #5969 | — / `none` / `—` | `open` | #6124 |
| `MAY2026-MEDIUM-004` | `MEDIUM` | `.github/workflows/aggregate-ci-lane-history.yml:55,67,68,69,80,83,93,98` | Untrusted CI artifacts can poison committed lane-history data | #5995 | #6133 / `candidate` / `—` | `open` | #5995 |
| `MAY2026-MEDIUM-005` | `MEDIUM` | `.github/workflows/ci-nightly.yml:8,53,55,56,77,84,105,134` | Pull request jobs run untrusted code with writable PR and issue token scopes | #6028 | #6145 / `candidate` / `—` | `open` | #6028 |
| `MAY2026-MEDIUM-006` | `MEDIUM` | `.github/workflows/ci-security.yml:51,52,60,78,133,169` | Fork PR branch-name collision can skip security scans | #5981 | #6025 / `candidate` / `—` | `open` | #5981 |
| `MAY2026-MEDIUM-007` | `MEDIUM` | `.github/workflows/ci.yml:74,75,77,94,118,205,399` | Fork PR branch-name collision can skip the merge-blocking CI gate | #5981 | #6025 / `candidate` / `—` | `open` | #5981 |
| `MAY2026-MEDIUM-008` | `MEDIUM` | `.github/workflows/ci.yml:187,189,192,324,326,329,550,552,555` | Mutable Codecov action tag receives a secret token | #5973 | #5974 / `candidate` / `—` | `open` | #5973 |
| `MAY2026-MEDIUM-009` | `MEDIUM` | `.github/workflows/methodology-gate.yml:23,26,36` | Mutable action refs allow CI supply-chain execution | #5973 | #5974 / `candidate` / `—` | `open` | #5973 |
| `MAY2026-MEDIUM-010` | `MEDIUM` | `.github/workflows/perl-version-matrix.yml:68` | Mutable checkout action in PR matrix | #5973 | #5974 / `candidate` / `—` | `open` | #5973 |
| `MAY2026-MEDIUM-011` | `MEDIUM` | `.github/workflows/post-publish-smoke.yml:49,50,63,81` | Event-controlled version or ref is interpolated into shell before validation | #5971 | #5972 / `candidate` / `—` | `open` | #5971 |
| `MAY2026-MEDIUM-012` | `MEDIUM` | `.github/workflows/post-publish-smoke.yml:99,137` | Mutable actions can tamper with smoke results | #5971 | #5972 / `candidate` / `—` | `open` | #5971 |
| `MAY2026-MEDIUM-013` | `MEDIUM` | `.github/workflows/pr-plan.yml:39,44,71` | Mutable actions in PR workflow | #6003 | #6137 / `candidate` / `—` | `open` | #6003 |
| `MAY2026-MEDIUM-014` | `MEDIUM` | `.github/workflows/publish-crates.yml:218,219,221,225` | Release or manual version is interpolated into shell before validation | #5993 | #5994 / `candidate` / `—` | `open` | #5993 |
| `MAY2026-MEDIUM-015` | `MEDIUM` | `.github/workflows/release.yml:44,47,58,73,81` | Release tag input is interpolated into shell before validation | #5991 | #5992 / `candidate` / `—` | `open` | #5991 |
| `MAY2026-MEDIUM-016` | `MEDIUM` | `.github/workflows/ripr.yml:66,69,72,74,77,79` | Pull request base branch is interpolated into shell commands | #6124 | — / `none` / `—` | `open` | #6124 |
| `MAY2026-MEDIUM-017` | `MEDIUM` | `.github/workflows/scoop-bump.yml:37,45,46,47,54,67,83,95` | Release tag output is interpolated into PowerShell without validation | #5977 | #5978 / `candidate` / `—` | `open` | #5977 |
| `MAY2026-MEDIUM-018` | `MEDIUM` | `.github/workflows/tokmd.yml:13,15,35,37,38,40,41,43,58,70` | Downloaded tokmd binary is verified only against a co-hosted checksum | #6011 | #6015 / `candidate` / `—` | `open` | #6011 |
| `MAY2026-MEDIUM-019` | `MEDIUM` | `.github/workflows/ux-regression-gate.yml:153,156,158,161` | Codecov action is only pinned to a mutable major tag while receiving a secret | #6027 | #6144 / `candidate` / `—` | `open` | #6027 |
| `MAY2026-MEDIUM-020` | `MEDIUM` | `.github/workflows/vscode-managed-binary-smoke.yml:36,39,65` | Workflow uses mutable GitHub Action tags | #5973 | #5974 / `candidate` / `—` | `open` | #5973 |
| `MAY2026-MEDIUM-021` | `MEDIUM` | `.github/workflows/vscode-published-extension-smoke.yml:43,49,52,71` | Workflow uses mutable GitHub Action tags | #5973 | #5974 / `candidate` / `—` | `open` | #5973 |
| `MAY2026-MEDIUM-022` | `MEDIUM` | `.github/workflows/winget-bump.yml:19,20,21,30` | Privileged workflow uses mutable checkout action tag | #5977 | #5978 / `candidate` / `—` | `open` | #5977 |
| `MAY2026-MEDIUM-023` | `MEDIUM` | `.github/workflows/workflow-policy.yml:30,43` | Workflow uses mutable GitHub Action tags | #5973 | #5974 / `candidate` / `—` | `open` | #5973 |
| `MAY2026-MEDIUM-024` | `MEDIUM` | `.github/workflows/workflow-trigger-lint.yml:24,49` | Workflow uses mutable GitHub Action tags | #5973 | #5974 / `candidate` / `—` | `open` | #5973 |
| `MAY2026-MEDIUM-025` | `MEDIUM` | `vscode-extension/src/gherkinProviders.ts:63,65,317,328,520,525,530,536,541` | Workspace-controlled step regexes can still cause ReDoS | #6066 | #6158 / `candidate` / `—` | `open` | #6066 |
| `MAY2026-MEDIUM-026` | `MEDIUM` | `vscode-extension/src/gherkinProviders.ts:54,61,62,221,231,242,249,250` | Gherkin definition lookup reads too much workspace content without byte limits | #6066 | #5998 / `candidate` / `—` | `open` | #6066 |
| `MAY2026-MEDIUM-027` | `MEDIUM` | `vscode-extension/src/gherkinStepDefinitions.ts:282,284,286,287,290,297` | Step definition generation follows workspace symlinks outside the workspace | #6066 | #5998 / `candidate` / `—` | `open` | #6066 |
| `MAY2026-MEDIUM-028` | `MEDIUM` | `vscode-extension/src/gherkinStepDefinitions.ts:15,369,374,380,385` | Workspace-controlled step regexes can still cause catastrophic backtracking | #6066 | #6158 / `candidate` / `—` | `open` | #6066 |
| `MAY2026-MEDIUM-029` | `MEDIUM` | `vscode-extension/src/gherkinStepDefinitions.ts:307,310,317,318` | Step definition scanning reads up to 500 workspace files concurrently without size limits | #6066 | #5998 / `candidate` / `—` | `open` | #6066 |
| `MAY2026-MEDIUM-030` | `MEDIUM` | `vscode-extension/src/podPreview.ts:267,276,277,278,425,426,430,433,439,495` | POD preview allows workspace-controlled HTML and CSS injection | #6030 | #6047 / `candidate` / `—` | `open` | #6030 |
| `MAY2026-HIGH_BUG-001` | `HIGH_BUG` | `.github/workflows/chocolatey-bump.yml:19,20,104,107,108,121` | Chocolatey PR creation is configured with an unusable token and path | #5977 | #5978 / `candidate` / `—` | `open` | #5977 |
| `MAY2026-HIGH_BUG-002` | `HIGH_BUG` | `.github/workflows/scoop-bump.yml:19,20,103,106,120` | Scoop PR creation uses a token that cannot write to the target repository | #5977 | #5978 / `candidate` / `—` | `open` | #5977 |
| `MAY2026-BUG-001` | `BUG` | `.github/actions/rust-checks/action.yml:79,80,92,93,119,120` | Failure status outputs are not reliably written | #5969 | #6096 / `candidate` / `—` | `open` | #5969 |
| `MAY2026-BUG-002` | `BUG` | `.github/actions/setup-perl-lsp/action.yml:281,286,287` | Source build mode copies perl-dap to an invalid destination | #5975 | #5976 / `candidate` / `—` | `open` | #5975 |
| `MAY2026-BUG-003` | `BUG` | `.github/workflows/pr-plan.yml:39,59,82` | PRs can modify the planner that evaluates themselves | #6003 | #6137 / `candidate` / `—` | `open` | #6003 |
| `MAY2026-BUG-004` | `BUG` | `.github/workflows/ux-regression-gate.yml:50,132,366` | Workflow tests PR head but reports status for a different SHA | #6027 | #6144 / `candidate` / `—` | `open` | #6027 |
| `MAY2026-BUG-005` | `BUG` | `vscode-extension/src/downloader.ts:531,558,560,589` | Release metadata fetch can hang extension startup indefinitely | #6031 | #6020 / `candidate` / `—` | `open` | #6031 |
| `MAY2026-BUG-006` | `BUG` | `vscode-extension/src/testAdapter.ts:198,203,204,206,207` | Test runner buffers unbounded prove output without a timeout | #6033 | #6010 / `candidate` / `—` | `open` | #6033 |

## Update contract

A row moves to `proven_closed` only when the accepted PR is merged, the landed commit is
observed on the recorded current-main SHA, the current source seam is inspected, and
discriminating proof is cited. A closed issue or an existing PR is not closure evidence.

Regenerate with:

```bash
python3 scripts/ci/check_security_reconciliation.py --write
```
