//! DAP adapter entry point
//!
//! This binary provides the Debug Adapter Protocol server for Perl debugging.
//! It follows the TDD approach with comprehensive test scaffolding for 19 acceptance criteria.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use perl_dap::backend::capabilities::ControlMode;
use perl_dap::backend::external_peer::ExternalDebuggerPeerBackend;
use perl_dap::backend::peer_launch::{
    DEFAULT_LISTEN_HANDSHAKE_TIMEOUT, ENV_PEER_TOKEN, ExternalPeerLaunchConfig, PeerRendezvousMode,
    prepare_mirror_listen_session, run_mirror_listen_session_stdio,
};
use perl_dap::backend::{DapPeerBridge, run_external_peer_session_stdio};
use perl_dap::model::{DebugSessionPacket, DebugSource};
use perl_dap::ptkdb_bootstrap::render_ptkdbrc;
use perl_dap::session_plan::DebugSessionPlanBuilder;
use perl_dap::{DapConfig, DapMode, DapServer};
use perl_lsp_rs_core::product_identity::{
    BinaryIdentityPacketV1, IdentityOutputFormat, requested_identity_output,
};
use perl_lsp_rs_core::runtime::launcher::{init_logging, log_server_startup};

const DEFAULT_DAP_PORT: u16 = 13_603;

/// Native and external-peer editor TCP (`--socket` / editor `--port`) no longer
/// bind an editor listener (#10565, #10566).
///
/// The flags remain parsed because they are flattened from shared
/// `TransportArgs`. Every `perl-dap` use — native, `--external-peer`, and
/// `--external-peer-listen` — must fail before bind with a stdio migration.
/// Silently ignoring the flag would leave a client waiting on a port while this
/// process read stdin.
fn native_editor_socket_retired() -> anyhow::Error {
    anyhow::anyhow!(
        "native perl-dap editor TCP (--socket / --port) has been retired.\n\
         The adapter no longer binds an editor-facing listener.\n\
         \n\
         Use parent-owned stdio instead:\n\
         \n\
         \x20 perl-dap --stdio"
    )
}

/// Quote a user-supplied peer spec for a pasteable migration command.
///
/// The error is an invitation to paste `perl-dap --stdio --external-peer …`.
/// Unquoted whitespace or metacharacters would change argv boundaries.
fn shell_quote(value: &str) -> String {
    let bare = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '_' | '-' | '/'));
    if bare {
        value.to_owned()
    } else if cfg!(windows) {
        windows_shell_quote(value)
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

/// Quote a value for a `cmd.exe` command line.
///
/// The remediation is displayed to users on the host that will run it. POSIX
/// single quotes are literal characters to `cmd.exe`, so use its double-quoted
/// region convention instead. Percent signs remain single because this command
/// is intended for an interactive prompt, not a batch file.
fn windows_shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '%' => quoted.push('%'),
            '"' => quoted.push_str("\"\""),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

fn editor_socket_retired(
    external_peer: Option<&str>,
    external_peer_listen: Option<&str>,
) -> anyhow::Error {
    if let Some(peer_addr) = external_peer {
        let quoted = shell_quote(peer_addr);
        return anyhow::anyhow!(
            "perl-dap editor TCP (--socket / --port) has been retired.\n\
             External-peer modes expose DAP only through child stdio.\n\
             The adapter no longer binds an editor-facing listener.\n\
             \n\
             Use parent-owned stdio with the same debugger-peer backend:\n\
             \n\
             \x20 perl-dap --stdio --external-peer {quoted}"
        );
    }
    if let Some(spec) = external_peer_listen {
        let quoted = shell_quote(spec);
        return anyhow::anyhow!(
            "perl-dap editor TCP (--socket / --port) has been retired.\n\
             External-peer modes expose DAP only through child stdio.\n\
             The adapter no longer binds an editor-facing listener.\n\
             \n\
             Use parent-owned stdio with the same debugger-peer backend:\n\
             \n\
             \x20 perl-dap --stdio --external-peer-listen {quoted}"
        );
    }
    native_editor_socket_retired()
}

/// How long to wait for the external peer handshake / a session poll tick.
const EXTERNAL_PEER_TIMEOUT: Duration = Duration::from_secs(10);
const EXTERNAL_PEER_POLL: Duration = Duration::from_millis(50);

/// Run an external-peer DAP session over **stdio**: the editor spawns us as a
/// child process and drives DAP over our stdin/stdout, while we connect to the
/// running debugger peer at `peer_addr` and translate DAP ↔ the Perl Debugger
/// Peer Protocol.
fn run_external_peer_bridge_stdio(peer_addr: &str) -> anyhow::Result<()> {
    tracing::info!(peer = peer_addr, "Starting external-peer DAP session on stdio");
    let backend = ExternalDebuggerPeerBackend::connect(peer_addr, EXTERNAL_PEER_TIMEOUT)
        .map_err(|e| anyhow::anyhow!("failed to connect to debugger peer {peer_addr}: {e}"))?;
    let bridge = DapPeerBridge::new(Box::new(backend));
    run_external_peer_session_stdio(bridge, EXTERNAL_PEER_POLL)?;
    Ok(())
}

/// Parse a `HOST` or `HOST:PORT` bind spec for `--external-peer-listen`.
///
/// A bare host (or empty string) binds an ephemeral loopback port (`port = 0`).
/// A `HOST:PORT` binds the given port. An unparseable port falls back to `0`.
fn parse_listen_bind(spec: &str) -> (String, u16) {
    let spec = spec.trim();
    if spec.is_empty() {
        return ("127.0.0.1".to_string(), 0);
    }
    match spec.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => {
            (host.to_string(), port.trim().parse().unwrap_or(0))
        }
        _ => (spec.to_string(), 0),
    }
}

/// Run a mirror-mode external-peer **listen** session: bind a loopback listener
/// for a (future) debugger peer to connect back to, expose the env-var contract
/// the peer reads to find us, and serve DAP to the editor over stdio while
/// queueing breakpoints until the peer handshakes.
///
/// The peer process itself is out of scope for this wiring; this establishes the
/// host side of the mirror session, proven end-to-end against a fake peer in the
/// crate tests. The editor speaks DAP over stdio only.
fn run_external_peer_listen(spec: &str) -> anyhow::Result<()> {
    let (host, port) = parse_listen_bind(spec);
    let config = ExternalPeerLaunchConfig {
        mode: PeerRendezvousMode::Listen,
        control: ControlMode::Mirror,
        host,
        port,
        ..ExternalPeerLaunchConfig::default()
    };
    let (peer_listener, endpoint, bridge) = prepare_mirror_listen_session(&config)
        .map_err(|e| anyhow::anyhow!("failed to bind peer listener: {e}"))?;

    // Surface the env-var contract the (future) peer process reads to find and
    // authenticate to this host session. The token is a per-session bearer
    // secret, so it is masked in the log — logging it in cleartext would hand
    // any log reader the credential the handshake now enforces. Non-sensitive
    // keys (address, mode) are logged verbatim.
    for (key, value) in endpoint.env_vars() {
        let logged = if key == ENV_PEER_TOKEN { "<redacted>" } else { value.as_str() };
        tracing::info!(%key, value = %logged, "external-peer listen: peer env contract");
    }
    tracing::info!(
        peer_addr = %endpoint.addr,
        "external-peer listen: waiting for a debugger peer to connect back"
    );

    // The peer must present this token in its `peer/hello` to be accepted; the
    // acceptor rejects any handshake without a match, so the loopback bind is
    // not the sole access control.
    let expected_token = Some(endpoint.session_credential());

    run_mirror_listen_session_stdio(
        peer_listener,
        bridge,
        DEFAULT_LISTEN_HANDSHAKE_TIMEOUT,
        EXTERNAL_PEER_POLL,
        expected_token,
    )?;
    Ok(())
}

/// Build a debug-session packet for `program`, deriving source facts from the
/// program text when it is readable.
fn build_session_packet(program: &Path) -> DebugSessionPacket {
    let mut builder = DebugSessionPlanBuilder::new(program);
    if let Ok(text) = std::fs::read_to_string(program) {
        let source = DebugSource::from_path(program);
        builder = builder.source_facts_from_text(&source, &text);
    }
    builder.build()
}

fn resolve_socket_port(args: &perl_lsp_rs_core::runtime::launcher::TransportArgs) -> Option<u16> {
    if args.socket || args.port.is_some() {
        Some(args.port.unwrap_or(DEFAULT_DAP_PORT))
    } else {
        None
    }
}

fn write_runtime_identity(format: IdentityOutputFormat) -> anyhow::Result<()> {
    let packet = BinaryIdentityPacketV1::embedded_dap(env!("CARGO_PKG_VERSION"));
    match format {
        IdentityOutputFormat::Human => write!(std::io::stdout(), "{}", packet.to_human())?,
        IdentityOutputFormat::Json => writeln!(std::io::stdout(), "{}", packet.to_json()?)?,
    }
    Ok(())
}

/// Perl Debug Adapter Protocol server
#[derive(Parser, Debug)]
#[command(
    name = "perl-dap",
    version,
    about,
    long_about = None,
    after_help = "Editor TCP is retired. `perl-dap --socket` / `--port` — including \
with `--external-peer` / `--external-peer-listen` — fails before bind. Use \
`perl-dap --stdio`, `perl-dap --stdio --external-peer HOST:PORT`, or \
`perl-dap --stdio --external-peer-listen HOST[:PORT]`. Authenticated debugger-peer \
TCP remains a backend transport, not an editor listener."
)]
struct Args {
    #[command(flatten)]
    transport: perl_lsp_rs_core::runtime::launcher::TransportArgs,

    /// Logging level (error, warn, info, debug, trace)
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Emit a Devel::ptkdb `.ptkdbrc` bootstrap for PROGRAM to stdout and exit.
    #[arg(long, value_name = "PROGRAM")]
    ptkdb_bootstrap_rc: Option<PathBuf>,

    /// Emit a `perl-lsp-debug-session-v1` JSON session plan for PROGRAM to
    /// stdout and exit.
    #[arg(long, value_name = "PROGRAM")]
    debug_session_plan: Option<PathBuf>,

    /// Connect to an external debugger peer at HOST:PORT (e.g. Devel::ptkdb) and
    /// relay DAP <-> the Perl Debugger Peer Protocol: the editor drives DAP over
    /// stdio, we drive the peer. Editor `--socket` / `--port` fail before bind.
    #[arg(long, value_name = "HOST:PORT")]
    external_peer: Option<String>,

    /// Listen for a mirror-mode external debugger peer to connect back (the
    /// `mode: "listen"` external-peer launch). Binds a loopback debugger-peer
    /// listener (a bare HOST or empty value allocates an ephemeral port), exposes
    /// the PERL_DAP_PEER* env contract, and serves DAP to the editor over stdio
    /// — queueing breakpoints until the peer handshakes. Editor control is
    /// mirror-rejected. Editor `--socket` / `--port` fail before bind.
    #[arg(long, value_name = "HOST[:PORT]")]
    external_peer_listen: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let raw_args: Vec<String> = std::env::args().collect();
    if let Some(format) = requested_identity_output(&raw_args) {
        write_runtime_identity(format)?;
        return Ok(());
    }
    let args = Args::parse_from(raw_args);

    // One-shot emit surfaces: these do not start a server. Write directly to
    // the stdout handle (rather than the print!/println! macros) so the shipped
    // binary stays clear of the `clippy::print_stdout` restriction lint.
    if let Some(program) = args.ptkdb_bootstrap_rc.as_deref() {
        let packet = build_session_packet(program);
        write!(std::io::stdout(), "{}", render_ptkdbrc(&packet, true))?;
        return Ok(());
    }
    if let Some(program) = args.debug_session_plan.as_deref() {
        let packet = build_session_packet(program);
        writeln!(std::io::stdout(), "{}", serde_json::to_string_pretty(&packet)?)?;
        return Ok(());
    }

    // Every `--socket` / editor `--port` combination fails before bind and
    // before any "server starting" log that would claim a listener exists,
    // including when combined with `--external-peer` / `--external-peer-listen`.
    if resolve_socket_port(&args.transport).is_some() {
        return Err(if args.external_peer.is_some() || args.external_peer_listen.is_some() {
            editor_socket_retired(
                args.external_peer.as_deref(),
                args.external_peer_listen.as_deref(),
            )
        } else {
            native_editor_socket_retired()
        });
    }

    init_logging(&args.log_level);

    // External-peer session: drive an explicitly selected debugger engine over
    // the peer protocol while the editor speaks DAP over stdio.
    if let Some(peer_addr) = args.external_peer.as_deref() {
        log_server_startup(
            "perl-dap",
            env!("CARGO_PKG_VERSION"),
            args.transport.mode(),
            None,
            None,
        );
        return run_external_peer_bridge_stdio(peer_addr);
    }

    // External-peer LISTEN mode (mirror): bind the authenticated debugger-peer
    // listener and wait for the peer to connect back. Editor DAP stays on stdio.
    if let Some(spec) = args.external_peer_listen.as_deref() {
        log_server_startup(
            "perl-dap",
            env!("CARGO_PKG_VERSION"),
            args.transport.mode(),
            None,
            None,
        );
        return run_external_peer_listen(spec);
    }

    log_server_startup("perl-dap", env!("CARGO_PKG_VERSION"), args.transport.mode(), None, None);

    // The shipped binary always runs the native adapter. External
    // implementations may be compared in repository-only conformance tooling,
    // but no alternate DAP server is reachable from this CLI or crate runtime.
    let config =
        DapConfig { log_level: args.log_level, mode: DapMode::Native, workspace_root: None };

    let mut server = DapServer::new(config)?;

    tracing::info!("Starting DAP server on stdio");
    server.run()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Args, DEFAULT_DAP_PORT, editor_socket_retired, native_editor_socket_retired,
        resolve_socket_port, windows_shell_quote,
    };
    use clap::{CommandFactory, Parser};
    use perl_lsp_rs_core::product_identity::{
        BinaryIdentityPacketV1, BinaryRole, IdentityOutputFormat, requested_identity_output,
    };

    #[test]
    fn native_socket_flags_fail_with_stdio_migration_before_any_bind() {
        let rendered = native_editor_socket_retired().to_string();
        assert!(rendered.contains("perl-dap --stdio"));
        assert!(rendered.contains("--socket"));
        assert!(!rendered.contains("already in use"));
        assert!(!rendered.contains("127.0.0.1"));
        assert!(!rendered.contains("--external-peer"));
    }

    #[test]
    fn peer_socket_flags_fail_with_stdio_migration_and_preserve_peer_mode() {
        let connect = editor_socket_retired(Some("127.0.0.1:5000"), None).to_string();
        assert!(connect.contains("perl-dap --stdio --external-peer 127.0.0.1:5000"));
        assert!(!connect.contains("already in use"));

        let listen = editor_socket_retired(None, Some("127.0.0.1")).to_string();
        assert!(listen.contains("perl-dap --stdio --external-peer-listen 127.0.0.1"));
        assert!(!listen.contains("already in use"));
    }

    #[test]
    fn cli_help_has_no_pls_product_surface() {
        let help = Args::command().render_long_help().to_string();
        assert!(!help.contains("--bridge"));
        assert!(!help.contains("Perl::LanguageServer"));
        assert!(!help.contains("BridgeAdapter"));
        assert!(
            help.contains("Editor TCP is retired"),
            "perl-dap --help must classify editor --socket as retired: {help}"
        );
        assert!(
            help.contains("perl-dap --stdio"),
            "perl-dap --help must name the stdio migration: {help}"
        );
        assert!(
            !help.contains("add `--socket`"),
            "perl-dap --help must not advertise a peer editor socket wrapper: {help}"
        );
    }

    #[test]
    fn cli_rejects_removed_bridge_flag() {
        let result = Args::try_parse_from(["perl-dap", "--bridge"]);
        assert!(result.is_err());
    }

    #[test]
    fn dap_identity_flags_select_the_shared_packet_without_starting_clap() {
        let json_args = vec!["perl-dap".to_owned(), "--info".to_owned(), "--json".to_owned()];
        let human_args = vec!["perl-dap".to_owned(), "--identity".to_owned()];
        assert_eq!(requested_identity_output(&json_args), Some(IdentityOutputFormat::Json));
        assert_eq!(requested_identity_output(&human_args), Some(IdentityOutputFormat::Human));

        let packet = BinaryIdentityPacketV1::embedded_dap("0.18.0");
        assert_eq!(packet.binary.role, BinaryRole::Dap);
        assert_eq!(packet.binary.executable, "perl-dap");
    }

    #[test]
    fn socket_mode_uses_dap_default_port() {
        let args = perl_lsp_rs_core::runtime::launcher::TransportArgs {
            stdio: false,
            socket: true,
            port: None,
        };
        assert_eq!(resolve_socket_port(&args), Some(DEFAULT_DAP_PORT));
    }

    #[test]
    fn explicit_socket_port_is_preserved() {
        let args = perl_lsp_rs_core::runtime::launcher::TransportArgs {
            stdio: false,
            socket: true,
            port: Some(9_999),
        };
        assert_eq!(resolve_socket_port(&args), Some(9_999));
    }

    #[test]
    fn stdio_mode_does_not_resolve_a_socket_port() {
        let args = perl_lsp_rs_core::runtime::launcher::TransportArgs {
            stdio: true,
            socket: false,
            port: None,
        };
        assert_eq!(resolve_socket_port(&args), None);
    }

    #[test]
    fn fn_main_still_fails_socket_flags_via_native_editor_socket_retired() {
        // The inventory scan ratchets `native_editor_socket_retired` inside
        // `fn main`. Peer combinations share that fail-before-bind gate through
        // `editor_socket_retired`, which delegates to it for the native path.
        let source = include_str!("main.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(production.contains("native_editor_socket_retired"));
        assert!(production.contains("editor_socket_retired"));
        assert!(!production.contains("fn bind_editor_listener"));
    }

    #[test]
    fn peer_stdio_migration_quotes_metacharacter_specs() {
        let connect = editor_socket_retired(Some("host; touch /tmp/x"), None).to_string();
        let listen = editor_socket_retired(None, Some("a b; rm -rf /")).to_string();
        let expected_connect = if cfg!(windows) {
            "--external-peer \"host; touch /tmp/x\""
        } else {
            "--external-peer 'host; touch /tmp/x'"
        };
        let expected_listen = if cfg!(windows) {
            "--external-peer-listen \"a b; rm -rf /\""
        } else {
            "--external-peer-listen 'a b; rm -rf /'"
        };
        assert!(connect.contains(expected_connect), "{connect}");
        assert!(listen.contains(expected_listen), "{listen}");
        assert!(connect.contains("perl-dap --stdio --external-peer"), "{connect}");
        assert!(!connect.contains(&format!("{expected_connect} --socket")), "{connect}");
        assert!(!listen.contains(&format!("{expected_listen} --socket")), "{listen}");
    }

    #[test]
    fn windows_remediation_uses_cmd_quoting() {
        assert_eq!(windows_shell_quote("[::1]:13604"), "\"[::1]:13604\"");
        assert_eq!(windows_shell_quote("100% ready\"now"), "\"100% ready\"\"now\"");
    }
}
