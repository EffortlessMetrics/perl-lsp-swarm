# PLSP-PROP-LSP4IJ-TEMPLATE-SUBMISSION

## Problem
`perl-lsp` does not yet have an upstream LSP4IJ user-defined language server template, blocking straightforward IntelliJ-side installation and discovery for Perl users.

## Proposal
Establish an internal preparation lane that produces:
1. A validated local LSP4IJ template fixture.
2. A release/install asset contract aligned with LSP4IJ GitHub installer support.
3. Raw and manual compatibility receipts.
4. A minimal upstream payload for `redhat-developer/lsp4ij`.

## Non-goals
- DAP template inclusion in the first upstream PR.
- Neovim latency/performance remediation.
- Cross-editor configuration parity efforts.

## Acceptance
Ready-for-upstream is true only when rail, contract, and compatibility receipts exist and the generated payload is limited to LSP4IJ-expected template/docs files.
