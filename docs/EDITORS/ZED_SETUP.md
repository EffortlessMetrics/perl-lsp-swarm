# Zed integration status for perl-lsp

> **Status: planned / not proven.**
>
> The proposed EffortlessMetrics provider, `perl-lsp-rs`, has no public setup.
> The public Perl extension must register the provider before Zed settings can
> select it.

## Perl providers

The [captured upstream manifest](https://github.com/tree-sitter-perl/zed-perl/blob/286bfb84548d25a95214dea92c347c56451cf14c/extension.toml)
for the public Zed Perl extension (extension ID `perl`) registers two providers:

| Zed server ID | Product |
| --- | --- |
| `perlnavigator-server` | Perl Navigator |
| `perl-lsp` | `tree-sitter-perl/perl-tree-sitter-lsp` |

The proposed third provider is `perl-lsp-rs`, displayed as **Perl LSP
(EffortlessMetrics)**. Its product is `EffortlessMetrics/perl-lsp`, and its
launch command is `perllsp --stdio`. The existing `perl-lsp` settings belong
to the tree-sitter-perl provider. Leave its binary path assigned to that
provider.

## Development candidate

The repository stages an extension candidate under
`.ci/fixtures/zed-perl-upstream/`. Its provisional registration belongs to the
staged fixture. The public provider proposal is tracked in
[#10842](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10842).
See the [upstream submission guide](../integrations/ZED_UPSTREAM_SUBMISSION.md)
for the exact-base staging material.

## Project configuration

Use `.perl-lsp.toml` in the project root for settings shared across editors.
That keeps project configuration independent of extension release timing and
avoids claiming that an older public `perllsp` release supports a newer
initialization-options contract.

A future released extension may also forward Zed-specific settings through
`workspace/configuration`, but those fields need a version-bound actual-host
receipt before this guide treats them as supported.

## File associations

The staged Perl-language update covers:

```text
.pl  .PL  .pm  .t  .psgi  .cgi  .fcgi
```

`.pod` remains assigned to the extension's separate POD language and grammar.
Do not add `.pod` to the Perl file-type override.

## Semantic tokens

Zed keeps LSP semantic tokens disabled by default. The staged extension maps
`perllsp`'s custom SQL and JSON heredoc token types, but that package integrity
does not prove rendered behavior in Zed. Semantic-token support remains a
separate receipt cell after upstream registration.

## Debugger support (perl-dap)

Zed debugger support is a separate evidence surface from the language-server
row above; its current status and earned cells are projected by the generated
surface [ZED_DAP_SUPPORT.md](ZED_DAP_SUPPORT.md) from the committed journey
receipts. Installing the Perl extension does not by itself prove debugging.
The `perl-dap` adapter has its own identity and evidence. Its status never
changes this page's LSP verdict, and an LSP change never alters the debugger cells.

## Evidence required before promotion

Generic LSP operation shows that the native executable can serve a protocol
client. A staged extension and a build establish repository-level facts.
An exact-source development-extension receipt must then bind the actual Zed
version and platform, the extension source and build, the selected provider,
the binary identity, and the behavior observed in that host.

Public artifact execution and official registry publication are separate
milestones. Public support requires an installed-host receipt using the
released Perl extension and exact public executable, followed by projection
of the earned cells into the support registry and generated documentation.

## Competing Perl tooling

Zed does not run the VS Code coexistence detector. Use `perllsp --health`, your
editor configuration, and [Competing Perl tooling (coexistence)](COEXISTENCE.md) for
honest overlap guidance without inventing automatic conflict detection.

For server-side configuration and general diagnostics, see
[CONFIG.md](../reference/CONFIG.md) and
[TROUBLESHOOTING.md](../how-to/TROUBLESHOOTING.md).
