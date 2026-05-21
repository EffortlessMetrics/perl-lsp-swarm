# PLSP-SPEC-LSP4IJ-TEMPLATE-SUBMISSION

## Objective
Provide a deterministic, receipt-backed path to upstream `perl-lsp` template contribution in LSP4IJ.

## Deliverables
- Lane docs under `.rails/lanes/lsp4ij-template-submission/`.
- Local fixture under `tools/lsp4ij/templates/lsp/perl-lsp/`.
- `xtask` validation command for fixture integrity.
- Release asset contract documentation and receipt.
- Raw protocol and manual LSP4IJ compatibility receipts.
- Upstream payload render output limited to required files.

## Functional Requirements
1. Template fixture defines `id = perl-lsp` and `programArgs` for windows/default.
2. Installer uses GitHub assets keyed by OS/architecture and configures `perllsp --stdio`.
3. Settings are nested server-native keys (no VS Code dotted keys).
4. Validation must fail on missing JSON files, malformed JSON, or contract mismatch.
5. Compatibility receipts must cover initialize/open/diagnostics/completion/hover/symbols/shutdown.

## Scope Partition
- Submission prep: required upstream template + docs payload.
- Compat/support: ongoing docs/receipts maintenance.
- DAP support: separate follow-up track.
- Neovim latency: separate non-blocking track.

## Constraints
- Keep upstream PR small and LSP-only.
- Avoid runtime behavior changes unless validation demands minimal corrective updates.
