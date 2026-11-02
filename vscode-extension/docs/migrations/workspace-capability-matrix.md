# Workspace capability matrix

The extension manifest declares a workspace extension with `virtualWorkspaces.supported:
false` and `untrustedWorkspaces.supported: false`. VS Code keeps the extension disabled
in virtual workspaces and in restricted (untrusted) workspaces until the user grants
Workspace Trust. The runtime records a `workspace_topology.v1` object in client trust
reports and first-hour receipts so host classification can be checked against those
manifest boundaries.

| Host/workspace mode        | Product support                         | Current evidence                                 | Claim boundary                                                                        |
| -------------------------- | --------------------------------------- | ------------------------------------------------ | ------------------------------------------------------------------------------------- |
| Trusted local single-root  | Supported                               | Exact-source smoke and topology contract tests   | Exercises one file-backed folder                                                      |
| Trusted local multi-root   | Supported                               | Exact-source capability smoke and topology tests | Exercises two file-backed folders in a real host                                      |
| Untrusted file workspace   | Unsupported until trust is granted      | Untrusted capability smoke (decline oracle)      | Records topology/trust; does not treat provider success as supported                  |
| Virtual workspace/document | Unsupported                             | URI classification tests                         | Capability projection is product-declined; no virtual-host LSP claim                  |
| Untitled Perl document     | Supported in trusted hosts (degraded fs) | Untitled URI classification tests                | No workspace-root path is invented                                                    |
| Remote workspace host      | Not promoted by local proof             | Remote-host classification test                  | Requires a scheduled real remote-host receipt (#9873 train)                           |

The classifier records workspace mode, trust state, host kind, folder count, URI schemes,
untitled/virtual document counts, **manifest-gated** capability statuses, and limitations.
It intentionally does not claim server initialization, provider success, remote process
execution, or filesystem access for unsupported URI schemes or untrusted workspaces. Those
behaviors require the corresponding host receipt or an explicit trust grant.
