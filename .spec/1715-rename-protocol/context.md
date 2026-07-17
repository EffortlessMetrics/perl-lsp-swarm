# Context: #1715 — Rename Protocol

## Problem

The rename implementation negotiated the `defaultBehavior` and
`documentChanges` protocol shapes incompletely: capability integers could wrap,
keyword positions could be advertised as renameable, empty edits bypassed the
formatter, and converted edits lost live document versions or WorkspaceEdit
metadata. This weakens client validation and stale-edit protection.

## Why this approach

Keep the internal rename representation as `WorkspaceEdit.changes`, then format
it at the response boundary according to the client capability. The formatter
can read the live document map for versions and clone the top-level object so
metadata remains intact. Prepare-rename keeps sigiled ranges server-controlled
and delegates only valid plain identifiers to client default behavior.

## Alternatives rejected

- **Make all document changes unversioned**: rejected because open-document
  versions provide stale-edit protection.
- **Replace the internal `changes` representation**: rejected because it would
  broaden the seam into rename construction and workspace-index code.
- **Generate change annotations in this PR**: deferred because it is separate
  protocol functionality and would exceed the bounded rename slice.

## Prior art / duplicates

The existing private `to_workspace_edit_format` helper and `DocumentState`
version field are the local prior art. No duplicate `.spec` artifact existed for
#1715; this directory is the authoritative acceptance record for this PR.

## Links

- Issue: #1715
- PR: #4406
- LSP 3.17 specification: https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/
- Hazard defaults: `docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md` (LSP section)
- Canonical spec shape: `docs/reference/SPEC_TEMPLATE.md`
- Related follow-up: change-annotation support remains explicitly deferred
