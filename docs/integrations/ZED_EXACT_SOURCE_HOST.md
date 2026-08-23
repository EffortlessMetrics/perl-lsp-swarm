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
binary_route = binary_override | worktree_path
```

`binary_override` writes the exact binary into `lsp.perllsp.binary.path`.
`worktree_path` omits that override so the staged extension resolves the
server itself through `worktree.which("perllsp")`; preparation and launch both
reject the route unless the session PATH resolves `perllsp` to the exact
prepared binary.

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
  --wasm /path/to/zed-perl-checkout/extension.wasm \
  --perllsp /path/to/perllsp \
  --perllsp-version <exact-version> \
  --perllsp-build <exact-source-commit> \
  --resolution-route binary_override \
  --workspace /path/to/fixture \
  --fixture-id zed-core-v1
```

Preparation rejects prior run state, dirty or wrong extension subjects, version mismatches, symlinked fixture/package files, a WASM artifact outside the exact extension checkout, or a `perllsp` binary whose embedded Git revision does not match `--perllsp-build`. It binds file and tree SHA-256 identities and writes an isolated profile with:

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

With `--resolution-route worktree_path` the `binary` override is omitted and
Zed resolves `perllsp` through the worktree shell environment; the driver
accepts the route only when this session's PATH resolves `perllsp` to the
exact prepared binary. `binary.arguments` stays empty so the extension remains
the exact `--stdio` authority. Preparation writes `manifest.json`, hashes its exact bytes, and injects that digest into the observation template. Do not copy an observation file from another prepared run.

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
4. retain the exact Zed language-server log;
5. close the Zed window normally.

The launcher captures foreground logs, samples the exact `perllsp` executable, records a redacted process inventory, applies a bounded timeout, and fails when Zed exits unsuccessfully, exact `perllsp` is never observed, or a new process survives shutdown. The launch result and process inventory both bind themselves to the prepared manifest digest.

## Finalize

Set `observations.result` to `pass`, `fail`, or `instrument_failed` and record only direct observations. Keep `prepared_manifest_sha256` unchanged. Populate the language-server log binding with the exact source path and its digest:

```json
{
  "language_server_log": {
    "path": "/absolute/path/to/the/exact/zed-language-server.log",
    "sha256": "sha256:<64-hex-digest>",
    "prepared_manifest_sha256": "sha256:<digest-injected-by-prepare>"
  }
}
```

Then run:

```bash
python3 scripts/zed_exact_source_finalize.py \
  --run-dir target/zed-host/run-1 \
  --output target/zed-host/run-1/receipt.json

cargo run -p xtask --bin validate-zed-host-receipt -- \
  target/zed-host/run-1/receipt.json
```

Finalization re-hashes the Zed executables, extension manifest, extension tree, bound WASM, `perllsp`, settings, and workspace fixture. It verifies that observations, launch evidence, process inventory, and the language-server log all belong to the current prepared manifest, then verifies every copied artifact reference against its bytes before redaction. A pass is published only after the complete `zed_host_compat.v1` schema and the shared semantic `validate_pass` authority both accept it.

A passing row must directly prove the required host cells, including manifest discovery, exact Perl attachment, initialize, root identity, workspace configuration, diagnostics, navigation, references, post-edit freshness, restart, shutdown, POD separation, and bounded redacted artifacts.

## Limits

One receipt proves only its exact Zed build, platform, extension tree, binary, fixture, settings, route, file families, and observed methods. It cannot close #7912, promote #7122, or establish managed/public support by inference.
