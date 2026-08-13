# IntelliJ IDEA / LSP4IJ Setup

This guide covers `perllsp` through the [LSP4IJ](https://github.com/redhat-developer/lsp4ij) plugin in IntelliJ-platform IDEs.

LSP4IJ is the integration dependency. `perl-lsp` does **not** ship a repository-owned native JetBrains plugin.

> **Maintained LSP4IJ line:** 0.20.0 and newer.
>
> **Evidence boundary:** setup/configuration can be documented before the complete real-host journey is receipted. Public feature/support claims must come from the client-support registry and the exact IntelliJ/LSP4IJ receipt tracked by #7719. One IntelliJ receipt does not automatically prove every JetBrains product.

## Integration identity

The canonical language-server process is:

```text
perllsp --stdio
```

Keep these subjects separate when troubleshooting or reporting support:

```text
IDE product + exact version
LSP4IJ exact version
Perl template stage
perllsp install route + exact binary
workspace/project fixture
```

A different Perl server, a different JetBrains LSP plugin, or the JetBrains Perl plugin cannot satisfy a `perllsp + LSP4IJ` support receipt.

## Template stages

There are three distinct LSP4IJ Perl-template subjects:

1. **Released built-in template** — whatever the installed LSP4IJ release currently ships.
2. **Repository-owned corrected template** — the importable candidate maintained by `perl-lsp` for pre-upstream verification.
3. **Future corrected built-in template** — an upstream LSP4IJ release that contains the reviewed correction and is then tested again as a released client.

Do not treat these as interchangeable. A local imported template can prove the proposed correction before upstream release, but it cannot prove what users receive from an unmodified released LSP4IJ installation.

## Install LSP4IJ

1. Open **Settings > Plugins**.
2. Search the Marketplace for **LSP4IJ**.
3. Install or update LSP4IJ.
4. Restart the IDE when required.

Record the exact LSP4IJ version when reproducing behavior.

## Choose the `perllsp` binary

LSP4IJ can consume different binary subjects. Keep the route explicit.

### External or PATH-selected binary

Use an existing installation when you want to control the exact binary yourself:

```bash
perllsp --version
perllsp --health
```

If the IDE does not inherit the shell `PATH`, use the absolute path to the intended binary.

### LSP4IJ-managed public artifact

Managed installation is a separate evidence subject. A built-in template selecting a pre-existing PATH binary does **not** prove that LSP4IJ downloaded a public artifact.

Platform/architecture support, Linux libc disposition, release-asset identity, and checksum limitations are governed by the LSP4IJ installer contract in #7876. Do not infer a managed-install platform from another platform or from the release workflow alone.

### Local exact-source candidate

For development or pre-upstream interoperability testing, point LSP4IJ at the exact locally built candidate and record its source SHA/hash. This is not public-artifact evidence.

## Recommended: LSP4IJ Upstream Integration

Use the released built-in Perl entry only with the state that your installed LSP4IJ version actually ships.

The currently reviewed upstream Perl material and the repository-owned corrected candidate are intentionally tracked as different subjects. In particular, generic server configuration belongs under canonical `perl.*` settings; VS Code extension settings under `perl-lsp.*` are not a generic LSP settings authority.

The repository-owned corrected LSP4IJ template import route is **not available yet**. `docs/EDITORS/lsp4ij-perl-lsp.json` is a legacy manual descriptor, not that template. Until the importable corrected template lands under #7875, treat this verification route as unavailable and use [Legacy Raw Command Setup](INTELLIJ_IDEA_LEGACY_RAW_COMMAND.md) for local exact-source binding instead.

Use [Legacy Raw Command Setup](INTELLIJ_IDEA_LEGACY_RAW_COMMAND.md) only for development, custom launch flags, or a LSP4IJ build where the relevant template route is unavailable.

## Configuration authority

Do not collapse project configuration, live IDE overrides, initialization options, and installer settings.

### Portable project configuration

Prefer `.perl-lsp.toml` for project/repository behavior that should travel with the checkout and work across editors.

### LSP4IJ Configuration tab

The corrected generic-client projection uses server-native `perl.*` keys. LSP4IJ can expand dotted keys such as:

```text
perl.workspace.includePaths
perl.workspace.usePerl5lib
perl.inlayHints.enabled
```

into the nested server payload.

A corrected template keeps transmitted settings sparse by default. Schema defaults describe server behavior; they are not automatically explicit high-precedence IDE overrides.

Do **not** copy VS Code-only keys such as these into generic LSP4IJ server configuration:

```text
perl-lsp.serverPath
perl-lsp.autoDownload
perl-lsp.channel
perl-lsp.linuxLibc
perl-lsp.autoUpdate
perl-lsp.trace.server
```

### Initialization options

Use `initializationOptions` only for values that genuinely need initialize/reinitialize timing. Do not use initialization options as the default substitute for ordinary live `perl.*` settings.

The server-native shape remains rooted at `perl` when an initialization-time value is required:

```json
{
  "perl": {
    "workspace": {
      "includePaths": ["lib", "vendor/lib"]
    }
  }
}
```

Whether a field belongs here, in live client settings, or only in project/user authority comes from the canonical generic settings/configuration contract, not this editor guide.

### Include-path discriminator

When validating configuration, use a module that resolves **only** when the additional include path is active. Seeing JSON in a log is not proof that the setting changed server behavior.

## File activation

The initial LSP4IJ support/activation boundary is deliberately narrow:

```text
*.pl
*.pm
*.t
```

Each family must be opened through the actual host and exercise at least one meaningful semantic request before it can be marked host-proven.

Treat `.PL`, `.cgi`, `.fcgi`, `.psgi`, POD, XS, templates, special filenames, and extensionless/shebang scripts as independent cells. Parser capability or a broad upstream file-pattern list does not promote them automatically.

## Verify the active subject

For any interoperability run:

1. Record the IDE product/version and LSP4IJ version.
2. Record whether the template is released built-in, locally imported corrected, or future corrected built-in.
3. Record whether the binary is external/PATH, LSP4IJ-managed public artifact, or local exact source.
4. Open a `.pl`, `.pm`, or `.t` file.
5. Confirm the LSP4IJ console shows the intended `perllsp --stdio` process.
6. Confirm the binary identity/version is the intended candidate.
7. Exercise only the feature cells required by the receipt or troubleshooting task.
8. Shut the language server down and confirm no orphaned `perllsp` process remains.

## Feature evidence

Server capability advertisement is not the same thing as IntelliJ/LSP4IJ host support.

The support registry keeps separate cells for at least:

```text
initialize / initialized
open / change / save / close
diagnostics
completion + resolve
hover / definition / references
document + workspace symbols
prepareRename + rename
code actions
document + range formatting
format-on-save mechanism
semantic tokens
folding ranges
inlay hints
inline completion
workspace/configuration
workspace folders / root mapping
watched files
position encoding / Unicode behavior
shutdown / process cleanup
```

A cell may be `proven`, `limited`, `client_not_exposed`, or `not_proven`. Do not infer one from the corresponding `perllsp` capability bit.

### Inline completion

LSP4IJ-shaped protocol profiles can negotiate standard `textDocument/inlineCompletion`, including dynamic registration through `client/registerCapability`. That is protocol-profile evidence.

User-facing “inline completion works in IntelliJ/LSP4IJ” requires the actual host to consume the feature in #7719.

### Format on save

Document/range formatting and format-on-save are separate cells. Server support for `willSave`/`willSaveWaitUntil` does not prove that current LSP4IJ exposes a working format-on-save path.

## Project and workspace topology

Do not assume IntelliJ projects, modules, content roots, and LSP workspace folders are equivalent.

The real-host receipt owns the tested result for:

- ordinary single-root projects;
- `lib/` + `t/` layouts;
- configured include paths;
- external file create/change/delete;
- multiple modules/content roots where exercised.

`.perl-lsp.toml` remains the portable per-root project authority. Do not treat `scopeUri` alone as proof of independent per-folder IDE settings.

## Watched files

Current server history includes a broad JetBrains-family dynamic watched-file suppression workaround. It is being retired or narrowed through #7710 using the exact supported LSP4IJ capability profile.

Treat that workaround as compatibility debt with an explicit retirement test, **not** as a permanent rule that JetBrains clients cannot use dynamic registration. Current documentation should follow the result of #7710/#7719 rather than preserving the historical assumption indefinitely.

## Coexistence with other Perl integrations

The JetBrains Perl plugin (Camelcade) and other Perl language-server integrations are separate products.

When both are installed, verify:

- which integration owns the active file type;
- which language-server process actually launched;
- whether diagnostics/actions/formatters are duplicated;
- debugger ownership separately from LSP ownership.

If coexistence is limited, document the exact winning integration and the specific integration to disable or select. Do not infer comparative quality from the conflict.

## Troubleshooting

### Server does not start

Confirm the exact subject first:

```bash
perllsp --version
perllsp --health
```

Then inspect the LSP4IJ client/console log and confirm the command is the intended `perllsp --stdio` binary. Keep LSP4IJ/client logs separate from server stderr when capturing evidence.

### Wrong server is active

A Perl file can be claimed by another plugin or language server. Check the LSP4IJ console/process identity before debugging semantic behavior.

### Module resolution differs from expectation

Verify the workspace root and use a behavior-bearing include-path discriminator. Prefer `.perl-lsp.toml` for shared project configuration; use a sparse `perl.*` IDE override only when you intentionally want a higher-precedence editor/user value.

### Feature is missing

Distinguish:

```text
server does not advertise/implement it
client does not expose it
feature is exposed but the real-host cell fails
feature has not been exercised yet
```

Do not turn `not_proven` into a server bug by inference.

### External file changes stay stale

Record the LSP4IJ version and watched-file capability/registration path, then compare the result with #7710/#7719. Do not reintroduce an unbounded JetBrains-name workaround.

### Repeated restarts behave strangely

Confirm the old `perllsp` process exited before evaluating the new run. Orphan cleanup is part of the real-host receipt.

## Debugging / DAP

The presence of a Perl DAP template in LSP4IJ is a distribution/configuration fact, not proof that `perl-dap` works through the IntelliJ debugger integration.

LSP and DAP evidence are independent. See [IntelliJ / LSP4IJ `perl-dap` Setup and Evidence Boundary](INTELLIJ_DAP_SETUP.md) for the debugger subject model, launch journey, attach boundary, feature cells, and troubleshooting. Actual debugger support still requires #7877/#7122 evidence.

## See also

- [Legacy Raw Command Setup](INTELLIJ_IDEA_LEGACY_RAW_COMMAND.md)
- [IntelliJ / LSP4IJ `perl-dap` Setup and Evidence Boundary](INTELLIJ_DAP_SETUP.md)
- [Editor Setup](../how-to/EDITOR_SETUP.md)
- [Installation](../how-to/INSTALLATION.md)
- [Configuration Reference](../reference/CONFIG.md)
- [Troubleshooting](../how-to/TROUBLESHOOTING.md)
