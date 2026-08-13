# LSP-perllsp

First-party Sublime Text helper-package candidate for `perllsp`, with an
optional direct `perl-dap` registration for Sublime Debugger.

## Current evidence stage

This directory is the **exact-source package candidate** owned by
`perl-lsp-swarm` issues #7686 and #7711. It can be loaded into Sublime for
development and real-host testing, but its presence here is not a Package
Control publication receipt. The reviewed package is intended to be exported to
a dedicated `EffortlessMetrics/LSP-perllsp` repository before public submission.

## What it owns

- activation on Sublime's built-in `source.perl` syntax;
- exact `perllsp --stdio` launch;
- a pinned compatibility manifest rather than an untested `latest` download;
- verified official release archives stored under Sublime Package Storage;
- an explicit user-owned external-binary escape hatch;
- Perl/SQL/JSON semantic-token mappings;
- `perldoc://` syntax mapping for LSP 2.13 dynamic text content;
- a curated `workspace/executeCommand` palette;
- optional Sublime Debugger adapter registration using direct
  `perl-dap --stdio`.

The package deliberately reconstructs LSP executable, stdio transport and
process-environment authority from package/user settings during
`on_pre_start_async`. A committed `.sublime-project` may tune ordinary server
configuration but cannot replace the launched executable, switch to TCP, or
inject process environment.

The DAP path follows the same authority boundary. A project owns the program,
working directory, arguments, include paths and debuggee environment. It cannot
replace the `perl-dap` adapter executable.

## Exact-source installation

Copy or link this directory as `LSP-perllsp` under Sublime's Packages directory,
then install the `LSP` package. Open a Perl file using the built-in Perl syntax.
Do not add a duplicate custom server entry.

For an explicit local LSP build, open **Preferences: LSP-perllsp Settings** and
set:

```json
{
  "server_path": "/absolute/path/to/perllsp"
}
```

## Optional native debugging

Install Sublime's `Debugger` package. `LSP-perllsp` registers adapter type
`perl` directly with Debugger and returns Debugger's native `StdioTransport`;
there is no protocol proxy.

The current managed `perllsp` release manifest does **not** contain a reviewed
`perl-dap` artifact. Put a matching `perl-dap` on `PATH`, place it beside an
explicit `server_path`, or set the user-owned path:

```json
{
  "dap_path": "/absolute/path/to/perl-dap"
}
```

Use **Debugger: Add Configuration** and select **Perl: Debug current file**, or
copy `Perl.sublime-project.example`. Project data may configure `program`,
`cwd`, `perlPath`, `args`, `includePaths`, `env`, and `stopOnEntry`; it does not
own `dap_path`.

## Display settings

Sublime LSP disables semantic highlighting and inlay-hint display by default.
Enable the surfaces you want in **Preferences: LSP Settings**:

```json
{
  "semantic_highlighting": true,
  "show_inlay_hints": true
}
```

`LSP-file-watcher-rust` is recommended when external filesystem changes should
refresh the workspace without reopening Sublime. It is optional and must remain
a separate support cell.

Current Sublime LSP 2.13 does not expose LSP 3.18 inline-completion UI, so the
package does not claim `perllsp` inline completion even though the server can
serve clients that advertise it.

## Portable project configuration

Use `.perl-lsp.toml` for project settings shared across editors. Do not place
managed download sources, executable paths, credentials, remote AI activation,
or external machine include roots in project-owned Sublime configuration.
