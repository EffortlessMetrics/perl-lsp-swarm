# Zed integration status for perl-lsp

> **Status: planned / not proven.**
>
> The public Zed Perl extension does **not** register `perllsp`. Installing the
> binary alone therefore does not create a working public Zed integration, and
> this project does not currently claim direct Zed support.

## The current product identities

The public Perl extension currently exposes two independent servers:

| Zed server ID | Product |
| --- | --- |
| `perlnavigator-server` | Perl Navigator |
| `perl-lsp` | `tree-sitter-perl/perl-tree-sitter-lsp` |

EffortlessMetrics ships a different executable and product:

| Intended Zed server ID | Product |
| --- | --- |
| `perllsp` | `EffortlessMetrics/perl-lsp` |

Do not configure the existing `perl-lsp` ID to launch `perllsp`. That substitutes
one product behind another product's identity and can make logs, downloads,
settings, and support evidence point at the wrong server.

## Why `settings.json` is not enough

Zed language extensions register the server IDs available for a language. The
`lsp` section in Zed settings can configure or override an already registered
server, but it does not register a new arbitrary server for Perl.

A correct public route therefore requires the Perl extension to register a
third, dedicated `perllsp` server ID and dispatch it explicitly to
`perllsp --stdio`.

## Prepared upstream candidate

The repository carries a submission-ready candidate under:

```text
.ci/fixtures/zed-perl-upstream/
```

It is bound to an exact `tree-sitter-perl/zed-perl` base and includes:

- a separate `perllsp` server registration;
- exhaustive dispatch that rejects unknown IDs instead of falling through to
  Perl Navigator;
- PATH-first `perllsp` resolution;
- managed downloads from `EffortlessMetrics/perl-lsp` for checked release
  targets;
- explicit `--stdio` launch;
- `.PL`, `.psgi`, `.cgi`, and `.fcgi` activation while preserving the separate
  POD language;
- default mappings for `perllsp` custom semantic-token types;
- a separate Zed-defaults fragment that keeps both alternative Perl servers
  dormant until selected.

See [ZED_UPSTREAM_SUBMISSION.md](../integrations/ZED_UPSTREAM_SUBMISSION.md) for
the exact base, apply script, verification chain, and copy-ready upstream PR
text.

## Expected configuration after upstream registration

This is the intended future shape, not a currently supported public setup. Once
a released Perl extension actually registers `perllsp`, select it and disable
the other Perl servers:

```json
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
  }
}
```

A user-installed binary can then be selected through the dedicated identity:

```json
{
  "lsp": {
    "perllsp": {
      "binary": {
        "path": "/absolute/path/to/perllsp",
        "arguments": ["--stdio"]
      }
    }
  }
}
```

Until the registration exists in a released extension, that settings entry has
no public server to configure.

## Project configuration

Use `.perl-lsp.toml` in the project root for settings shared across editors.
That keeps project configuration independent of extension release timing and
avoids claiming that an older public `perllsp` release supports a newer
initialization-options contract.

A future released extension may also forward Zed-specific settings through
`workspace/configuration`, but those fields need a version-bound actual-host
receipt before this guide treats them as supported.

## File associations

The prepared Perl-language update covers:

```text
.pl  .PL  .pm  .t  .psgi  .cgi  .fcgi
```

`.pod` remains assigned to the extension's separate POD language and grammar.
Do not add `.pod` to the Perl file-type override.

## Semantic tokens

Zed keeps LSP semantic tokens disabled by default. The prepared extension maps
`perllsp`'s custom SQL and JSON heredoc token types, but that package integrity
does not prove rendered behavior in Zed. Semantic-token support remains a
separate receipt cell after upstream registration.

## Evidence required before promotion

Merging or compiling the extension is not enough to promote the support claim.
The first actual-host receipt must bind:

```text
Zed version and platform
Perl extension version/ref
perllsp path, version, build identity, and hash
winning server ID
workspace root/configuration behavior
pull diagnostics after open and edit
completion plus hover/navigation
one edit followed by a changed re-query result
one applied workspace edit
shutdown, exit, and orphan-process result
```

A second receipt must repeat the journey using the released public extension
and a public `perllsp` artifact. Only then may the support registry and generated
documentation promote the relevant Zed cells.

For server-side configuration and general diagnostics, see
[CONFIG.md](../reference/CONFIG.md) and
[TROUBLESHOOTING.md](../how-to/TROUBLESHOOTING.md).
