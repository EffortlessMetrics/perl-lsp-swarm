# Acceptance criteria: agent context v1

- [ ] `perl.agentContext` is advertised by both command lists, admitted by the
  input-validation allowlist, and covered by parity tests.
- [ ] `workspace/executeCommand` returns `agent_context.v1` with the nested
  `workspace_trust_report.v1`, current advertised LSP feature IDs, the
  advertised custom command IDs, and the documented next-action pointers.
- [ ] The response is read-only and its claim boundary states that it does not
  scan, probe, run perldoc, launch DAP, apply edits, or execute follow-ups.
- [ ] The response accepts a caller-supplied runtime-state object while
  preserving the trust report's existing sanitization.
- [ ] The request accepts both an omitted `arguments` property and an explicit
  empty array, while rejecting a present non-array value.
- [ ] Command inventory and command-backed next actions reflect whether the
  initialized session advertised `lsp.execute_command`.
- [ ] `schemas/agent_context.v1.schema.json` matches the implementation and
  references the existing trust-report schema.
- [ ] Command reference and Codex onboarding docs explain the request shape,
  conditional bridge support, and the report-only boundary.

## Proof

```text
cargo test -p perl-lsp-rs --test lsp_execute_command_tests test_execute_command_agent_context_is_read_only_and_actionable --profile agent --locked -- --nocapture --test-threads=1
cargo test -p perl-lsp-rs --test lsp_execute_command_tests test_execute_command_capabilities --profile agent --locked -- --nocapture --test-threads=1
cargo test -p perl-lsp-rs-core --lib test_validate_execute_command_allows_agent_context --profile agent --locked -- --nocapture --test-threads=1
cargo test -p perl-lsp-rs --lib test_supported_commands_includes_go_to_test --profile agent --locked -- --nocapture --test-threads=1
git diff --check
```

## Non-goals

- No change to workspace indexing, module resolution, diagnostics, providers,
  DAP, perldoc, bridge implementations, or editor-specific APIs.
- No support-tier promotion, telemetry, file scan, process probe, or edit
  application.
