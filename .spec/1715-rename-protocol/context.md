# Issue #1715 — Rename Protocol Context

## Objective

Align rename preparation and workspace-edit responses with the negotiated LSP
protocol shapes, while keeping the implementation limited to the rename seam.

## Scope

- Accept only the defined `prepareSupportDefaultBehavior` value `1`.
- Return the `defaultBehavior` variant only for a valid non-sigiled rename
  target; reserved Perl keywords remain unavailable for rename.
- Convert `WorkspaceEdit.changes` to `documentChanges` when requested by the
  client, preserving WorkspaceEdit metadata and live document versions.
- Keep empty rename responses in the negotiated WorkspaceEdit format.
- Add direct unit and integration proof for each contract.

## Non-goals

- Generating or attaching change annotations.
- Claiming complete issue #1715 closure beyond the protocol behaviors listed
  above.
- Changing rename semantics outside prepare-rename validation and WorkspaceEdit
  serialization.

## Claim boundary

Open documents use their tracked `DocumentState.version`; documents not present
in the live document map use `version: null`. Hosted required checks remain the
integration authority for the merge decision.
