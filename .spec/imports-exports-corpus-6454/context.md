# Context — imports and exports parser corpus

Issue: #6454

The imports_exports fixture already exercises package loading, export metadata, and imported calls through symbol-oriented expectations, but it had no executable AST anchors. This slice measures the parser projections that downstream navigation and import analysis consume: Package, Use, VariableDeclaration, Subroutine, Return, and FunctionCall.
