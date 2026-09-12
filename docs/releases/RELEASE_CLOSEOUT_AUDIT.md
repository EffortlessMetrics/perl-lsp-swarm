# Release Closeout Audit

> Re-runnable distribution-channel verification checklist. Run after every
> tag is cut. Per-release populations live alongside the release notes
> (e.g. `0.14.0-closeout-audit.md`).

The 0.14.0 closeout left users on 0.13.3 because some channels never
published. This doc is the guardrail that prevents the same gap from
recurring in future releases.

## Why the gap exists

Different channels are started in different ways. `release.yml` dispatches some
of them for you; others require an explicit `workflow_dispatch` you run
yourself. If you only tag and assume "the workflow orchestrator handles the
rest," the dispatch-only channels silently stay on the prior version.

Note that on an orchestrated cut nothing is started by `release:published`. The
Release is created by `release.yml` using `secrets.GITHUB_TOKEN`, and
[events triggered by that token do not create workflow
runs](https://docs.github.com/en/actions/security-for-github-actions/security-guides/automatic-token-authentication#using-the-github_token-in-a-workflow).
The `release:published` triggers on the package-channel workflows apply only
when someone publishes a Release out of band — by hand, with a PAT, or from a
GitHub App (#15454).

| Channel | Trigger | Auto on tag? |
|---|---|---|
| GitHub Release (binaries) | `release.yml` on tag push | Yes |
| crates.io | `publish-crates.yml` from `release.yml` | Yes (with new-crate burst cap) |
| Homebrew tap | `brew-bump.yml` dispatched from `release.yml` | Yes (tap-repo PR still needs merge) |
| Scoop bucket | `scoop-bump.yml` dispatched from `release.yml` | Yes (bucket-repo PR still needs merge) |
| Chocolatey | `chocolatey-bump.yml` dispatched from `release.yml` | Yes (package submission may queue) |
| Winget (repo-local) | `winget-bump.yml` dispatched from `release.yml` | Yes (upstream submission still manual) |
| VS Code Marketplace | `publish-extension.yml` | **No - `workflow_dispatch` only** |
| Open VSX | `publish-extension.yml` | **No - `workflow_dispatch` only** |
| Docker (Hub + GHCR) | `docker-publish.yml` | **No - `workflow_dispatch` only** |

**Three channels (Docker, VS Code Marketplace, Open VSX) require a manual
`workflow_dispatch` by the operator.** They are the most common 0.14.0-style
"still pending" channels. Brew/Scoop/Chocolatey/Winget are also
`workflow_dispatch` workflows, but `release.yml` dispatches them for you — so
they need no operator action to start, and each opens a downstream package-repo
PR that must merge before users see the bump.

## Per-channel verification

Run from the publishing repo (`EffortlessMetrics/perl-lsp`) checkout.
Replace `vX.Y.Z` with the actual tag.

### 1. GitHub Release

```bash
gh release view vX.Y.Z --json name,isDraft,isPrerelease,publishedAt,assets \
  | jq '{name, isDraft, isPrerelease, publishedAt, asset_count: (.assets | length)}'
```

Expected: `isDraft=false`, asset count matches the platform matrix
(typically 5 platforms x {tarball, sha256, sig} = ~15).

If draft: `gh release edit vX.Y.Z --draft=false`. Publishing it yourself this
way *does* fire `release:published` downstream, because it is your credential
rather than the workflow's `GITHUB_TOKEN` — so the package-channel workflows
start from that event. On a cut where `release.yml` already dispatched them,
check for a second run per channel before re-dispatching by hand.

### 2. crates.io

Primary packages (update list if workspace top-level binaries change):

```bash
for crate in perllsp perl-lsp-rs perl-parser perl-dap; do
  echo -n "$crate: "
  cargo search "$crate" --limit 1 | head -1
done
```

Full inventory (all crates listed in `[workspace.metadata.publish.allow]`):

```bash
cargo metadata --format-version=1 --no-deps \
  | jq -r '.metadata.publish.allow[]' \
  | while read crate; do
      printf "%-40s " "$crate"
      cargo search "$crate" --limit 1 | head -1 || echo "NOT FOUND"
    done
```

If any are stuck below `X.Y.Z`: a common cause is the new-crate burst
rate limit (crates.io: burst=5, refill 1/10min). Remediation:
`just publish-new-crates` per `docs/reference/MANUAL_PUBLISH_NEW_CRATES.md`.

### 3. VS Code Marketplace

```bash
# Listing version (BSD grep portable; macOS/Linux both work):
curl -s "https://marketplace.visualstudio.com/items?itemName=EffortlessMetrics.perl-lsp-rs" \
  | grep -oE 'data-version="[^"]+"' | head -1 | sed -E 's/.*"([^"]+)"/\1/'
```

Or check programmatically via the marketplace gallery API. If not at
`X.Y.Z`:

```bash
gh workflow run publish-extension.yml -f version=X.Y.Z
```

Then watch the run; this requires `VSCE_PAT` to be set.

### 4. Open VSX

```bash
curl -s "https://open-vsx.org/api/EffortlessMetrics/perl-lsp-rs" \
  | jq '.version'
```

Open VSX is bundled into the same `publish-extension.yml` workflow as
the VS Code Marketplace, but uses `OVSX_PAT` instead. Same dispatch:

```bash
gh workflow run publish-extension.yml -f version=X.Y.Z
```

### 5. Docker

```bash
docker pull effortlessmetrics/perl-lsp:X.Y.Z-perl
docker pull ghcr.io/effortlessmetrics/perl-lsp-perl:X.Y.Z
```

The runtime is the only published image. The unsuffixed tags carried the
Rust build toolchain and are retired (#8980) — do not expect them to
resolve, and do not re-dispatch the publish workflow to try to create them.

Both should resolve. If either fails with "manifest unknown":

```bash
gh workflow run docker-publish.yml -f version=X.Y.Z
```

Requires `DOCKER_USERNAME` and `DOCKER_PASSWORD` for Hub; GHCR
authenticates via `GITHUB_TOKEN`.

### 6. Homebrew tap

```bash
brew update
brew info --json perllsp | jq '.[0].versions.stable'
```

If still on prior version: `brew-bump.yml` is dispatched by `release.yml` on an
orchestrated cut and opens a PR against `EffortlessMetrics/homebrew-perllsp`.
Check that PR was merged:

```bash
gh pr list -R EffortlessMetrics/homebrew-perllsp --state all --limit 5
```

If the PR is open: review and merge it. If the PR doesn't exist:
re-dispatch with `gh workflow run brew-bump.yml -f tag=vX.Y.Z`.

### 7. Scoop bucket

Same pattern as Homebrew:

```bash
gh pr list -R EffortlessMetrics/scoop-perllsp --state all --limit 5
```

Re-dispatch: `gh workflow run scoop-bump.yml -f tag=vX.Y.Z`.

### 8. Chocolatey

```bash
choco search perllsp --exact
```

Chocolatey moderation can queue submissions for hours/days. If
`choco search` returns a stale version, distinguish between "workflow
never fired" and "submitted but queued in moderation":

```bash
# Did the bump workflow run at all for this version?
gh run list --workflow=chocolatey-bump.yml --limit 10 \
  | grep -E "vX\.Y\.Z|completed"
```

If the workflow never ran, dispatch it:

```bash
gh workflow run chocolatey-bump.yml -f tag=vX.Y.Z
```

If the workflow ran successfully but `choco search` is still stale, the
submission is in moderation - nothing to do but wait.

### 9. End-to-end smoke

After the channels above resolve, confirm a fresh install on each
platform works:

```bash
# crates.io install
cargo install perllsp --version X.Y.Z --force
perllsp --version  # -> X.Y.Z

# Homebrew
brew upgrade perllsp
perllsp --version  # -> X.Y.Z

# Docker
docker run --rm effortlessmetrics/perl-lsp:X.Y.Z-perl --version  # -> X.Y.Z
```

For LSP4IJ specifically (the JetBrains plugin that hit the 0.14.x
crash): users typically install `perllsp` via `cargo install`,
Homebrew, or download the GitHub Release binary. Any of those three
must serve the new version for them to actually receive the fix.

## Updating per-release populated audits

After running the checks above for a specific release, create:

```
docs/releases/{X.Y.Z}-closeout-audit.md
```

Populate it from this template with the actual results, then update the
`channels` frontmatter block in the corresponding `vX.Y.Z.md` release
notes file so `notes_status` can flip from `pending` to `closed`.

## Hard rules

- Do not mark a release `notes_status: closed` until every channel above
  resolves to `X.Y.Z` or is documented as deliberately skipped.
- Do not assume `release:published` starts anything on an orchestrated cut.
  It cannot: the Release is published with `GITHUB_TOKEN`. Docker, VS Code
  Marketplace, and Open VSX are dispatch-only; the package channels are
  dispatched by `release.yml`.
- A Homebrew/Scoop/Chocolatey bump that opens a tap-repo PR is not
  the same as a user-facing publish. The PR must merge.

## Related

- 0.14.0 release notes (template): [`v0.14.0.md`](v0.14.0.md)
- Manual new-crate publish: [`../reference/MANUAL_PUBLISH_NEW_CRATES.md`](../reference/MANUAL_PUBLISH_NEW_CRATES.md)
- Release process: [`../RELEASE_PROCESS.md`](../RELEASE_PROCESS.md)
- Release checklist: [`../project/RELEASE_CHECKLIST.md`](../project/RELEASE_CHECKLIST.md)
