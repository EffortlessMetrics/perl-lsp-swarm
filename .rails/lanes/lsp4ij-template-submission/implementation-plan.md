# Implementation Plan — LSP4IJ Template Submission

## Phase 1 — Source-of-truth rail
- Define lane, proposal, spec, and rollout bridge docs.
- Confirm scope boundaries (no DAP, no Neovim latency work in submission PR).

## Phase 2 — Local template fixture
- Create `tools/lsp4ij/templates/lsp/perl-lsp/` fixture files.
- Keep language mapping conservative.
- Use server-native nested settings keys.

## Phase 3 — Validation tooling
- Add `cargo xtask lsp4ij-template validate --template <path>`.
- Enforce JSON parse, required fields, release asset contract names, and non-dotted settings keys.

## Phase 4 — Release/install contract
- Document required release asset names and archive content.
- Add receipt proving GitHub assets and local executable behavior contract.

## Phase 5 — Compatibility receipts
- Add raw LSP compatibility smoke test + receipts.
- Add manual LSP4IJ validation template and executed receipt.

## Phase 6 — Upstream payload
- Add render command to stage upstream payload files.
- Ensure payload only contains template/doc files expected by LSP4IJ.

## Claim Boundary
This lane prepares and validates the LSP-only upstream submission path. DAP and editor latency rails remain separate follow-up concerns.

## Rollback
If validation reveals incompatibilities, revert fixture/render changes and keep lane docs with blockers recorded; do not submit upstream until receipts are green.
