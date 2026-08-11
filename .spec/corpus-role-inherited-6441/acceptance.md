# Acceptance — role and inherited-method parser corpus

- `role_method` has concrete package, subroutine, return, and call expectations.
- `inherited_method` has concrete package, subroutine, return, and qualified-call expectations.
- Both fixtures are exercised by the public parser E2E test.
- The manifest remains valid JSON and unrelated fixture formatting is unchanged.
- The claim remains bounded to observable parser output; semantic dispatch is not inferred.
