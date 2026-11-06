#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
mod claude;
mod claude_host_test;
mod mcp;

use perllsp::protocol::product_identity::{
    BinaryIdentityPacketV1, IdentityOutputFormat, IdentityRequest, requested_identity,
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
    host_test: bool,
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();

    // The one-shot identity surface is resolved ahead of the ordinary parser, so
    // a mix is reported here rather than reaching clap — which would deny
    // `--identity` as an unknown option, or answer a requested JSON packet with
    // the human `--info` projection.
    let identity = requested_identity(&args);
    if let Some(message) = identity.rejection_message() {
        let _ = writeln!(std::io::stderr(), "{message}");
        return std::process::ExitCode::FAILURE;
    }
    if let IdentityRequest::Output(format) = identity {
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

    if let Some(code) = mcp::try_run(&args) {
        return std::process::ExitCode::from(code);
    }

    match run_claude_product_command(&args) {
        Ok(Some(code)) => return std::process::ExitCode::from(code),
        Ok(None) => {}
        Err(reason) => {
            let _ = writeln!(std::io::stderr(), "{reason}");
            return std::process::ExitCode::FAILURE;
        }
    }

    // `perllsp doctor ...` (without `--client`) is the shared doctor surface;
    // normalize the positional form onto the shared CLI's `--doctor` flag so
    // invocations like `perllsp doctor --external-tools` work as documented.
    let args = normalize_doctor_invocation(args);
    std::process::ExitCode::from(perllsp::run_cli(args) as u8)
}

/// Rewrite the `perllsp doctor ...` positional form to `perllsp --doctor ...`.
fn normalize_doctor_invocation(mut args: Vec<String>) -> Vec<String> {
    if args.get(1).map(String::as_str) == Some("doctor") {
        args[1] = "--doctor".to_string();
    }
    args
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

    if invocation.host_test {
        if invocation.action != ClaudeProductAction::Doctor {
            return Err("`perllsp --host-test` is only admitted on `doctor --client claude`");
        }
        let mut executor = claude_host_test::ReservedExecutor;
        let outcome = claude_host_test::dispatch(
            claude_host_test::ProductSurface::DoctorClientClaudeHostTest { json: invocation.json },
            &mut executor,
        );
        if writeln!(std::io::stdout(), "{}", outcome.rendered).is_err() {
            return Ok(Some(1));
        }
        return Ok(Some(outcome.exit_code));
    }

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
    Ok(ClaudeProductInvocation { action: ClaudeProductAction::Setup, json, host_test: false })
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
    let mut host_test = false;
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
            "--host-test" if host_test => {
                return Err("`perllsp doctor --host-test` accepts the flag only once");
            }
            "--host-test" => host_test = true,
            "--prompt" | "--command" | "--workspace" | "--env" | "--environment"
            | "--credential" | "--token" | "--executable" | "--retry" | "--timeout"
            | "--profile" | "--count" => {
                return Err(
                    "Claude host-test rejects client-supplied process, prompt, workspace, environment, credential, and profile authority",
                );
            }
            _ => return Err("unknown Claude integration doctor argument"),
        }
        index += 1;
    }

    Ok(Some(ClaudeProductInvocation { action: ClaudeProductAction::Doctor, json, host_test }))
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
            Ok(Some(ClaudeProductInvocation {
                action: ClaudeProductAction::Setup,
                json: true,
                host_test: false,
            }))
        );
    }

    #[test]
    fn canonical_doctor_surface_requires_explicit_claude_client() {
        let parsed = parse_claude_product_invocation(&args(&[
            "perllsp", "doctor", "--client", "claude", "--json",
        ]));
        assert_eq!(
            parsed,
            Ok(Some(ClaudeProductInvocation {
                action: ClaudeProductAction::Doctor,
                json: true,
                host_test: false,
            }))
        );
    }

    #[test]
    fn unrelated_existing_doctor_surface_falls_through() {
        assert_eq!(parse_claude_product_invocation(&args(&["perllsp", "doctor"])), Ok(None));
    }

    #[test]
    fn doctor_positional_form_normalizes_to_shared_flag() {
        let normalized =
            super::normalize_doctor_invocation(args(&["perllsp", "doctor", "--external-tools"]));
        assert_eq!(normalized, args(&["perllsp", "--doctor", "--external-tools"]));
    }

    #[test]
    fn non_doctor_invocations_are_unchanged() {
        let input = args(&["perllsp", "--stdio"]);
        assert_eq!(super::normalize_doctor_invocation(input.clone()), input);
        let input = args(&["perllsp", "doctora"]);
        assert_eq!(super::normalize_doctor_invocation(input.clone()), input);
    }

    #[test]
    fn setup_does_not_accept_an_implicit_or_other_client() {
        assert!(parse_claude_product_invocation(&args(&["perllsp", "setup"])).is_err());
        assert!(parse_claude_product_invocation(&args(&["perllsp", "setup", "other"])).is_err());
    }

    #[test]
    fn host_test_is_opt_in_on_explicit_claude_doctor() {
        let parsed = parse_claude_product_invocation(&args(&[
            "perllsp",
            "doctor",
            "--client",
            "claude",
            "--host-test",
            "--json",
        ]));
        assert_eq!(
            parsed,
            Ok(Some(ClaudeProductInvocation {
                action: ClaudeProductAction::Doctor,
                json: true,
                host_test: true,
            }))
        );
    }

    #[test]
    fn host_test_is_not_selected_by_ordinary_doctor_or_setup() {
        assert_eq!(parse_claude_product_invocation(&args(&["perllsp", "doctor"])), Ok(None));
        let setup =
            parse_claude_product_invocation(&args(&["perllsp", "setup", "claude", "--host-test"]));
        assert!(setup.is_err(), "setup must not admit --host-test: {setup:?}");
    }

    #[test]
    fn host_test_rejects_hostile_client_authority() {
        for flag in [
            "--prompt",
            "--command",
            "--workspace",
            "--env",
            "--credential",
            "--token",
            "--executable",
            "--retry",
            "--timeout",
            "--profile",
            "--count",
        ] {
            let parsed = parse_claude_product_invocation(&args(&[
                "perllsp",
                "doctor",
                "--client",
                "claude",
                "--host-test",
                flag,
            ]));
            assert!(parsed.is_err(), "expected {flag} to be rejected, got {parsed:?}");
        }
    }
}
