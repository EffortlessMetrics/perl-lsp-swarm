# LSP4IJ Template Submission Rollout

## Goal
Prepare and validate a minimal upstream contribution that adds `perl-lsp` as an LSP4IJ user-defined language server template.

## Internal Sequence
1. Source-of-truth rail established in `.rails/`.
2. Local template fixture created and validated.
3. Release asset contract documented and verified.
4. Raw LSP compatibility receipt captured.
5. Manual LSP4IJ validation captured.
6. Upstream payload rendered and reviewed.

## Scope Controls
- **LSP template submission:** in scope.
- **DAP support:** explicit follow-up unless manually validated and separately proposed.
- **Neovim latency rail:** separate and non-blocking for LSP4IJ submission.

## Required Proof for Upstream Readiness
- JSON fixture validates.
- Installer asset names match published release contract.
- `perllsp` extraction and health checks are receipted.
- LSP behavior receipts (raw + manual) are present.
- Upstream payload contains only expected template/doc files.

## Claim Boundary
This rollout prepares only the LSP4IJ LSP template and associated docs/receipts. It does not claim DAP template readiness or broader editor/runtime quality milestones.

## Rollback
If any compatibility or release contract check fails, stop upstream submission and keep changes local to lane/fixture/docs until mismatch is resolved.
