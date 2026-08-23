## Summary

Add EffortlessMetrics `perllsp` as a third, separately identified language
server in the existing Perl extension.

```text
perlnavigator-server -> Perl Navigator
perl-lsp             -> tree-sitter-perl/perl-tree-sitter-lsp
perllsp              -> EffortlessMetrics/perl-lsp
```

The IDs remain independent. Unknown server IDs fail explicitly instead of
falling through to another provider.

## Process resolution

For the `perllsp` server ID, the extension uses the standard Zed precedence:

```text
lsp.perllsp.binary.path
-> worktree PATH lookup for perllsp
-> checked managed public release asset
```

The effective command is exact `perllsp --stdio`. User arguments that select
MCP, socket, or utility modes are rejected rather than starting a non-LSP
process behind the Zed provider identity.

## Configuration

Zed process configuration remains under `lsp.perllsp.binary`. Server-native
configuration remains under `lsp.perllsp.settings.perl` and is forwarded through
`workspace/configuration`. `.perl-lsp.toml` remains the portable project-level
surface.

## Language integration

The candidate adds the reviewed `.PL`, `.psgi`, `.cgi`, and `.fcgi` Perl
activation suffixes alongside `.pl`, `.pm`, and `.t`. `.pod` remains assigned to
the separate POD language. The extension also includes defaults only for the
custom SQL and JSON heredoc semantic-token types emitted by `perllsp`.

## Managed targets

[BLOCKED: replace this section with the exact final managed target table and
public artifact receipt identities from #7903.]

## Actual Zed evidence

[BLOCKED: replace this section with the exact Zed version, platform, extension
WASM digest, perllsp digest, process identity, bounded journey result, and
receipt ID from #7907.]

## Default server selection

A separate narrow `zed-industries/zed` change keeps `perl-lsp` and `perllsp`
disabled by default while retaining Perl Navigator as the current default. Users
may explicitly select either alternative or multiple providers.

[BLOCKED: state the tested compatibility matrix and final safe submission order
from #7908.]

## Verification

[BLOCKED: insert the final exact commands and green results for formatting,
clippy, host helper tests, `wasm32-wasip2` build, public asset checks, and actual
Zed host execution.]

## Limitations

- No DAP integration is included.
- Platforms and install routes not named by final receipts remain unproven.
- An upstream merge does not by itself prove the official Zed registry install.

> This body is deliberately blocked. It must not be submitted with any
> `[BLOCKED: ...]` marker remaining.
