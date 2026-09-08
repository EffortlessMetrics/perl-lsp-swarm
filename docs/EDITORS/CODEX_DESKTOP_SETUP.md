# Codex Desktop integration status for perl-lsp

> **Status: unsupported / not proven.**
>
> Codex Desktop has no documented surface for registering a custom language
> server. There is no `perllsp --stdio` configuration to perform here, and this
> project does not claim direct Codex Desktop support.

## Why there is no setup guide

Earlier revisions of this page instructed users to add a Perl "custom language
server" that runs `perllsp --stdio` inside Codex Desktop. No OpenAI source
documents such an extension point: the current Codex plugin contract covers
plugins, skills, connectors, hooks, and MCP servers — not language servers over
stdio. Those instructions were invented and have been removed rather than kept
as deprecated steps.

The [client support ledger](../project/status/lsp_clients.md) records this host
as `no_documented_lsp_surface` with tier `not_proven_unsupported`.

## Current Codex host boundaries

| Host | Current boundary |
| --- | --- |
| Codex Desktop | no documented custom-language-server surface; not proven |
| Codex in the ChatGPT desktop app | plugins can bundle skills and MCP servers; any `perllsp` route needs a real host receipt first (#6961) |
| Codex CLI | registers MCP servers, not LSP servers; see [Codex CLI setup](CODEX_CLI_SETUP.md) |
| Codex IDE extension | plugins are unavailable under the current upstream contract |

A third-party LSP-to-MCP bridge may provide compatibility on MCP-capable hosts.
It is not bundled, supported, or verified by perl-lsp.

## Verify your installation independently

The server itself must be healthy regardless of editor support:

```bash
perllsp --version
perllsp --health
```

If the server does not start, follow
[troubleshooting](../how-to/TROUBLESHOOTING.md).
