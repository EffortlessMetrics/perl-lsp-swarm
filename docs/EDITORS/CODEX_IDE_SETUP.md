# Codex IDE extension integration status for perl-lsp

> **Status: routes documented; exact-host receipts not proven (#7826).**
>
> The Codex IDE extension can expose standalone skills and shared local MCP
> configuration. Those product facts make Perl integration plausible; they do
> not prove discovery, binary selection, tool calls, post-edit freshness, or
> cleanup on your exact host version.

Parent umbrella: [#6961](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6961). Executable host matrix: [#7826](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7826).

## Supported integration shapes (independent rows)

Each row must be receipted separately under `agent_client_compat.v1` with host
`codex_ide`. CLI or desktop receipts cannot substitute.

| Route | Integration mode | Product subject | Current state |
| --- | --- | --- | --- |
| Standalone verification skill | `cli_skill` | exact #7825 skill subject | blocked until #7825 lands |
| Shared/direct local MCP | `native_local_mcp` | `perllsp mcp --stdio` (#6958 / #7230) | blocked until native MCP conformance lands |
| Plugin installability | companion observation (not a semantic mode) | first-party plugin package when available | host-specific `pass` / `unsupported` / `not_proven` |

Plugin availability is **companion host evidence**, not an
`agent_client_compat.v1` integration mode. If a plugin is visible on the tested
IDE surface, packaged skill behavior receipts under `cli_skill` and packaged MCP
behavior under `native_local_mcp` still cannot satisfy the standalone or
direct/shared-config rows.

## Journey A — standalone skill (`cli_skill`)

1. Start from an isolated IDE profile/workspace without the `perl-lsp` VS Code
   extension installed as the semantic subject.
2. Install `perllsp` through the stage-appropriate ordinary route and prove a
   fresh host process resolves it.
3. Install the exact #7825 standalone skill through its documented route.
4. Restart the IDE extension at the documented skill-discovery boundary.
5. Prove the skill appears in the IDE skill selector or explicit invocation
   surface.
6. Invoke it on the canonical known-defect fixture; prove it selects the
   narrowest #7233 command and parses the versioned result.
7. Apply the fixture patch through the IDE/host edit authority (not hidden skill
   mutation); re-invoke affected verification and prove the defect clears.
8. Retain one partial/unreadable/instrument-failure result.
9. Prove exact skill source/path/digest and `perllsp` identity.
10. Remove or update the standalone skill and prove the IDE follows the
    documented refresh boundary.

This row proves deterministic CLI verification only. It does not prove semantic
MCP or plugin installation.

## Journey B — shared local MCP (`native_local_mcp`)

Exercise at least one ordinary setup route; keep CLI-configured and
IDE-configured routes as separate cells.

### CLI-configured shared route

Equivalent to:

```text
codex mcp add perl-lsp -- perllsp mcp --stdio
```

Then open/restart the IDE extension and prove the IDE observes the configured
server without a divergent second command.

### IDE-configured route

When the IDE exposes MCP server UI:

```text
Add server → STDIO → perllsp mcp --stdio → restart extension
```

Record the resulting shared/config identity and whether CLI/desktop can observe
it. Do not infer equivalence because both paths serialize similarly.

Semantic cells must prove actual MCP tool calls (orientation, diagnostics,
definition, references, symbol query, preview), post-edit freshness, preview
non-mutation, restart cleanup, and rejection of editor/search/model false greens.

## Plugin boundary

- Do not claim IDE plugin installation is universally unavailable; record the
  exact tested host/version instead (`unsupported` / `not_proven` / `pass`).
- Plugin state in Codex CLI or ChatGPT desktop does not satisfy IDE rows without
  observed IDE behavior.
- Documentation must say **plugin installation unavailable or not proven on the
  tested host**, not “all Perl integration unsupported.”

## False-green controls

Reject:

- CLI transcripts substituted for IDE behavior;
- answers from editor search, terminal reads, or model inference;
- standalone CLI skill substituted for MCP semantics;
- another configured Perl/MCP server answering;
- stale server processes after extension restart;
- receipts copied from another host, platform, or candidate.

## Evidence class

Use `agent_client_compat.v1` with separate journey cells pinned in
`xtask/src/codex_ide_compat.rs` and validated by
`xtask/tests/codex_ide_compat_contract.rs`.

Optional exact-host execution:

```bash
CODEX_IDE_SMOKE=1 cargo test -p xtask codex_ide_host_smoke -- --ignored --test-threads=1
```

## Related setup docs

- [Codex CLI setup](CODEX_CLI_SETUP.md) — MCP bridge route; does not prove IDE rows.
- [Codex Desktop setup](CODEX_DESKTOP_SETUP.md) — separate host boundary.

The [client support ledger](../project/status/lsp_clients.md) lists the earned
tier for this host. Only exact IDE rows earned under #7826 may flow to #6739 /
#7122 publication surfaces.
