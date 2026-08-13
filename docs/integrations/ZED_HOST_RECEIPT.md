# Actual Zed host receipt

> **State:** harness and fail-closed template only; no Zed host result is recorded.
>
> **Owner:** [#7907](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7907)

The Zed integration cannot become submission-ready from a WASM build or a
synthetic LSP client. This runbook records the exact subjects and bounded journey
required from a real Zed desktop process.

## Receipt files

- Schema: `.ci/schemas/zed-host-compat.v1.schema.json`
- Exact-source template:
  `.ci/fixtures/zed-perl-upstream/receipts/exact-source-template.json`

Copy the template to a content-addressed receipt path before execution. Do not
edit the template into a permanent pass in place.

## Clean subject

The run must bind:

```text
exact Zed version/channel/build
exact tree-sitter-perl/zed-perl base and candidate commit
built extension WASM SHA-256
exact perllsp path/version/build/SHA-256
binary resolution route
OS/version/architecture
clean profile and prior-cache state
fixture and settings digests
```

The selected server order must use `perllsp` and disable the other two providers.
Another Perl server process cannot satisfy any `perllsp` cell.

## Route A — explicit binary or worktree PATH

1. Install the exact candidate as a Zed development extension in a clean profile.
2. Select `perllsp`; disable `perlnavigator-server` and `perl-lsp`.
3. Resolve the candidate through an explicit `binary.path` or the worktree PATH.
4. Capture the effective process and prove exact `perllsp --stdio`.
5. Run the bounded journey below.

## Route B — managed public artifact

This route begins only after #7903 has an executable public-asset receipt.
Remove binary overrides, start from an empty managed cache, and prove the exact
asset target, digest, extraction path, launched process, restart reuse, and
known-good preservation on a bounded failed candidate.

Route A cannot satisfy Route B.

## Bounded journey

Record evidence or an explicit non-pass result for:

- extension discovery and Perl attachment;
- initialize/initialized and workspace root;
- diagnostics on open/change/save and removal after repair;
- completion, hover, definition, references, document symbols, workspace symbols;
- one safe edit or explicit bounded refusal;
- non-ASCII positions and mixed newline handling;
- canonical `workspace/configuration` behavior;
- custom semantic-token rendering where emitted;
- post-edit freshness and restart/re-index;
- clean shutdown with no orphaned `perllsp` process.

Exercise `.pl`, `.pm`, `.t`, `.PL`, `.psgi`, `.cgi`, `.fcgi`, optional shebang
activation, and `.pod`. The `.pod` pass means Zed selected the separate POD
language and did not attach `perllsp` through the Perl registration.

## False-green boundary

The validator rejects a pass when exact host, extension, binary, fixture,
provider isolation, logs, or required journey cells are absent. It also rejects
another provider ID, a non-LSP process route, and a development extension
presented as an official-registry install.

A green exact-source receipt may unblock manual upstream submission. It does not
prove the public registry route or promote the public Zed support row; #7912 owns
that later evidence stage.
