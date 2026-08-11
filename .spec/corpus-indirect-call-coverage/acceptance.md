# Acceptance

- The fixture contains user-defined indirect calls with sigiled receivers and multiple arguments.
- It contains builtin filehandle syntax and an indirect constructor.
- It contains an indirect call inside control flow.
- It contains a comma-separated call that remains an ordinary function call.
- The manifest records executable AST expectations for the selected nodes.
- The public parser E2E test selects the fixture.
- The change does not touch perl-core harnesses, TAP receipts, or upstream smoke lanes.
