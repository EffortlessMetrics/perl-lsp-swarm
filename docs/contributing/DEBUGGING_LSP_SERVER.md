# Debugging the LSP Server

This guide covers the `perllsp` / `perl-lsp-rs` binary's built-in debugging
modes. For debugging Perl programs **via DAP**, see
[docs/how-to/DEBUGGING.md](../how-to/DEBUGGING.md) instead.

## Quick-reference

| Goal | Command |
|------|---------|
| Parse a single file, print diagnostics | `perllsp --check path/to/file.pl` |
| Run in TCP mode for a manual client | `perllsp --socket --port 9257` |
| Enable structured logging to stderr | `perllsp --stdio --log` |
| Filter log output (per-crate level) | `RUST_LOG=perl_lsp_rs=debug perllsp --stdio --log` |
| Write logs to a rotating file | `PERL_LSP_LOG_FILE=/tmp/perl-lsp.log perllsp --stdio --log` |
| Check server health and feature state | `perllsp --health` |
| Print full version and build info | `perllsp --info` |

---

## `--check`: parse diagnostics without an editor

`--check` parses one or more Perl files and prints any diagnostics to stdout,
then exits. This is the fastest way to reproduce a parser bug or verify that a
particular file produces the expected error output without needing an LSP
client:

```bash
perllsp --check lib/MyModule.pm t/basic.t
```

Output is always plain text (`path: ok` or `path: FAIL - …` with optional
context lines). Exit code 0 means no errors. (`--json` currently affects
`--doctor` only, not `--check`.)

---

## `--socket --port`: TCP mode for manual clients

By default the server expects a piped stdio connection from the editor.
`--socket` switches to TCP transport so you can connect manually — useful when
you want to drive the protocol from a test script, `netcat`, or a custom
client:

```bash
# Start the server (default port 9257)
perllsp --socket

# Specify a different port
perllsp --socket --port 8100
```

With a TCP connection, the LSP JSON-RPC protocol runs over the socket instead
of stdin/stdout. The server keeps listening and spawns a handler per accepted
connection; it does not exit when a client disconnects.

---

## `--log`: structured tracing output

Add `--log` to any run mode to enable tracing output on **stderr**. LSP
protocol traffic continues on stdout/stdin (or the socket), so stderr is
always safe to redirect:

```bash
# Stdio mode with logging
perllsp --stdio --log

# Socket mode with logging
perllsp --socket --port 9257 --log
```

### Filter verbosity with `RUST_LOG` / `PERL_LSP_LOG`

Both environment variables can enable logging. `PERL_LSP_LOG` is preferred over
`RUST_LOG` when the runtime selects a default filter, but if `RUST_LOG` is
already set in the process environment it still wins at subscriber init
(`EnvFilter::try_from_default_env()`). Use one variable, or unset `RUST_LOG`
before relying on `PERL_LSP_LOG`. The filter syntax is the standard `tracing`
subscriber format — crate-level or module-level:

```bash
# Everything at info level
RUST_LOG=info perllsp --stdio --log

# Debug logs from the LSP server crate only
RUST_LOG=perl_lsp_rs=debug perllsp --stdio --log

# Debug for providers, trace for the parser
RUST_LOG=perl_lsp_rs_core::providers=debug,perl_parser=trace perllsp --stdio --log
```

Setting either env var activates logging even without the `--log` flag.

### Write logs to a file

Set `PERL_LSP_LOG_FILE` to a file path. The server writes daily-rotated log
files (max 5 files) at that path **in addition to** stderr output:

```bash
PERL_LSP_LOG_FILE=/tmp/perl-lsp.log perllsp --stdio --log
```

The file prefix is derived from the basename of the path you provide.

---

## Checking server health and build info

```bash
# Quick health check (outputs JSON-compatible summary)
perllsp --health

# Full build info: version, git tag, feature profile, binary path
perllsp --info
```

`--health` is especially useful in CI or editor plugin diagnostics to confirm
the correct binary version is in `$PATH`.

---

## Using a custom binary with VS Code

The VS Code extension always launches `perllsp` over **stdio**; it does not
connect to a manually started socket server. To point the editor at a locally
built binary, set `perl-lsp.serverPath` in workspace or user `settings.json`:

```json
{
  "perl-lsp.serverPath": "/absolute/path/to/target/debug/perllsp"
}
```

Then run **Perl: Restart Language Server** from the command palette.

Socket mode (`--socket --port`) is for manual clients — test harnesses,
`netcat`, or custom LSP drivers — not for the VS Code language client.

---

## Related reading

- `perllsp --help` — full CLI reference
- [CONTRIBUTING.md](../../CONTRIBUTING.md) — dev environment setup and `just` commands
- [docs/project/CI.md](../project/CI.md) — how CI runs LSP integration tests
- [crates/perl-lsp-rs/src/lib.rs](../../crates/perl-lsp-rs/src/lib.rs) — rustdoc overview of transport modes and logging
