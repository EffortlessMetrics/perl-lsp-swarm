//! Product-level reservation for the native MCP adapter command.
//!
//! This module owns only argv separation and fail-closed candidate behavior.
//! It must not start the LSP runtime or claim that the native adapter exists.

use std::io::Write;

const MCP_USAGE: &str = "Usage: perllsp mcp --stdio [--workspace <ROOT>]";
const MCP_HELP: &str = "Native MCP adapter command (reserved)\n\n\
Usage: perllsp mcp --stdio [--workspace <ROOT>]\n\n\
Options:\n\
  --stdio             Use MCP over stdio\n\
  --workspace <ROOT>  Fix the MCP session root\n\
  -h, --help          Print help\n\n\
Status:\n\
  The native MCP adapter is not available in this candidate.\n\
  This command never starts the LSP runtime.\n";
const MCP_UNAVAILABLE: &str = concat!(
    "`perllsp mcp --stdio` is reserved for the native MCP adapter, ",
    "which is not available in this candidate.\n",
    "No MCP server was started.\n",
);

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum McpProductAction {
    Help,
    Launch,
}

/// Run the product-level MCP command when the explicit `mcp` namespace is selected.
///
/// Returning `None` leaves every existing LSP and utility command on the established
/// lower CLI path. Returning `Some` means this module fully owned the invocation and
/// the lower LSP parser must not see it.
pub(super) fn try_run(args: &[String]) -> Option<u8> {
    if args.get(1).map(String::as_str) != Some("mcp") {
        return None;
    }

    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    Some(run(&args[2..], &mut stdout, &mut stderr))
}

fn run(args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    match parse_invocation(args) {
        Ok(McpProductAction::Help) => write_with_exit_code(stdout, MCP_HELP, 0),
        Ok(McpProductAction::Launch) => write_with_exit_code(stderr, MCP_UNAVAILABLE, 1),
        Err(error) => {
            let rendered = format!("error: {error}\n{MCP_USAGE}\n");
            write_with_exit_code(stderr, &rendered, 1)
        }
    }
}

fn parse_invocation(args: &[String]) -> Result<McpProductAction, &'static str> {
    if matches!(args, [arg] if arg == "--help" || arg == "-h") {
        return Ok(McpProductAction::Help);
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Err("`perllsp mcp --help` cannot be combined with launch options");
    }

    let mut saw_stdio = false;
    let mut saw_workspace = false;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--stdio" if saw_stdio => {
                return Err("`perllsp mcp` accepts `--stdio` only once");
            }
            "--stdio" => saw_stdio = true,
            "--workspace" if saw_workspace => {
                return Err("`perllsp mcp` accepts `--workspace` only once");
            }
            "--workspace" => {
                index += 1;
                let Some(root) = args.get(index) else {
                    return Err("`perllsp mcp --workspace` requires a root path");
                };
                if root.is_empty() || root.starts_with('-') {
                    return Err("`perllsp mcp --workspace` requires a root path");
                }
                saw_workspace = true;
            }
            "--mcp" => {
                return Err("`--mcp` is not a transport alias; use the `mcp` subcommand");
            }
            _ => return Err("unknown `perllsp mcp` argument"),
        }
        index += 1;
    }

    if !saw_stdio {
        return Err("`perllsp mcp` requires the explicit `--stdio` transport");
    }

    Ok(McpProductAction::Launch)
}

fn write_with_exit_code(writer: &mut dyn Write, text: &str, code: u8) -> u8 {
    if writer.write_all(text.as_bytes()).is_ok() { code } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::{MCP_HELP, MCP_UNAVAILABLE, McpProductAction, parse_invocation, run};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn explicit_mcp_grammar_accepts_stdio_and_optional_workspace() {
        assert_eq!(parse_invocation(&args(&["--stdio"])), Ok(McpProductAction::Launch));
        assert_eq!(
            parse_invocation(&args(&["--workspace", ".", "--stdio"])),
            Ok(McpProductAction::Launch)
        );
        assert_eq!(
            parse_invocation(&args(&["--stdio", "--workspace", "workspace"])),
            Ok(McpProductAction::Launch)
        );
    }

    #[test]
    fn explicit_mcp_help_is_not_a_launch_action() {
        assert_eq!(parse_invocation(&args(&["--help"])), Ok(McpProductAction::Help));
        assert_eq!(parse_invocation(&args(&["-h"])), Ok(McpProductAction::Help));
    }

    #[test]
    fn explicit_mcp_grammar_rejects_ambiguous_or_lsp_arguments() {
        assert_eq!(
            parse_invocation(&args(&[])),
            Err("`perllsp mcp` requires the explicit `--stdio` transport")
        );
        assert_eq!(
            parse_invocation(&args(&["--socket", "--stdio"])),
            Err("unknown `perllsp mcp` argument")
        );
        assert_eq!(
            parse_invocation(&args(&["--stdio", "--stdio"])),
            Err("`perllsp mcp` accepts `--stdio` only once")
        );
        assert_eq!(
            parse_invocation(&args(&["--workspace", "--stdio"])),
            Err("`perllsp mcp --workspace` requires a root path")
        );
        assert_eq!(
            parse_invocation(&args(&["--stdio", "--help"])),
            Err("`perllsp mcp --help` cannot be combined with launch options")
        );
        assert_eq!(
            parse_invocation(&args(&["--mcp", "--stdio"])),
            Err("`--mcp` is not a transport alias; use the `mcp` subcommand")
        );
    }

    #[test]
    fn accepted_launch_is_fail_closed_and_protocol_clean() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run(&args(&["--workspace", ".", "--stdio"]), &mut stdout, &mut stderr);

        assert_eq!(code, 1);
        assert!(stdout.is_empty());
        assert_eq!(String::from_utf8_lossy(&stderr), MCP_UNAVAILABLE);
        assert!(!String::from_utf8_lossy(&stderr).contains("Content-Length"));
    }

    #[test]
    fn help_is_a_non_protocol_stdout_surface() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run(&args(&["--help"]), &mut stdout, &mut stderr);

        assert_eq!(code, 0);
        assert_eq!(String::from_utf8_lossy(&stdout), MCP_HELP);
        assert!(stderr.is_empty());
    }
}
