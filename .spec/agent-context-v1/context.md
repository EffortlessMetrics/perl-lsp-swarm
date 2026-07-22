# Agent context v1: read-only LSP orientation

## Problem

Agents and generic LSP clients must discover the server's useful custom command
surface and workspace setup guidance before choosing a diagnostic explanation,
module-resolution explanation, or edit-preview request. The current server has
those facts in separate surfaces, but no single standard LSP command points a
client at them.

## Selected seam

Add `perl.agentContext` to `workspace/executeCommand`. The response is a thin
envelope around the existing `perl.workspaceTrustReport`, the current custom
execute-command advertisement, and static pointers to existing follow-up
commands.

## Reuse

- `crates/perl-lsp-rs/src/runtime/language/workspace_trust_report.rs` remains
  the source of workspace/setup facts and redaction boundaries.
- `crates/perl-lsp-rs/src/runtime/language/misc.rs` remains the live command
  dispatcher.
- `crates/perl-lsp-rs-core/src/protocol/capabilities.rs` and
  `crates/perl-lsp-rs/src/execute_command/provider.rs` remain the command
  advertisement lists and must stay in parity.
- `crates/perl-lsp-rs-core/src/runtime/input_validation/constants.rs` remains
  the execute-command security allowlist.
- `LspServer::advertised_feature_ids` stores the exact canonical IDs emitted by
  the latest initialize response, including initialization-option disables.
- `schemas/workspace_trust_report.v1.schema.json` remains the nested report
  contract; `schemas/agent_context.v1.schema.json` governs only the envelope.

## Contract decisions

- The command is additive and uses the existing standard
  `workspace/executeCommand` request.
- `arguments` follows the standard LSP optional property contract. An omitted
  field and an empty array are both normalized to no arguments. An optional
  first object is forwarded to the existing trust report as caller-supplied
  client runtime state.
- `execute_commands` and command-backed `next_actions` reflect the current
  initialized capability state. They are empty or omitted when the client
  disables `lsp.execute_command`; the source-backed setup pointer remains.
- `next_actions` are pointers, not execution instructions. They do not create
  new facts, execute commands, mutate files, or grant permission to apply edits.
- The response claims current runtime/advertisement state only. It does not
  claim that Perl, perldoc, DAP, a bridge, or any external tool is available.

## Alternatives rejected

- A new scheduler, agent control plane, or background session manager: not
  required for orientation and would broaden the seam.
- A new probe/doctor command: conflicts with the existing report-only trust
  contract and would make onboarding dependent on side effects.
- A bridge-specific MCP API: the repository does not prove that every bridge
  forwards arbitrary custom execute commands, so the contract stays standard
  LSP and documents bridge support as conditional.
