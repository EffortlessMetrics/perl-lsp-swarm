mod claude;

use perllsp::protocol::product_identity::{
    BinaryIdentityPacketV1, IdentityOutputFormat, requested_identity_output,
};
use std::io::Write as _;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ClaudeProductAction {
    Setup,
    Doctor,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ClaudeProductInvocation {
    action: ClaudeProductAction,
    json: bool,
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if let Some(format) = requested_identity_output(&args) {
        let packet = BinaryIdentityPacketV1::embedded_server(env!("CARGO_PKG_VERSION"));
        let rendered = match format {
            IdentityOutputFormat::Human => packet.to_human(),
            IdentityOutputFormat::Json => match packet.to_json() {
                Ok(value) => value,
                Err(error) => {
                    let _ = writeln!(std::io::stderr(), "failed to serialize identity: {error}");
                    return std::process::ExitCode::FAILURE;
                }
            },
        };
        if write!(std::io::stdout(), "{rendered}").is_err() {
            return std::process::ExitCode::FAILURE;
        }
        return std::process::ExitCode::SUCCESS;
    }

    match run_claude_product_command(&args) {
        Ok(Some(code)) => return std::process::ExitCode::from(code),
        Ok(None) => {}
        Err(reason) => {
            let _ = writeln!(std::io::stderr(), "{reason}");
            return std::process::ExitCode::FAILURE;
        }
    }

    std::process::ExitCode::from(perllsp::run_cli(args) as u8)
}

fn run_claude_product_command(args: &[String]) -> Result<Option<u8>, &'static str> {
    if args.get(1).map(String::as_str) == Some("claude") {
        return Err(
            "`perllsp claude ...` is not a public command surface; use `perllsp setup claude` for reconciliation or `perllsp doctor --client claude` for read-only diagnosis",
        );
    }

    let Some(invocation) = parse_claude_product_invocation(args)? else {
        return Ok(None);
    };

    let action = match invocation.action {
        ClaudeProductAction::Setup => "install",
        ClaudeProductAction::Doctor => "doctor",
    };
    let mut internal = vec!["perllsp".to_string(), "claude".to_string(), action.to_string()];
    if invocation.json {
        internal.push("--json".to_string());
    }

    match claude::try_run(&internal) {
        Some(code) => Ok(Some(code)),
        None => Err("internal Claude lifecycle dispatch did not recognize the canonical command"),
    }
}

fn parse_claude_product_invocation(
    args: &[String],
) -> Result<Option<ClaudeProductInvocation>, &'static str> {
    match args.get(1).map(String::as_str) {
        Some("setup") => parse_setup_invocation(&args[2..]).map(Some),
        Some("doctor") => parse_doctor_invocation(&args[2..]),
        _ => Ok(None),
    }
}

fn parse_setup_invocation(args: &[String]) -> Result<ClaudeProductInvocation, &'static str> {
    if args.first().map(String::as_str) != Some("claude") {
        return Err("`perllsp setup` currently requires the explicit client `claude`");
    }

    let json = parse_json_only(&args[1..])?;
    Ok(ClaudeProductInvocation { action: ClaudeProductAction::Setup, json })
}

fn parse_doctor_invocation(
    args: &[String],
) -> Result<Option<ClaudeProductInvocation>, &'static str> {
    let Some(client_index) = args.iter().position(|arg| arg == "--client") else {
        return Ok(None);
    };
    let Some(client) = args.get(client_index + 1) else {
        return Err("`perllsp doctor --client` requires a client name");
    };
    if client != "claude" {
        return Ok(None);
    }

    let mut json = false;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--client" => {
                index += 1;
                if args.get(index).map(String::as_str) != Some("claude") {
                    return Err("Claude integration doctor requires `--client claude`");
                }
            }
            "--json" => json = true,
            _ => return Err("unknown Claude integration doctor argument"),
        }
        index += 1;
    }

    Ok(Some(ClaudeProductInvocation { action: ClaudeProductAction::Doctor, json }))
}

fn parse_json_only(args: &[String]) -> Result<bool, &'static str> {
    let mut json = false;
    for arg in args {
        if arg == "--json" && !json {
            json = true;
        } else {
            return Err("unknown Claude setup argument; only `--json` is currently supported");
        }
    }
    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::{ClaudeProductAction, ClaudeProductInvocation, parse_claude_product_invocation};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn canonical_setup_surface_maps_to_setup_action() {
        let parsed =
            parse_claude_product_invocation(&args(&["perllsp", "setup", "claude", "--json"]));
        assert_eq!(
            parsed,
            Ok(Some(ClaudeProductInvocation { action: ClaudeProductAction::Setup, json: true }))
        );
    }

    #[test]
    fn canonical_doctor_surface_requires_explicit_claude_client() {
        let parsed = parse_claude_product_invocation(&args(&[
            "perllsp", "doctor", "--client", "claude", "--json",
        ]));
        assert_eq!(
            parsed,
            Ok(Some(ClaudeProductInvocation { action: ClaudeProductAction::Doctor, json: true }))
        );
    }

    #[test]
    fn unrelated_existing_doctor_surface_falls_through() {
        assert_eq!(parse_claude_product_invocation(&args(&["perllsp", "doctor"])), Ok(None));
    }

    #[test]
    fn setup_does_not_accept_an_implicit_or_other_client() {
        assert!(parse_claude_product_invocation(&args(&["perllsp", "setup"])).is_err());
        assert!(parse_claude_product_invocation(&args(&["perllsp", "setup", "other"])).is_err());
    }
}
