# Exact-source Zed managed public-artifact route

> **State:** contract and validator implemented; no managed Zed route is proven until a real development-extension session passes.
>
> **Owner:** #7994. Controllers: #7907 and #7903. Inputs: #7980 and #7984.

This lane combines two independent subjects:

```text
#7980
  exact stable public asset bytes, archive layout, binary digest, and matching-host stdio lifecycle

#7984
  exact development extension, Zed host, platform, fixture, and semantic journey
```

It then proves that the extension itself selects the #7980 subject in real Zed. A direct process smoke cannot fill this row.

## Clean managed journey

Start with:

```text
development extension = exact staged candidate
explicit binary override = absent
worktree PATH candidate = absent
managed cache = empty
other Perl providers = disabled
selected server ID = perllsp
```

Observe:

1. the exact stable release and target from #7980;
2. exact asset ID, name, URL, size, archive digest, and expected member;
3. exact extracted `perllsp` digest and version output;
4. exact `perllsp --stdio` process identity;
5. the bounded core Zed journey from #7984;
6. restart reuse of the same verified cache subject without replacement;
7. normal disable/shutdown with no orphan.

A pre-existing PATH, workspace, or cache binary invalidates the clean journey.

## Recovery matrix

Run each scenario against an isolated cache fixture with one verified known-good subject:

```text
missing_asset
duplicate_matching_asset
wrong_target
checksum_mismatch
unsafe_archive_member
missing_expected_executable
partial_download
extraction_failure
launch_failure
```

Every scenario must retain:

```text
known_good_before == known_good_after
candidate_selected = false
fallback_server_id = null
retry_result = pass
```

The extension may not fall through to `perl-lsp` or Perl Navigator. Older managed versions remain available until the candidate has been verified, extracted, and launched successfully.

## Receipt

The contract and template are:

```text
.ci/fixtures/zed-perl-upstream/managed-route.v1.json
.ci/fixtures/zed-perl-upstream/receipts/managed-route-template.json
```

Bind one exact passing `zed_managed_asset_receipt.v1`, one exact passing `zed_host_compat.v1` at `exact_source_dev_extension`, the selected public subject, cache state, core journey receipt, and all recovery rows. Validate with:

```bash
cargo run -p xtask --bin validate-zed-managed-route -- \
  .ci/fixtures/zed-perl-upstream/managed-route.v1.json \
  /path/to/managed-route-receipt.json
```

A passing receipt requires:

- exact #7980 asset and binary identities;
- exact #7984 host/extension/platform/fixture identity;
- `resolution_route = managed_public_artifact`;
- exact `perllsp --stdio`;
- empty-cache installation and same-subject restart reuse;
- no replacement attempt on ordinary restart;
- preservation of older versions until launch success;
- passing core journey, disable, shutdown, and orphan cells;
- all nine failure scenarios preserving known-good state;
- `official_registry = not_proven` and `public_support = not_proven`.

## Limits

This remains a development-extension receipt. It proves the managed binary route before upstream submission, not official-registry distribution. #7912 must repeat the journey from the released official registry subject before public support can be considered, and #8000 separately owns support projection.
