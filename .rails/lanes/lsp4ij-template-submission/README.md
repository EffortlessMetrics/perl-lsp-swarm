# LSP4IJ Template Submission Lane

## Purpose
Track the internal work required to submit a focused upstream PR to `redhat-developer/lsp4ij` adding a `perl-lsp` user-defined LSP template.

## Scope Boundaries
- **In scope:** LSP template fixture, validation, release/install contract checks, LSP compatibility receipts, manual validation receipts, and upstream payload rendering.
- **Out of scope for initial submission:** DAP template submission, Neovim latency remediation work, VS Code/Zed quality rails, and runtime behavior changes unless validation requires a minimal fix.

## Workstream Split
- **Submission prep** → files required for upstream PR payload.
- **Compat/support** → ongoing compatibility receipts and maintenance docs.
- **DAP support** → follow-up lane unless manually validated before opening DAP work.
- **Neovim latency** → separate lane; non-blocking for this submission.

## Success Criteria
1. Local fixture exists and validates.
2. Installer contract matches real release assets.
3. Raw and manual LSP4IJ compatibility receipts are present.
4. Upstream payload is generated and limited to expected template/docs files.
