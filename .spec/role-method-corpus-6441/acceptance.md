# Acceptance — role-method parser corpus

- The existing `role_method` fixture has concrete AST expectations.
- The public parser E2E test exercises the fixture.
- Expectations cover package, subroutine, return, and call nodes.
- The change measures existing parser output and does not change parser behavior.
- The claim remains bounded to this fixture; role resolution and runtime dispatch are not inferred.