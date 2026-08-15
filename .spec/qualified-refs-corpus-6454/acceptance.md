# Acceptance — qualified references parser corpus

- The existing qualified_refs fixture has concrete AST expectations.
- The public parser E2E test exercises the fixture.
- Expectations cover the package, both subroutines, return, local call, and package-qualified call.
- The change measures existing parser output and does not change parser behavior.
