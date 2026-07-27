# Debugging the LSP Server

This guide covers the `perllsp` / `perl-lsp-rs` binary's built-in debugging
modes. For debugging Perl programs **via DAP**, see
[docs/reference/DEBUGGING.md](../reference/DEBUGGING.md) instead.

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

The output format is JSON when the server was built with the `--json` flag;
otherwise plain text. Exit code 0 means no errors.

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
of stdin/stdout. The server accepts one client at a time and exits when the
connection closes.

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

Both environment variables set the tracing filter; `PERL_LSP_LOG` takes
precedence over `RUST_LOG`. The filter syntax is the standard `tracing`
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

## Connecting from VS Code for manual testing

When the server runs in socket mode you can point VS Code at it directly with
a `launch.json` configuration, which lets you restart the server independently
of the editor:

```json
{
  "perl.lsp.command": "perllsp",
  "perl.lsp.args": ["--socket", "--port", "9257"]
}
```

Restart VS Code's language client after starting the server manually.

---

## Related reading

- `perllsp --help` — full CLI reference
- [CONTRIBUTING.md](../../CONTRIBUTING.md) — dev environment setup and `just` commands
- [docs/reference/CI.md](../project/CI.md) — how CI runs LSP integration tests
- [crates/perl-lsp-rs/src/lib.rs](../../crates/perl-lsp-rs/src/lib.rs) — rustdoc overview of transport modes and logging
