# #4998 - External include-root authority context

## Proposition

Machine-scoped `externalIncludePaths` must be admissible only from a
server-owned trusted user/operator source. Transport position in a
`workspace/configuration` result array, key spelling, client names, and
client-supplied scope labels confer no authority (#4998): any LSP client can
forge them, so a hostile repository plus a generic client reaches the same
read-any-file primitive that `.perl-lsp.toml` validation closed.

## Current-main state when this packet was written

PR #5021 landed the key split (`perl-lsp.includePaths` resource /
`perl-lsp.externalIncludePaths` machine), resource-scope path rejection, and
the extension-side `inspect().globalValue` projection. The issue was reopened
because the server still promoted the first unscoped `workspace/configuration`
result and every `workspace/didChangeConfiguration` payload into machine trust
through `WorkspaceConfigUpdateContext::apply_external_include_paths: bool`.

## This slice

- Replace the positional boolean with a typed
  `ExternalIncludePathAuthority` (`Untrusted(channel)` |
  `TrustedUserOperator`) at the configuration application boundary.
- Classify every current production channel untrusted:
  initialization options, didChangeConfiguration, unscoped configuration slot,
  folder-scoped configuration slots, project/resource config, unknown.
- Unauthorized non-empty arrivals are rejected with an actionable reason and
  never clear previously accepted trusted values.
- Contained relative `includePaths` behavior is unchanged on all channels.
- Align product claims: the VS Code setting description states server-side
  application is pending a trusted transport.

## Non-goals

- No trusted operator adapter implementation; `TrustedUserOperator` is
  reserved for the #10817 observation train and for tests proving the rule is
  not "absolute paths are impossible".
- No change to PERL5LIB / interpreter-startup `@INC` provenance.
- No `.perl-lsp.toml` behavior change beyond unchanged relative validation.
- Full runtime-derived root reclassification stays with #10813/#10817/#10807;
  this slice adds only a recurrence gate test pinning dependency-detected
  roots to relative workspace-contained literals.
