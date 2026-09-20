# Zed public `perl-dap` asset receipts

> **State:** executable receipt matrix implemented; one current matching-host
> receipt (windows-x86_64, release `v0.17.0`) is committed. Linux and macOS
> rows stay `managed_extracted_not_executed` until run on matching hosts.
>
> **Owner:** [#9516](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9516)
>
> **Adapter authority:** [#9485](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9485) (landed via #12271)

The Zed adapter authority registers the exact `perl-dap` debug adapter on the
extension candidate. This lane proves the public artifact and process
boundary it manages — independently of Zed:

```text
release metadata
  < downloaded bytes, independent SHA-256, and the release SHA256SUMS asset
  < safe archive inspection and exact perl-dap member extraction
  < exact-host perl-dap --version (exact canonical `perl-dap <version>` line) and the
  DAP initialize/disconnect exchange with its partial-order transcript proof
  (initialize before disconnect, initialized before terminated, terminated last)
  < offline managed-cache known-good preservation suite (18 scenarios)
  < real Zed debug session (#9486) and official-registry journey (#9487)
```

The `perllsp` receipts lane ([#7980](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7980))
remains the separate LSP authority. The two products share one release
archive family (`perllsp-{version}-{triple}` archives shipping both
binaries) and never share identity, member, cache, or proof rows.

## Checked projection

`.ci/fixtures/zed-perl-upstream/perl-dap-managed-downloads.v1.json` binds the
current stable release (`v0.17.0`), every shared asset id/name/size/digest,
the exact nested `perl-dap` member digest per target, the canonical release
topology, and the #9485 adapter projection manifest — both by digest, so any
drift on either surface invalidates the receipts offline.

> **Recorded divergence:** the landed #9485 authority projects a root-level
> `perl-dap.exe` Windows member, but every captured public archive — the
> Windows zip included — carries both binaries inside the
> `perllsp-{version}-{triple}` directory. The contract binds the member the
> public bytes actually contain and records the divergence; the fixture-side
> expectation is tracked against #9485.

## Static checks

Offline, suitable for pull requests (on Windows use `python`):

```bash
python3 scripts/zed_dap_asset_receipts.py validate-dap-contract \
  --contract .ci/fixtures/zed-perl-upstream/perl-dap-managed-downloads.v1.json \
  --bind-repo-root

python3 scripts/zed_dap_asset_receipts.py validate-dap-receipt \
  --receipt .ci/fixtures/zed-perl-upstream/receipts/dap-asset-template.json \
  --contract .ci/fixtures/zed-perl-upstream/perl-dap-managed-downloads.v1.json

python3 scripts/zed_dap_asset_receipts.py validate-dap-receipt \
  --receipt .ci/fixtures/zed-perl-upstream/receipts/dap-asset-windows-x86_64.v1.json \
  --contract .ci/fixtures/zed-perl-upstream/perl-dap-managed-downloads.v1.json

python3 scripts/zed_dap_asset_receipts.py dap-cache-recovery \
  --work-dir target/zed-dap-cache-recovery

cargo test -p xtask --test zed_dap_asset_receipt --locked
```

## Execute one host receipt

From a clean checkout with ordinary network access:

```bash
python3 scripts/zed_dap_asset_receipts.py execute-dap \
  --contract .ci/fixtures/zed-perl-upstream/perl-dap-managed-downloads.v1.json \
  --output target/zed-dap-public/receipt.json \
  --work-dir target/zed-dap-public/work \
  --token-env GITHUB_TOKEN
```

`GITHUB_TOKEN` is optional and used only for read-only GitHub release and
asset requests. The producer has no release, repository, branch, issue, or
registry mutation path.

The producer fails closed on: stable-release drift from the checked
projection; draft or prerelease substitution; asset name/id/size/digest
drift on either digest authority; unsafe, duplicate, linked, foreign, or
ambiguous archive members; a missing or digest-mismatched `perl-dap` member;
a version output that does not identify `perl-dap` with the expected
release version; DAP lifecycle, stdout-purity, or cleanup violations; or a
failing cache-recovery scenario. Failures write `fail`,
`instrument_failed`, or `contract_stale` receipts instead of empty passes.

## Target dispositions

Each row keeps exactly one disposition:

```text
managed_executed                # verifier host matches; full DAP smoke passed
managed_extracted_not_executed  # bytes/archive proven; this verifier cannot run it
path_only / deferred / unsupported
```

Cross-built extraction is not process execution and never promotes host or
Zed support. Windows ARM64 remains explicitly unsupported and cannot inherit
Windows x86_64 or emulation evidence.

## Evidence boundary

This lane proves public asset bytes, archive assumptions, the matching
host's bounded adapter process lifecycle, and the preservation semantics of
the isolated managed-DAP cache model this lane owns (`cache_recovery:
proven_isolated_cache_model_only`). The production Zed downloader in the
extension fixture is a different implementation surface owned by #9485 and
is deliberately not claimed here. It does not prove Zed registration, configuration, launch, a real
debug session (#9486), official-registry installation (#9487), breakpoint/
stack/variables behavior, or public support projection (#9489).

## Downstream consumption

Issue #9487 consumes the checked projection and the committed
matching-host receipts directly; no second public asset inventory may be
reconstructed.
A receipt is current only while its recorded contract sha256, release
identity, topology/projection binding digests, and verifier remain exact.
