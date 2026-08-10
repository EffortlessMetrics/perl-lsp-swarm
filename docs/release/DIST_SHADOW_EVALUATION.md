# cargo-dist shadow evaluation

`dist-workspace.toml` is a **shadow contract**, not a release authority.

The active artifact builder is `.github/workflows/release.yml`. The dist configuration exists to answer a narrower question:

> Can dist describe the same applications and target matrix without publishing anything?

## Current authority map

| Concern | Authority |
|---|---|
| Cargo and application version selection | workspace manifests and version-bump workflow |
| Release-note capture | Changie ledger and curated release notes |
| Release archive construction | `.github/workflows/release.yml` |
| Shadow plan and comparison | `dist-workspace.toml` + `scripts/check_dist_shadow.py` |
| Publication/channel closeout | release orchestration and channel receipts |

## Safety boundary

The shadow config must keep:

```toml
[dist.github-releases]
create = false
```

It declares no installers because the active release workflow currently ships archives only. Adding shell, PowerShell, Homebrew, MSI, or other installers is a product/distribution decision and requires its own test and channel-acceptance evidence.

`pr-run-mode = "skip"` prevents generated dist CI from presenting itself as a normal pull-request gate.

## Contract check

Run:

```bash
python3 scripts/check_dist_shadow.py
python3 scripts/tests/test_dist_shadow.py
```

The checker derives the live binary and target inventory from `release.yml` and requires exact equality with `dist-workspace.toml`. It also rejects stale binary/target names, enabled GitHub Release creation, or an installer claim absent from the active workflow.

## Manual plan

The `Dist Shadow` workflow accepts a release tag and executes:

```bash
dist plan --output-format=json --tag <tag>
```

The resulting `dist-plan.json` is retained as a workflow artifact. The workflow has read-only repository permission and performs no build, host, publish, announce, tag, release, or package-manager action.

## Adopt-or-delete checkpoint

Do not keep the shadow indefinitely.

After one representative release candidate, compare:

1. application names and versions;
2. target matrix;
3. archive names and internal binary names;
4. checksums;
5. metadata and release manifest usefulness;
6. build duration and failure recovery;
7. installer requirements, if any;
8. interaction with Changie and curated release notes.

Then record one decision:

- **adopt** dist as the artifact-building authority through a dedicated migration PR; or
- **delete** `dist-workspace.toml` and the shadow workflow because the evidence does not justify a second maintained plan.

Until that decision, no release procedure may cite dist output as proof that an artifact was built or published.
