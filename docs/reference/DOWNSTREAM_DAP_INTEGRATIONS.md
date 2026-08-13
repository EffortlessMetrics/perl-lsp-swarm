# Downstream DAP integrations

This is the in-repo source of truth for the debug-adapter (DAP) contract that
downstream consumers depend on. It exists so a change to our release packaging
cannot silently break an editor integration we don't control.

The machine-readable companion is
[`downstream-dap-integrations.json`](downstream-dap-integrations.json); it is
validated against produced release archives by:

```bash
cargo xtask release artifact-check --dist <dir>
```

See [CI_GATE_PLAYBOOK.md](CI_GATE_PLAYBOOK.md) and `RELEASE.md` for where this
runs in the release flow.

## VS Code / Open VSX

Source: `vscode-extension/package.json`

- Debug type: `perl`
- Adapter binary: managed `perl-dap` (downloaded with the LSP server; no separate install)
- Launch mode: stdio
- User-facing install: `EffortlessMetrics.perl-lsp-rs`
- Launch config accepts both `perl` and `perlPath` for the interpreter
  (`resolve_launch_interpreter`, `crates/perl-dap/src/debug_adapter/process.rs`)

## LSP4IJ (JetBrains)

Source: downstream template in `redhat-developer/lsp4ij`

This section describes the **release-artifact contract** that LSP4IJ material
can consume. It does not prove that LSP4IJ selected the managed installer path
or that `perl-dap` works through an actual IntelliJ debugger session.

- #7876 owns the exact managed-installer/platform selection contract and the
  external/PATH-versus-managed evidence split.
- #7877 owns actual debugger initialize/launch/breakpoint/stack/scopes/variables/
  step/cleanup behavior.
- [`INTELLIJ_DAP_SETUP.md`](../EDITORS/INTELLIJ_DAP_SETUP.md) documents the
  user-facing evidence boundary.

The installer expects each `EffortlessMetrics/perl-lsp` release archive to
contain the DAP binary alongside the LSP server.

Expected release artifacts (`perllsp-<version>-<triple><ext>`):

| Platform | Archive |
|---|---|
| Windows x64 | `perllsp-<version>-x86_64-pc-windows-msvc.zip` |
| Windows arm64 | `perllsp-<version>-aarch64-pc-windows-msvc.zip` |
| Linux x64 (glibc) | `perllsp-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 (glibc) | `perllsp-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| Linux x64 (musl) | `perllsp-<version>-x86_64-unknown-linux-musl.tar.gz` |
| Linux arm64 (musl) | `perllsp-<version>-aarch64-unknown-linux-musl.tar.gz` |
| macOS x64 | `perllsp-<version>-x86_64-apple-darwin.tar.gz` |
| macOS arm64 | `perllsp-<version>-aarch64-apple-darwin.tar.gz` |

Expected extracted binaries (each archive unpacks to a
`perllsp-<version>-<triple>/` directory):

| Platform | LSP binary | DAP binary |
|---|---|---|
| Windows | `perllsp.exe` | `perl-dap.exe` |
| Unix / macOS | `perllsp` | `perl-dap` |

On Unix and macOS the binaries must carry the executable bit. Every archive is
listed in the consolidated top-level `SHA256SUMS` published with the release.
That release manifest existing does **not** prove that the current LSP4IJ
generic installer consumes or independently verifies it before execution; the
managed-installer receipt must record that disposition explicitly.

## Maintaining this contract

When release packaging changes (target triples, archive layout, binary names),
update **both** this file and `downstream-dap-integrations.json`, then run the
artifact check against a freshly built `dist/` to confirm they still agree.

Keep artifact-contract maintenance separate from host support promotion: a
correct archive layout cannot satisfy the #7877 IntelliJ/LSP4IJ debugger
journey by itself.
