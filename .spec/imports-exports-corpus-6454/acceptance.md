# Acceptance — imports and exports parser corpus

- The existing imports_exports fixture is registered with concrete AST expectations.
- The public parser E2E test exercises the fixture.
- The expectations cover both packages, use statements, export storage, subroutines, returns, and the imported function call.
- The change measures existing parser output and does not change parser behavior.
