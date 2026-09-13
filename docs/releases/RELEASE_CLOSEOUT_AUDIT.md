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
| Homebrew tap | `brew-bump.yml` dispatched from `release.yml` | Starts automatically; opens a PR on the owned tap that still needs merge |
| Scoop bucket | `scoop-bump.yml` dispatched from `release.yml` | Starts automatically; opens a repo-local manifest PR — `ScoopInstaller/Main` submission stays manual |
| Chocolatey | `chocolatey-bump.yml` dispatched from `release.yml` | Starts automatically; opens a repo-local package PR — community-repo submission stays manual |
| Winget | `winget-bump.yml` dispatched from `release.yml` | Starts automatically; opens a repo-local manifest PR — `winget-pkgs` submission stays manual |
| VS Code Marketplace | `publish-extension.yml` | **No - `workflow_dispatch` only** |
| Open VSX | `publish-extension.yml` | **No - `workflow_dispatch` only** |
| Docker (Hub + GHCR) | `docker-publish.yml` | **No - `workflow_dispatch` only** |

**Three channels (Docker, VS Code Marketplace, Open VSX) require a manual
`workflow_dispatch` by the operator.** They are the most common 0.14.0-style
"still pending" channels. Brew/Scoop/Chocolatey/Winget are also
`workflow_dispatch` workflows, but `release.yml` dispatches them for you, so
they need no operator action to *start*.

Starting is not publishing, and the four differ in what they leave behind:

- `brew-bump.yml` opens a PR against the owned tap
  `EffortlessMetrics/homebrew-tap` using `HOMEBREW_TAP_TOKEN`. Merging that PR
  is what users see.
- `scoop-bump.yml`, `chocolatey-bump.yml` and `winget-bump.yml` open
  **repo-local** PRs that refresh `distribution/**` metadata in this
  repository. Merging one updates the repository's own manifest and nothing
  else; submission to `ScoopInstaller/Main`,
  `chocolatey-community/chocolatey-coreteampackages`, and
  `microsoft/winget-pkgs` stays an explicit maintainer action, because this
  repository's `GITHUB_TOKEN` cannot write to those repositories.

## Per-channel verification

Run from the publishing repo (`EffortlessMetrics/perl-lsp`) checkout.
Replace `vX.Y.Z` with the actual tag.

### Reading a stale public listing

Scoop, Chocolatey and Winget need an explicit maintainer submission that this
repository cannot perform, so a stale public listing is ambiguous on its own.
It shows only that no *accepted publication* has propagated — never whether
something was filed. Each of those three sections links here rather than
restating the procedure.

**The submission receipt is the primary evidence.** When you file upstream,
record the submission URL in the populated audit for that release. With a
receipt, the state is known and nothing below is needed.

Without one, look for an upstream submission before filing. The three channels
expose different things — Scoop and Winget show pull-request state, Chocolatey
shows package-version moderation state — so match on what the evidence *means*,
not on its label:

| What you find | Scoop / Winget | Chocolatey | Action |
|---|---|---|---|
| Submission accepted | PR merged | version approved / listed | Record it as the receipt. The public listing is still propagating; wait. Do not refile. |
| Submission in progress | PR open | version pending moderation | Wait. Do not refile. |
| Submission rejected or superseded | PR closed unmerged | version rejected | Read the reason before refiling. |
| No evidence found | no matching PR | version absent from history | **Not proof there is none.** Confirm with whoever ran the release, then file. A search can miss a differently-titled submission, and Chocolatey moderation state is not reliably public. |

Never close a channel on the absence of evidence alone, and never refile
without establishing that the last attempt is genuinely gone.

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
orchestrated cut and opens a PR against `EffortlessMetrics/homebrew-tap`, the
tap named by its own `HOMEBREW_TAP_REPOSITORY`. Check that PR was merged:

```bash
gh pr list -R EffortlessMetrics/homebrew-tap --state all --limit 5
```

If the PR is open: review and merge it. If the PR doesn't exist:
re-dispatch with `gh workflow run brew-bump.yml -f tag=vX.Y.Z`.

### 7. Scoop bucket

**Not the same pattern as Homebrew.** `scoop-bump.yml` opens a *repo-local* PR
refreshing `distribution/scoop/perl-lsp.json` in this repository; it does not
touch `ScoopInstaller/Main`. Two separate things to confirm:

```bash
# 1. Repo-local refresh PR exists and merged
gh pr list -R EffortlessMetrics/perl-lsp-swarm \
  --search "head:automation/scoop-X.Y.Z" --state all --limit 5
```

Re-dispatch if absent: `gh workflow run scoop-bump.yml -f tag=vX.Y.Z`.

```bash
# 2. Public bucket actually carries the version
#    The package name is the manifest filename: perl-lsp, not the perllsp binary.
scoop info perl-lsp
```

If the repo-local PR merged but `scoop info` is stale, the channel is
**unresolved, not necessarily unsubmitted**. Upstream submission to
`ScoopInstaller/Main` is an explicit maintainer action — this repository's
`GITHUB_TOKEN` cannot write there — so check for one before filing:

```bash
gh pr list -R ScoopInstaller/Main --search "perl-lsp X.Y.Z" --state all --limit 5
```

Match the result against
[Reading a stale public listing](#reading-a-stale-public-listing).

### 8. Chocolatey

```bash
# The package id is perl-lsp (nuspec <id>), not the perllsp binary name.
choco search perl-lsp --exact
```

`chocolatey-bump.yml` refreshes `distribution/chocolatey/**` in this repository
through a repo-local PR. **It does not submit to
`chocolatey-community/chocolatey-coreteampackages`** — this repository's
`GITHUB_TOKEN` cannot write there, so submission is an explicit maintainer
action.

That makes three states, not two:

```bash
# Did the bump workflow run at all for this version?
gh run list --workflow=chocolatey-bump.yml --limit 10 \
  | grep -E "vX\.Y\.Z|completed"
```

1. **Workflow never ran** — dispatch it:

   ```bash
   gh workflow run chocolatey-bump.yml -f tag=vX.Y.Z
   ```

2. **Workflow succeeded, repo-local PR not merged** — merge it. The package
   metadata in this repository is still on the prior version.

3. **Workflow succeeded, PR merged, `choco search` still stale** — unresolved.
   Check the package's version history at
   `https://community.chocolatey.org/packages/perl-lsp`, then match against
   [Reading a stale public listing](#reading-a-stale-public-listing).

   Chocolatey deserves particular care here: moderation state is not reliably
   public, so a version missing from that history is **not** proof nothing was
   submitted. Use the recorded receipt, or ask whoever ran the release, before
   filing again — a duplicate submission in a moderation queue is worse than a
   delayed one.

A green workflow is *not* evidence of a queued submission, and a stale
`choco search` is *not* evidence that none exists. Moderation can queue for
hours or days, but only once something has actually been filed.

### 9. Winget

`winget-bump.yml` refreshes `distribution/winget/perl-lsp.yaml` through a
repo-local PR and does not submit to `microsoft/winget-pkgs`.

```bash
# 1. Repo-local refresh PR exists and merged
gh pr list -R EffortlessMetrics/perl-lsp-swarm \
  --search "head:automation/winget-X.Y.Z" --state all --limit 5

# 2. Public manifest carries the version
#    Winget keys on PackageIdentifier, not the perllsp binary name.
winget show --id EffortlessMetrics.perl-lsp --exact
```

Re-dispatch if absent: `gh workflow run winget-bump.yml -f tag=vX.Y.Z`.

As with Scoop and Chocolatey, a merged repo-local PR plus a stale `winget show`
leaves the channel unresolved rather than proven unsubmitted. Check for an
existing submission before filing:

```bash
gh pr list -R microsoft/winget-pkgs \
  --search "EffortlessMetrics.perl-lsp X.Y.Z" --state all --limit 5
```

Match the result against
[Reading a stale public listing](#reading-a-stale-public-listing). A merged
`winget-pkgs` PR with a stale `winget show` is propagation delay, not a missing
submission.

### 10. End-to-end smoke

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
- A bump workflow finishing green is not a publish. Homebrew's PR lands on the
  owned tap and must merge; the Scoop, Chocolatey and Winget PRs land **in this
  repository** and merging one changes nothing a user installs.
- For Scoop, Chocolatey and Winget, closure requires evidence from the public
  channel itself — `scoop info perl-lsp`, `choco search perl-lsp --exact`,
  `winget show --id EffortlessMetrics.perl-lsp --exact` — or a recorded
  upstream submission receipt. A merged repo-local metadata PR does not satisfy
  either. Note the package ids differ from the `perllsp` binary name.
- A stale public listing means **unresolved**, not unsubmitted, and an absent
  search result is not proof either. Follow
  [Reading a stale public listing](#reading-a-stale-public-listing): record a
  submission receipt when you file, and never refile without establishing that
  the last attempt is genuinely gone.

## Related

- 0.14.0 release notes (template): [`v0.14.0.md`](v0.14.0.md)
- Manual new-crate publish: [`../reference/MANUAL_PUBLISH_NEW_CRATES.md`](../reference/MANUAL_PUBLISH_NEW_CRATES.md)
- Release process: [`../RELEASE_PROCESS.md`](../RELEASE_PROCESS.md)
- Release checklist: [`../project/RELEASE_CHECKLIST.md`](../project/RELEASE_CHECKLIST.md)
