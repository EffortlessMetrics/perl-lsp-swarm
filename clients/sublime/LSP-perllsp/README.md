# LSP-perllsp

First-party Sublime Text helper-package candidate for `perllsp`.

## Current evidence stage

This directory is the **exact-source package candidate** owned by
`perl-lsp-swarm` issue #7686. It can be loaded into Sublime for development and
real-host testing, but its presence here is not a Package Control publication
receipt. The reviewed package is intended to be exported to a dedicated
`EffortlessMetrics/LSP-perllsp` repository before public submission.

## What it owns

- activation on Sublime's built-in `source.perl` syntax;
- exact `perllsp --stdio` launch;
- a pinned compatibility manifest rather than an untested `latest` download;
- verified official release archives stored under Sublime Package Storage;
- an explicit user-owned external-binary escape hatch;
- Perl/SQL/JSON semantic-token mappings;
- `perldoc://` syntax mapping for LSP 2.13 dynamic text content.

The package deliberately reconstructs executable, stdio transport and process
environment authority from package/user settings during `on_pre_start_async`.
A committed `.sublime-project` may tune ordinary server configuration but cannot
replace the launched executable, switch to TCP, or inject process environment.

## Exact-source installation

Copy or link this directory as `LSP-perllsp` under Sublime's Packages directory,
then install the `LSP` package. Open a Perl file using the built-in Perl syntax.
Do not add a duplicate custom server entry.

For an explicit local build, open **Preferences: LSP-perllsp Settings** and set:

```json
{
  "server_path": "/absolute/path/to/perllsp"
}
```

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
