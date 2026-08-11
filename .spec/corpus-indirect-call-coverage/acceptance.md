# Acceptance

- The fixture contains unknown lowercase calls with sigiled receivers and multiple arguments, preserved as ordinary function calls.
- It contains builtin filehandle syntax and an indirect constructor.
- It contains an ordinary user-defined call inside control flow.
- It contains a comma-separated call that remains an ordinary function call.
- The manifest records executable AST expectations for the selected nodes, including the user-defined-call negative boundary.
- The public parser E2E test selects the fixture.
- The change does not touch perl-core harnesses, TAP receipts, or upstream smoke lanes.
