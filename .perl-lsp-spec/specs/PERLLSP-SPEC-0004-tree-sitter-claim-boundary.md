# PERLLSP-SPEC-0004 — Tree-sitter Claim Boundary

## Requirement

Historical tree-sitter documents must explicitly distinguish between:
- historical vendored snapshot results, and
- current upstream target behavior.

Any claim about current upstream behavior must cite parser-target-registry differential receipts.

## Required wording class

Allowed claim class:
- "Our historical vendored target did not meet perl-lsp requirements at measurement time."

Disallowed claim class without current receipts:
- "Tree-sitter does not work for Perl" (global claim).
