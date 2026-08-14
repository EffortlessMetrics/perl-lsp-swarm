# Release settings closeout

> This document projects a live-settings receipt. Checked-in expectations do not prove that
> GitHub repository, ruleset, environment, token, or secret controls are active.

**Overall state:** `not_proven`

## Subject

- Repository: `EffortlessMetrics/perl-lsp-swarm`
- Source SHA: `56b4499c95687f7e18aca4eaf8b72cef23cd4968`
- Topology digest: `not_proven`
- Observed: `2026-08-08` by `not_proven`

## Channel dispositions

| Channel | Disposition | Owner | Limitation |
| --- | --- | --- | --- |
| `github_release` | `required` | #4145 | — |
| `crates_io` | `required` | #4145 | — |
| `vscode_marketplace` | `required` | #4145 | — |
| `open_vsx` | `required` | #4145 | — |
| `containers` | `not_proven` | #5889 | Release topology has not yet classified container publication as required or deferred for v0.18. |
| `homebrew` | `not_proven` | #5889 | Release topology has not yet classified Homebrew publication as required or deferred for v0.18. |
| `windows_metadata` | `deferred` | #5977 | Repository-local metadata preparation is retained; upstream package-manager submission remains a maintainer action. |

Disposition counts: `deferred`=1, `not_proven`=2, `required`=4.

## Live controls

| Control | State |
| --- | --- |
| `immutable_releases` | `not_proven` |
| `tag_ruleset` | `not_proven` |
| `branch_ruleset` | `not_proven` |
| `actions_policy` | `not_proven` |
| `codeowners` | `not_proven` |
| environment `github-release` (`github_release`) | `not_proven` |
| environment `crates-io` (`crates_io`) | `not_proven` |
| environment `vscode-marketplace` (`vscode_marketplace`) | `not_proven` |
| environment `open-vsx` (`open_vsx`) | `not_proven` |

## Evidence procedure

1. Read the effective repository, Actions, branch/tag ruleset, and environment settings from GitHub.
2. Record exact values and an identified observer; never include secret values.
3. Attach durable GitHub URLs to the observation or administrator closeout comment.
4. Bind the packet to the exact source SHA and release-topology digest.
5. Mark a control `proven` only after its expected value and negative direction are both checked.
6. Run the checker; a checklist or source declaration alone remains `not_proven`.

## Limitations

- This packet is an evidence instrument, not evidence that GitHub settings are active.
- Container and Homebrew channel requirements remain owned by release topology.
- No public publication is authorized by this packet.

Regenerate with:

```bash
python3 scripts/ci/check_release_settings_closeout.py --write
```
