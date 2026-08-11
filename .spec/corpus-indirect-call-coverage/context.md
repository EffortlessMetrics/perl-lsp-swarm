# Context

The parser-accuracy corpus has no fixture for indirect-call disambiguation even though the parser exposes an `IndirectCall` AST node and focused parser tests cover the local grammar seam. The missing corpus fixture leaves the behavior out of the manifest-backed end-to-end parser contract.

This slice is corpus-only. It does not change parser behavior, the LSP provider contract, or the CPAN harness lanes.
