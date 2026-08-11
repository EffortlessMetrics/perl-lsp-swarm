# Acceptance

- Existing format_decl and control_do_until parser-accuracy fixtures are selected by the public E2E test.
- The manifest contains executable entries for continue/redo, glob expressions, and tie/untie.
- Each new fixture parses through the public parser API and has package/subroutine AST anchors.
- Fixture source covers labeled loop control, glob function and diamond syntax, and tie/tied/untie operations.
- The manifest remains valid JSON and each fixture path exists.
- The change preserves the current parser representation where no dedicated NodeKind exists.
- The change does not touch CPAN harnesses, TAP receipts, or unrelated LSP behavior.
