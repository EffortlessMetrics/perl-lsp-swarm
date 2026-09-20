# Zed public `perllsp` asset receipts

> **State:** executable receipt producer implemented; no accepted receipt exists until the host matrix runs.
>
> **Owner:** [#7980](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7980)
>
> **Programme:** [#7759](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7759)

The checked Zed managed-download projection records which public release assets
the extension intends to use. That metadata is necessary, but it does not prove
that the bytes still exist, match their digests, contain the expected executable,
or run on a host.

This lane adds the next evidence stage:

```text
release metadata
  < downloaded bytes and independent SHA-256
  < safe archive inspection and exact-member extraction
  < exact-host perllsp --version and stdio lifecycle
  < actual Zed managed-download journey (#7994)
  < official-registry public journey (#7912)
```

No lower stage satisfies a higher one.

## Static checks

These checks are offline and suitable for pull requests:

```bash
python3 scripts/zed_public_asset_receipts.py validate-contract \
  --contract .ci/fixtures/zed-perl-upstream/managed-downloads.v1.json

python3 scripts/zed_public_asset_receipts.py validate-receipt \
  --receipt .ci/fixtures/zed-perl-upstream/receipts/managed-asset-template.json \
  --contract .ci/fixtures/zed-perl-upstream/managed-downloads.v1.json

cargo test -p xtask --test zed_managed_asset_receipt --locked
```

> On Windows hosts, invoke `python` instead of `python3` in every command in
> this document; the test harness selects `python` on Windows for the same
> reason.

The not-run template remains at:

```text
.ci/fixtures/zed-perl-upstream/receipts/managed-asset-template.json
```

It is not sample passing evidence.

## Execute one host receipt

From a clean checkout with ordinary network access (on Windows, use `python`
in place of `python3`):

```bash
python3 scripts/zed_public_asset_receipts.py execute \
  --contract .ci/fixtures/zed-perl-upstream/managed-downloads.v1.json \
  --output target/zed-public-assets/receipt.json \
  --work-dir target/zed-public-assets/work \
  --token-env GITHUB_TOKEN

python3 scripts/zed_public_asset_receipts.py validate-receipt \
  --receipt target/zed-public-assets/receipt.json \
  --contract .ci/fixtures/zed-perl-upstream/managed-downloads.v1.json
```

`GITHUB_TOKEN` is optional and is used only for read-only GitHub release and
asset requests. The producer has no release, repository, branch, issue, or
support-registry mutation path.

`validate-receipt` requires `--contract`: the receipt's recorded contract
digest is recomputed from that file and the receipt target rows are compared
with the contract target set, so a receipt cannot attest to a different or
narrower subject than the checked contract.

When `--work-dir` is omitted, the producer downloads and extracts into a
temporary work directory that is removed when the command finishes. Pass
`--work-dir` only to retain artifacts for inspection.

The complete matrix should run independently on current Linux, macOS, and
Windows hosts. Each receipt binds its verifier OS and architecture.

## Target dispositions

A managed target receives one of two successful execution-stage results:

```text
managed_executed
  The verifier OS and architecture match the target. The extracted binary
  identifies itself as perllsp and completes initialize, initialized, shutdown,
  and exit over stdio with protocol-only stdout and no new surviving process.

managed_extracted_not_executed
  The downloaded bytes and archive layout are proven, but this verifier cannot
  execute that target. This is not host process proof.
```

`path_only`, `deferred`, and `unsupported` remain explicit non-managed rows.
Windows ARM64 cannot inherit Windows x86-64 or emulation evidence.

## Failure behavior

The producer fails closed when:

- the current stable public release no longer matches the checked projection;
- a release is draft or prerelease;
- an asset name, ID, size, or GitHub digest differs;
- downloaded bytes differ from the retained SHA-256;
- the archive contains traversal, absolute paths, duplicate members, links, a
  missing required member, a noncanonically named required member, or another
  code-intelligence executable;
- the archive bytes are malformed (a corrupt zip or tar.gz becomes a per-target
  `fail` receipt, not an instrument crash);
- the version output does not identify `perllsp` or does not report the
  expected release version;
- stdio output contains stray or malformed data;
- initialize or shutdown does not complete as expected;
- the process times out, exits unsuccessfully, or leaves a new `perllsp`
  process behind;
- the launched process cannot be observed in the post-launch inventory, so
  cleanup cannot be proven.

The receipt is still written with `fail`, `instrument_failed`, or
`contract_stale` so the failed subject and exact boundary remain reviewable.
A failure is never converted into an empty pass.

## Evidence boundary

This lane proves only public asset bytes, archive assumptions, and the matching
host's bounded process lifecycle. It does not prove:

```text
Zed extension loading
Zed provider selection
Zed workspace/configuration behavior
Zed semantic method consumption
managed cache selection or recovery
official registry installation
public Zed support
```

[#7994](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7994)
owns the managed route inside real Zed. [#7912](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7912)
owns the official-registry public journey. [#8000](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/8000)
projects only exact public evidence into the support registry and generated
documentation.
