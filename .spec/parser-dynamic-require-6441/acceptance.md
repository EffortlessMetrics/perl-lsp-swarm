# Acceptance

- The existing `dynamic_require_boundary` manifest fixture is selected by the public parser E2E test.
- Its package, variable, require-call, import, and dynamic-boundary expectations remain executable.
- No parser behavior or manifest gold changes are introduced.

Claim boundary: this closes one selector gap; it does not establish general dynamic loading semantics.
