use perl_lsp_rs_core::runtime::{LaunchAction, LaunchParseError, TransportMode, parse_args};
use perl_tdd_support::{must, must_err};

#[test]
fn default_and_explicit_stdio_remain_lsp_transport() {
    for argv in [["perllsp"].as_slice(), ["perllsp", "--stdio"].as_slice()] {
        let plan = must(parse_args(argv.iter().copied()));
        assert_eq!(plan.action, LaunchAction::Run);
        assert_eq!(plan.config.transport, TransportMode::Stdio);
    }
}

#[test]
fn retired_mcp_alias_is_rejected_with_protocol_guidance() {
    let error = must_err(parse_args(["perllsp", "--mcp"]));
    assert!(matches!(&error, LaunchParseError::McpAliasRejected));
    assert_eq!(
        error.to_string(),
        "`--mcp` is not an LSP transport alias.\nUse `perllsp --stdio` for LSP.\nUse `perllsp mcp --stdio` only when the native MCP adapter is available."
    );
}

#[test]
fn mcp_assignment_spelling_is_rejected_too() {
    let error = must_err(parse_args(["perllsp", "--mcp=true"]));
    assert!(matches!(&error, LaunchParseError::McpAliasRejected));
}

#[test]
fn positional_mcp_cannot_become_an_lsp_run_plan() {
    assert!(parse_args(["perllsp", "mcp"]).is_err());
}
