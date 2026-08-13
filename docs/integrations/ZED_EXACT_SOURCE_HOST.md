# Exact-source Zed host receipt

> **State:** the driver exists; no Zed support is proven until a real host receipt passes.
>
> **Owner:** #7984. Controller: #7907.

This lane runs the staged Perl extension and exact current-source `perllsp` in a real Zed desktop host. It uses Zed's reviewed surfaces:

```text
zed --user-data-dir <isolated-profile> --foreground --wait <workspace>
zed::InstallDevExtension
```

The driver does **not** write Zed's extension index, installed-extension directory, database, or cache directly. Development-extension installation remains the documented in-product action.

## Evidence boundary

```text
evidence_stage = exact_source_dev_extension
install_route = dev_extension
binary_route = explicit_binary_path | worktree_path
```

This is not managed download, official-registry installation, or public support. #7994 owns the managed route in real Zed; #7912 owns the official-registry journey.

## Prepare

```bash
python3 scripts/zed_exact_source_prepare.py \
  --run-dir target/zed-host/run-1 \
  --zed-cli /path/to/zed-cli \
  --zed-app /path/to/zed-app-binary \
  --zed-version <exact-version> \
  --zed-channel stable \
  --zed-build <exact-build> \
  --extension-dir /path/to/zed-perl-checkout \
  --extension-base <reviewed-upstream-base> \
  --extension-candidate <exact-candidate-commit> \
  --extension-version <manifest-version> \
  --wasm /path/to/extension.wasm \
  --perllsp /path/to/perllsp \
  --perllsp-version <exact-version> \
  --perllsp-build <exact-source-commit> \
  --resolution-route explicit_binary_path \
  --workspace /path/to/fixture \
  --fixture-id zed-core-v1
```

Preparation rejects prior run state, dirty or wrong extension subjects, version mismatches, and symlinked fixture/package files. It binds file and tree SHA-256 identities and writes an isolated profile with:

```jsonc
{
  "languages": {
    "Perl": {
      "language_servers": [
        "perllsp",
        "!perlnavigator-server",
        "!perl-lsp",
        "..."
      ]
    }
  },
  "lsp": {
    "perllsp": {
      "binary": {
        "path": "/exact/perllsp",
        "arguments": []
      },
      "settings": {
        "perl": {}
      }
    }
  }
}
```

`binary.arguments` stays empty so the extension remains the exact `--stdio` authority.

## Launch

```bash
python3 scripts/zed_exact_source_launch.py \
  --run-dir target/zed-host/run-1 \
  --timeout-seconds 3600
```

Inside the isolated Zed session:

1. invoke `zed::InstallDevExtension` and select the exact prepared checkout;
2. verify the extension appears as a development override;
3. exercise the activation and semantic checklist in `observations.json`;
4. retain the Zed language-server log;
5. close the Zed window normally.

The launcher captures foreground logs, samples the exact `perllsp` executable, records a redacted process inventory, applies a bounded timeout, and fails when Zed exits unsuccessfully, exact `perllsp` is never observed, or a new process survives shutdown.

## Finalize

Set `observations.result` to `pass`, `fail`, or `instrument_failed`; record only direct observations; and provide the absolute local language-server log path. Then run:

```bash
python3 scripts/zed_exact_source_finalize.py \
  --run-dir target/zed-host/run-1 \
  --output target/zed-host/run-1/receipt.json

cargo run -p xtask --bin validate-zed-host-receipt -- \
  target/zed-host/run-1/receipt.json
```

Finalization re-hashes immutable subjects, copies redacted content-addressed logs, fills the existing `zed_host_compat.v1` exact-source receipt, and publishes a passing receipt only after the shared Rust `validate_pass` authority accepts it.

A passing row must directly prove the required host cells, including manifest discovery, exact Perl attachment, initialize, root identity, workspace configuration, diagnostics, navigation, references, post-edit freshness, restart, shutdown, POD separation, and bounded redacted artifacts.

## Limits

One receipt proves only its exact Zed build, platform, extension tree, binary, fixture, settings, route, file families, and observed methods. It cannot close #7912, promote #7122, or establish managed/public support by inference.
