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
    // Validate the spec's HOST:PORT shape before any connect attempt (#16556):
    // the transport's own failure for a malformed spec is a raw "invalid socket
    // address" that never names the expected format.
    let (peer_host, peer_port) = parse_peer_connect_spec(peer_addr).map_err(anyhow::Error::msg)?;
    tracing::info!(peer = peer_addr, peer_host = %peer_host, peer_port, "Starting external-peer DAP session on stdio");
    let backend = ExternalDebuggerPeerBackend::connect(peer_addr, EXTERNAL_PEER_TIMEOUT)
        .map_err(|e| anyhow::anyhow!("failed to connect to debugger peer {peer_addr}: {e}"))?;
    let bridge = DapPeerBridge::new(Box::new(backend));
    run_external_peer_session_stdio(bridge, EXTERNAL_PEER_POLL)?;
    Ok(())
}

/// Parse a `HOST:PORT` dial spec for `--external-peer` into host and port.
///
/// The whole spec is later handed to the peer transport, whose own failure for
/// a malformed spec is the raw "invalid socket address" — it never names the
/// expected shape (#16556). Validating here turns a typo'd rendezvous into a
/// startup error that names `HOST:PORT` instead. The host may be a hostname or
/// IPv4 address with no whitespace (bracketed IPv6 literals like `[::1]` are
/// accepted); the port must be numeric.
///
/// # Errors
/// Names the offending spec and the expected `HOST:PORT` format.
fn parse_peer_connect_spec(spec: &str) -> Result<(String, u16), String> {
    let spec = spec.trim();
    let expected = "expected HOST:PORT where HOST is a hostname or IP literal with no \
                    whitespace or unbracketed colons and PORT is numeric 0-65535 \
                    (for example 127.0.0.1:13604)";
    let reject =
        |reason: &str| format!("invalid --external-peer spec '{spec}': {reason}; {expected}");
    let Some((host, port)) = spec.rsplit_once(':') else {
        return Err(reject("missing ':' separator"));
    };
    if host.is_empty() {
        return Err(reject("host is empty"));
    }
    if host.chars().any(char::is_whitespace) {
        return Err(reject("host contains whitespace"));
    }
    let bracketed_ipv6 = host.starts_with('[') && host.ends_with(']');
    if host.contains(':') && !bracketed_ipv6 {
        return Err(reject("host contains unbracketed colons"));
    }
    match port.trim().parse::<u16>() {
        Ok(port) => Ok((host.to_owned(), port)),
        Err(_) => Err(reject("port is not a number in 0..=65535")),
    }
}

/// Parse a `HOST` or `HOST:PORT` bind spec for `--external-peer-listen`.
///
/// A bare host (or empty string) binds an ephemeral loopback port (`port = 0`).
/// A `HOST:PORT` binds the given port. An explicit-but-unparseable port is a
/// startup error: silently falling back to `0` would move the rendezvous
/// contract the peer must dial, and the session would then time out with no
/// explanation of why the peer never arrived (#16556).
///
/// # Errors
/// Names the offending spec and the expected `HOST[:PORT]` format.
fn parse_listen_bind(spec: &str) -> Result<(String, u16), String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Ok(("127.0.0.1".to_string(), 0));
    }
    match spec.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => match port.trim().parse::<u16>() {
            Ok(port) => Ok((host.to_string(), port)),
            Err(_) => Err(format!(
                "invalid --external-peer-listen spec '{spec}': PORT must be numeric 0-65535 \
                 when given; expected HOST[:PORT], where a bare HOST or empty value binds an \
                 ephemeral port (for example 127.0.0.1:13604)"
            )),
        },
        _ => Ok((spec.to_string(), 0)),
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
    let (host, port) = parse_listen_bind(spec).map_err(anyhow::Error::msg)?;
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
/// program text.
///
/// An unreadable program is a hard error, not a degenerate emit (#16553): both
/// one-shot consumers render program-specific setup from `source_facts`, so a
/// silently emitted packet (`"source_facts": {}`, no program setup) would look
/// complete while missing every program-specific fact, and the exit 0 would
/// hide the typo from scripts and Makefiles. The underlying read error is
/// preserved so a bad path is diagnosable.
///
/// # Errors
/// Fails when `program` cannot be read, naming the path and the OS error.
fn build_session_packet(program: &Path) -> anyhow::Result<DebugSessionPacket> {
    let mut builder = DebugSessionPlanBuilder::new(program);
    let text = std::fs::read_to_string(program)
        .map_err(|e| anyhow::anyhow!("program '{}' could not be read: {e}", program.display()))?;
    let source = DebugSource::from_path(program);
    builder = builder.source_facts_from_text(&source, &text);
    Ok(builder.build())
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

    /// Trusted root directory for workspace-bound launch authority (#8656).
    /// Repeatable. Every debugged `program` must live inside one trusted root
    /// unless `--allow-unbounded` is set. Mutually exclusive with
    /// `--allow-unbounded`.
    #[arg(long = "trusted-root", value_name = "DIR")]
    trusted_root: Vec<PathBuf>,

    /// Explicitly allow unbounded launch paths (#8656). This is a user-owned
    /// acknowledgement recorded in the session authority receipt; launch
    /// arguments and opened-project data can never set it. Mutually exclusive
    /// with `--trusted-root`.
    #[arg(long = "allow-unbounded", default_value_t = false)]
    allow_unbounded: bool,

    /// Operator note recorded with `--allow-unbounded` in the authority
    /// receipt.
    #[arg(
        long = "unbounded-note",
        value_name = "TEXT",
        default_value = "operator enabled via CLI"
    )]
    unbounded_note: String,

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
        let packet = build_session_packet(program)
            .map_err(|e| anyhow::anyhow!("--ptkdb-bootstrap-rc: {e}"))?;
        write!(std::io::stdout(), "{}", render_ptkdbrc(&packet, true))?;
        return Ok(());
    }
    if let Some(program) = args.debug_session_plan.as_deref() {
        let packet = build_session_packet(program)
            .map_err(|e| anyhow::anyhow!("--debug-session-plan: {e}"))?;
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
    //
    // The launch-authority flags are user/machine-owned startup inputs
    // (#8656): a server started with neither `--trusted-root` nor
    // `--allow-unbounded` fails closed in `DapServer::new` before any
    // debuggee process can spawn.
    let launch_authority = perl_dap::LaunchAuthorityStartup {
        trusted_roots: args.trusted_root.clone(),
        allow_unbounded: args.allow_unbounded.then(|| {
            perl_dap::UnboundedAcknowledgement::new(
                perl_dap::LaunchAuthoritySource::CommandLine,
                args.unbounded_note.clone(),
            )
        }),
    };
    let config = DapConfig {
        log_level: args.log_level,
        mode: DapMode::Native,
        workspace_root: None,
        launch_authority,
    };

    let mut server = DapServer::new(config)?;

    tracing::info!("Starting DAP server on stdio");
    server.run()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Args, DEFAULT_DAP_PORT, build_session_packet, editor_socket_retired,
        native_editor_socket_retired, parse_listen_bind, parse_peer_connect_spec,
        resolve_socket_port, windows_shell_quote,
    };
    use clap::{CommandFactory, Parser};
    use perl_lsp_rs_core::product_identity::{
        BinaryIdentityPacketV1, BinaryRole, IdentityOutputFormat, requested_identity_output,
    };
    use perl_test_must::{must, must_err_with, must_with};
    use std::path::Path;

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

    #[test]
    fn one_shot_packet_refuses_an_unreadable_program_with_a_named_path() {
        let missing = Path::new("./no-such-dir/no-such-16553.pl");
        let error = must_err_with(
            build_session_packet(missing),
            "an unreadable program must fail the one-shot emit instead of exiting 0 (#16553)",
        );
        let message = error.to_string();
        assert!(message.contains("could not be read"), "{message}");
        assert!(message.contains("no-such-16553.pl"), "{message}");
    }

    #[test]
    fn one_shot_packet_still_attaches_source_facts_for_a_readable_program() {
        let dir = must_with(tempfile::tempdir(), "test fixture tempdir must be created");
        let program = dir.path().join("prog.pl");
        must_with(
            std::fs::write(&program, "sub run {\n    my $x = 1;\n    return $x;\n}\n"),
            "test fixture program must be written",
        );
        let packet =
            must_with(build_session_packet(&program), "a readable program must build a packet");
        assert!(!packet.source_facts.is_empty(), "a readable program must keep its source facts");
    }

    #[test]
    fn listen_ephemeral_fallback_forms_survive_spec_validation() {
        assert_eq!(must(parse_listen_bind("")), ("127.0.0.1".to_owned(), 0));
        assert_eq!(must(parse_listen_bind("localhost")), ("localhost".to_owned(), 0));
        assert_eq!(must(parse_listen_bind("127.0.0.1:5000")), ("127.0.0.1".to_owned(), 5000));
    }

    #[test]
    fn listen_explicit_unparseable_port_is_a_startup_error_naming_the_format() {
        for spec in ["127.0.0.1:notaport", "127.0.0.1:1364O", "127.0.0.1:", "127.0.0.1:70000"] {
            let error = must_err_with(
                parse_listen_bind(spec),
                "an explicit-but-unparseable port must fail startup instead of binding ephemeral (#16556)",
            );
            assert!(error.contains("HOST[:PORT]"), "{error}");
            assert!(error.contains(spec), "{error}");
        }
    }

    #[test]
    fn peer_connect_spec_accepts_documented_host_port_forms() {
        assert_eq!(
            must(parse_peer_connect_spec("127.0.0.1:13604")),
            ("127.0.0.1".to_owned(), 13_604)
        );
        assert_eq!(must(parse_peer_connect_spec("localhost:5000")), ("localhost".to_owned(), 5000));
        assert_eq!(
            must(parse_peer_connect_spec("  localhost:5000  ")),
            ("localhost".to_owned(), 5000)
        );
        assert_eq!(must(parse_peer_connect_spec("[::1]:5000")), ("[::1]".to_owned(), 5000));
    }

    #[test]
    fn peer_connect_spec_malformed_specs_error_naming_host_port_format() {
        for spec in [
            "no-colon-thing",
            ":5000",
            "host name:5000",
            "a:b:5000",
            "127.0.0.1:notaport",
            "127.0.0.1:",
            "127.0.0.1:70000",
        ] {
            let error = must_err_with(
                parse_peer_connect_spec(spec),
                "a malformed spec must fail startup before any connect attempt (#16556)",
            );
            assert!(error.contains("HOST:PORT"), "{error}");
            assert!(error.contains(spec), "{error}");
        }
    }
}
