# Helix Setup Guide for `perllsp`

Use this guide to run `perllsp` through Helix's built-in LSP client.

Helix already has a built-in `perl` language entry, but its default Perl
language server is currently `perlnavigator`. Until an upstream `perllsp`
server definition is available, users must provide a `languages.toml`
override.

> [!IMPORTANT]
> This page documents the intended manual configuration. A working snippet is
> not, by itself, an actual-client or public-artifact support receipt. Helix
> versions, platforms, diagnostic modes, installation routes, and feature cells
> are verified separately.

## Current Helix cohorts

The current official release and current Helix master are different client
subjects:

| Cohort | Exact subject | Diagnostic transport | Workspace trust |
| --- | --- | --- | --- |
| Released stable | Helix 25.07.1 | push diagnostics | not present |
| Current source | master `079a789e8cb08ead67f19e1971a1b7438b37354b` | pull diagnostics | present |

Do not infer current-master behavior from Helix 25.07.1 or describe a master
checkout as released support. Other Helix versions may work, but the repository
does not currently claim an unbounded minimum such as “23.10 or later.”

## Prerequisites

- `perllsp` installed and available on `PATH`
- a local Perl interpreter for interpreter-backed features
- a Perl 5 project or file

Verify the installed subjects before editing Helix configuration:

```bash
hx --version
hx --health perl
perllsp --version
perllsp --health
perllsp --info
```

## Install `perllsp`

### Cargo

```bash
cargo install perllsp --locked
```

### From source

```bash
git clone https://github.com/EffortlessMetrics/perl-lsp.git
cd perl-lsp
cargo install --path crates/perllsp --locked
```

### Prebuilt binary

Download the archive for your platform from GitHub Releases, extract it, and put
`perllsp` on `PATH`.

Release assets use the `perllsp-<version>-<target>` naming pattern. Confirm the
version and executable selected by the fresh shell or Helix process; an older
`perllsp` earlier on `PATH` is a different test subject.

## Configure Helix globally

Create or update the user-level Helix language configuration:

- Linux/macOS: `~/.config/helix/languages.toml`
- Windows: `%AppData%\helix\languages.toml`

Add:

```toml
[language-server.perllsp]
command = "perllsp"
args = ["--stdio"]

[[language]]
name = "perl"
language-servers = ["perllsp"]
roots = [".perl-lsp.toml", "Makefile.PL", "Build.PL", "cpanfile", "dist.ini"]
file-types = [
  "pl",
  "pm",
  "t",
  "psgi",
  { glob = "latexmkrc" },
  { glob = ".latexmkrc" },
]
shebangs = ["perl"]
```

Restart Helix after changing `languages.toml`. The checked copy/paste fixture is
[`docs/examples/helix/languages.toml`](../examples/helix/languages.toml).

The `perllsp` key is a Helix-local server identifier. Keeping it equal to the
installed executable avoids a second name in logs and troubleshooting.

### Why the file-type list is explicit

Current Helix combines Perl 5 and Raku/NQP/P6 extensions in one built-in
`perl` language entry. `perllsp` is a Perl 5 language server. The override above
intentionally narrows that entry to the reviewed Perl 5 subset so Raku-family
files do not silently launch `perllsp`.

This safe override means the same Helix configuration no longer owns Raku/NQP/P6
file detection. Users who edit both language families need a separate reviewed
Raku configuration; this guide does not provide or imply one.

The guide also does not currently claim activation for POD, XS/XSI,
mixed-language templates, extensionless scripts, or additional file extensions.
Syntax detection and useful LSP semantics are separate evidence cells.

## Root selection

Helix starts at the opened file and walks upward for the configured `roots`. It
selects the topmost matching directory within the Helix workspace boundary.

The candidate Perl markers are:

```text
.perl-lsp.toml
Makefile.PL
Build.PL
cpanfile
dist.ini
```

For nested distributions, confirm the actual root in the LSP log. A project can
use `workspace-lsp-roots` in `.helix/config.toml` to stop Helix's upward search
early when the normal topmost-marker rule selects a parent distribution.

Prefer `.perl-lsp.toml` for portable project settings once the intended project
root is established.

## Project-local Helix configuration

A project may place the same language configuration in:

```text
.helix/languages.toml
```

The behavior is version-specific.

### Helix 25.07.1

Helix 25.07.1 predates workspace trust. Do not follow current-master trust
instructions on that release.

### Current master and later trust-aware builds

Current Helix master uses workspace trust. With the default
`[editor.workspace-trust] level = "servers"`, globally configured language
servers may start automatically, but project-local `.helix/config.toml` and
`.helix/languages.toml` require trust.

Use:

```text
:workspace-trust
:workspace-untrust
:workspace-exclude
```

Helix records a hash of `.helix/` contents. If those files change after trust is
granted, Helix treats the local configuration as stale and requires another
`:workspace-trust` action.

Global `languages.toml` remains the baseline path when you do not want a
repository-controlled editor configuration. Helix workspace trust does not
replace `perllsp`'s own project configuration and subprocess security policy.

## Initialization options

Helix sends `language-server.<name>.config` as LSP initialization options.

```toml
[language-server.perllsp.config.perl.workspace]
includePaths = ["lib", ".", "local/lib/perl5"]
useSystemInc = false
resolutionTimeout = 50

[language-server.perllsp.config.perl.inlayHints]
enabled = true
parameterHints = true
typeHints = true
maxLength = 30

[language-server.perllsp.config.perl.limits]
workspaceSymbolCap = 200
referencesCap = 500
completionCap = 100
```

The configuration layers have different owners:

```text
Helix languages.toml
  command, args, environment, language attachment, and editor-side roots

language-server.perllsp.config
  initializationOptions base layer

.perl-lsp.toml
  project-owned perllsp configuration

workspace/configuration
  later client configuration when the Helix cohort advertises it
```

Prefer `.perl-lsp.toml` for settings that should work across editors. Do not
create a Helix-only precedence rule.

For large workspaces, tune limits conservatively:

```toml
[language-server.perllsp.config.perl.limits]
workspaceSymbolCap = 100
referencesCap = 200
completionCap = 50
astCacheMaxEntries = 50
maxIndexedFiles = 5000
maxTotalSymbols = 250000
workspaceScanDeadlineMs = 20000
referenceSearchDeadlineMs = 1500
```

## Diagnostics by Helix cohort

`perllsp` selects diagnostics from the client's actual initialize capabilities.

### Helix 25.07.1

Helix 25.07.1 does not advertise `textDocument.diagnostic`. `perllsp` therefore
uses push diagnostics through `textDocument/publishDiagnostics`.

### Current master

Current Helix master advertises pull diagnostics and contains a client path that
requests `textDocument/diagnostic`. Actual support still requires an actual-host
receipt showing that the exact build polls and consumes the result; the presence
of a capability field alone is not enough.

Do not add a Helix client-name workaround or force both modes to look alike.

## Inlay hints

Enable Helix's display surface in `config.toml`:

```toml
[editor.lsp]
display-inlay-hints = true
```

Toggle it at runtime with:

```text
:toggle lsp.display-inlay-hints
```

A visible toggle proves that Helix has an inlay-hint surface; actual `perllsp`
hint support for a cohort is established only when the client requests and
renders the hints.

## Environment variables

Use Helix's `environment` table instead of wrapping the command with `env`:

```toml
[language-server.perllsp]
command = "perllsp"
args = ["--stdio"]
environment = { PERL5LIB = "lib" }
```

Use project configuration rather than a global environment override when the
path is specific to one repository.

## Verify the actual session

1. Open a reviewed Perl 5 file such as `lib/My/Module.pm`, `script/app.pl`, or
   `t/basic.t`.
2. Confirm the Helix statusline shows `perl`.
3. Run `hx --health perl` and confirm the configured server is `perllsp`.
4. Open `:log-open` and verify the launched command and selected workspace root.
5. Introduce a temporary syntax error and confirm diagnostics appear through the
   cohort's expected transport.
6. Remove the error and confirm the diagnostic changes or clears.
7. Exercise one navigation or completion request and confirm the result belongs
   to the current buffer contents.

Useful commands:

```text
:log-open
:lsp-restart
:lsp-stop
:set-language perl
```

Useful external checks:

```bash
hx --health perl
perllsp --health
perllsp --info
perllsp --check path/to/file.pl
```

## Common Helix LSP commands

| Action | Default keybinding | Command |
| --- | --- | --- |
| Go to definition | `gd` | `goto_definition` |
| Find references | `gr` | `goto_reference` |
| Hover | `<space>k` | `hover` |
| Completion | `<C-x>` in insert mode | `completion` |
| Document symbols | `<space>s` | `symbol_picker` |
| Workspace symbols | `<space>S` | `workspace_symbol_picker` |
| Rename symbol | `<space>r` | `rename_symbol` |
| Code action | `<space>a` | `code_action` |
| Diagnostics picker | `<space>d` | `diagnostics_picker` |
| Workspace diagnostics | `<space>D` | `workspace_diagnostics_picker` |
| Next diagnostic | `]d` | `goto_next_diag` |
| Previous diagnostic | `[d` | `goto_prev_diag` |
| Format file | `:format` or `:fmt` | `format` |
| Format selection | `=` | `format_selections` |

These are Helix UI routes, not proof that every `perllsp` feature cell has been
exercised on every Helix cohort. A returned edit must be applied, navigation must
land on the reviewed location, and formatting must produce the reviewed final
buffer before the corresponding integration cell is considered proven.

## Formatting

Helix uses the language server for `:format` unless an external formatter is
configured. Native LSP formatting does not require `perltidy`.

Do not configure an external formatter while validating the LSP formatting path;
it can mask a broken or unused server response.

To intentionally use an external formatter instead:

```toml
[[language]]
name = "perl"
formatter = { command = "perltidy", args = ["-q"] }
```

That is a separate integration path from `perllsp` document formatting.

## Troubleshooting

### `perllsp` does not start

Confirm the executable selected by a fresh shell:

```bash
command -v perllsp
perllsp --version
perllsp --health
perllsp --info
```

On Windows PowerShell:

```powershell
where.exe perllsp
perllsp --version
perllsp --health
perllsp --info
```

Then run:

```bash
hx --health perl
```

Open `:log-open` and confirm the command is `perllsp --stdio`, not
`perlnavigator`, the retired `perl-lsp` executable, or an older ambient binary.

If `perllsp --stdio` appears to hang when run manually, that is expected. It is
waiting for framed LSP input.

### Helix still starts `perlnavigator`

- Confirm the active `languages.toml` uses the server ID `perllsp` consistently.
- Confirm the file is detected as the `perl` language.
- On trust-aware Helix builds, confirm project-local `.helix/languages.toml` was
  trusted and is not stale.
- Prefer the global configuration while isolating trust or merge problems.
- Restart the session with `:lsp-restart` and inspect `:log-open` again.

### Raku/NQP/P6 files

This guide intentionally does not attach `perllsp` to Raku-family files. Do not
broaden the `file-types` list to include `.raku`, `.rakumod`, `.rakutest`,
`.rakudoc`, `.nqp`, `.p6`, `.pl6`, or `.pm6` and then interpret server startup
as support.

### Wrong root or missing modules

- Inspect the root in `:log-open`.
- Remember that Helix selects the topmost matching root marker within the
  workspace.
- Use `.perl-lsp.toml` at the intended project root.
- In nested projects, set `workspace-lsp-roots` in project-local
  `.helix/config.toml` where the Helix cohort supports and loads it.
- Keep include paths in `.perl-lsp.toml` or reviewed initialization options.

Example:

```toml
[language-server.perllsp.config.perl.workspace]
includePaths = ["lib", ".", "local/lib/perl5", "vendor/lib"]
useSystemInc = false
```

### No diagnostics

- Confirm the file language is `perl`.
- Confirm the active server is `perllsp`.
- For Helix 25.07.1, look for pushed diagnostics.
- For current master, confirm the exact build advertises and actually polls pull
  diagnostics rather than checking only the capability object.
- Run `perllsp --check path/to/file.pl` outside Helix to separate server/project
  behavior from client integration.

### Project-local configuration is ignored

On current trust-aware Helix builds:

- inspect the workspace-trust prompt or status indicator;
- run `:workspace-trust` for the current workspace;
- if `.helix/` changed after trust was granted, run it again;
- use global `languages.toml` to separate trust from server problems.

Do not apply this advice to Helix 25.07.1, which predates workspace trust.

### Slow performance

Reduce result and indexing limits only after confirming the correct server/root:

```toml
[language-server.perllsp.config.perl.limits]
workspaceSymbolCap = 100
referencesCap = 200
completionCap = 50
maxIndexedFiles = 5000
maxTotalSymbols = 250000
```

### Tree-sitter or highlighting problems

Syntax highlighting and LSP behavior are separate integrations.

Run:

```bash
hx --health perl
```

If you build Helix from source or maintain a custom runtime:

```bash
hx --grammar fetch
hx --grammar build
```

Refreshing the grammar does not fix an LSP process, diagnostic transport, root,
or configuration problem.

## Debugging

Helix DAP integration through `perl-dap --stdio` is tracked separately from LSP
support. Do not count an LSP `perl.debugFile` command, a VS Code debug receipt,
or a DAP unit test as Helix debugger support.

For server-side configuration and troubleshooting, see:

- [Configuration reference](../reference/CONFIG.md)
- [General troubleshooting](../how-to/TROUBLESHOOTING.md)
