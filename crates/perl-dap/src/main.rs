//! DAP adapter entry point
//!
//! This binary provides the Debug Adapter Protocol server for Perl debugging.
//! It follows the TDD approach with comprehensive test scaffolding for 19 acceptance criteria.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::Parser;
use perl_dap::backend::capabilities::ControlMode;
use perl_dap::backend::external_peer::ExternalDebuggerPeerBackend;
use perl_dap::backend::peer_launch::{
    DEFAULT_LISTEN_HANDSHAKE_TIMEOUT, ENV_PEER_TOKEN, ExternalPeerLaunchConfig, PeerRendezvousMode,
    prepare_mirror_listen_session, run_mirror_listen_session_socket,
    run_mirror_listen_session_stdio,
};
use perl_dap::backend::{
    DapPeerBridge, run_external_peer_session, run_external_peer_session_stdio,
};
use perl_dap::model::{DebugSessionPacket, DebugSource};
use perl_dap::ptkdb_bootstrap::render_ptkdbrc;
use perl_dap::session_plan::DebugSessionPlanBuilder;
use perl_dap::{DapConfig, DapMode, DapServer, DapSocketBindError};
use perl_lsp_rs_core::runtime::launcher::{init_logging, log_server_startup};

const DEFAULT_DAP_PORT: u16 = 13_603;

/// Quote a value for inclusion in a suggested shell command.
///
/// Peer specs come from the user's own command line and are normally a bare
/// `host:port`, but a suggested command is something the user is invited to
/// paste and run. An unquoted value containing a space or a shell
/// metacharacter would display one command and mean another.
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

/// Two alternative ports near the one that failed.
///
/// Near the top of the range `port + 10` wraps to 0 — which means "any port" —
/// and to the privileged low ports, so suggest downward there instead. A
/// remediation command that silently means "any port" is the same class of
/// dishonesty this module exists to remove.
fn suggested_alternative_ports(port: u16) -> (u16, u16) {
    if port > u16::MAX - 10 { (port - 1, port - 10) } else { (port + 1, port + 10) }
}

/// An editor-facing listener: how to name it, and how to re-launch it.
struct EditorListener {
    /// Names which listener failed, because `perl-dap` opens an editor-facing
    /// socket from three different paths and a user with more than one
    /// configured cannot otherwise tell which port to change.
    role: &'static str,
    /// The mode-selecting arguments that produced this listener.
    ///
    /// These are not decoration. Two of the three paths are selected by an
    /// option (`--external-peer`, `--external-peer-listen`); a remediation
    /// command that dropped it would start the *native* adapter on the
    /// suggested port. That command succeeds, so the user would reasonably
    /// believe the problem was fixed while silently getting a different
    /// debugger than the one they launched.
    mode_args: String,
}

impl EditorListener {
    /// The native adapter's `--socket` transport — the default socket path.
    fn native_socket() -> Self {
        Self { role: "DAP socket transport", mode_args: "--socket".to_owned() }
    }

    /// The editor side of the `--external-peer HOST:PORT` session.
    fn peer_bridge(peer_addr: &str) -> Self {
        Self {
            role: "editor-facing peer listener",
            mode_args: format!("--external-peer {} --socket", shell_quote(peer_addr)),
        }
    }

    /// The editor side of the `--external-peer-listen HOST[:PORT]` mirror session.
    fn peer_listen(spec: &str) -> Self {
        Self {
            role: "editor-facing listen-mode listener",
            mode_args: format!("--external-peer-listen {} --socket", shell_quote(spec)),
        }
    }
}

/// Explain a taken editor-facing port, naming which listener failed and how to
/// retry *that* listener.
///
/// `perl_lsp_rs_core::runtime::launcher::port_in_use_message` handles the same
/// failure for the language server, but it is not reusable here: its
/// remediation names `perllsp --socket --port N`, which is the wrong binary and
/// the wrong flags for a debug session. Telling someone whose debugger will not
/// start to run the language server is worse than saying nothing.
fn editor_port_in_use_message(port: u16, listener: &EditorListener) -> String {
    let (alt1, alt2) = suggested_alternative_ports(port);
    let (role, args) = (listener.role, &listener.mode_args);
    format!(
        "Port {port} is already in use, so perl-dap could not open its {role}.\n\
         Another debug session may still be running.\n\
         \n\
         Try a different port:\n\
         \n\
         \x20 perl-dap {args} --port {alt1}\n\
         \x20 perl-dap {args} --port {alt2}\n\
         \n\
         Or stop the process already using port {port}."
    )
}

/// Map a bind failure on an editor-facing listener to an actionable error.
///
/// The non-`AddrInUse` arm matters as much as the `AddrInUse` one: a bare `?`
/// also discards the port on permission-denied and address-not-available, which
/// are the other two ways this bind realistically fails.
fn describe_editor_bind_error(
    port: u16,
    listener: &EditorListener,
    error: &std::io::Error,
) -> anyhow::Error {
    if error.kind() == std::io::ErrorKind::AddrInUse {
        anyhow::anyhow!("{}", editor_port_in_use_message(port, listener))
    } else {
        let role = listener.role;
        anyhow::anyhow!("failed to bind the {role} on 127.0.0.1:{port}: {error}")
    }
}

/// Bind an editor-facing listener, reporting failures in terms the user can act on.
fn bind_editor_listener(
    port: u16,
    listener: &EditorListener,
) -> anyhow::Result<std::net::TcpListener> {
    std::net::TcpListener::bind(("127.0.0.1", port))
        .map_err(|error| describe_editor_bind_error(port, listener, &error))
}

/// Preserve the distinction between a native socket bind failure and a later
/// accepted-session failure while giving bind failures the same user-facing
/// context as the external-peer paths.
fn describe_native_socket_error(port: u16, error: anyhow::Error) -> anyhow::Error {
    match (error.downcast_ref::<DapSocketBindError>(), error.downcast_ref::<std::io::Error>()) {
        (Some(_), Some(source)) => {
            describe_editor_bind_error(port, &EditorListener::native_socket(), source)
        }
        _ => error,
    }
}

/// How long to wait for the external peer handshake / a session poll tick.
const EXTERNAL_PEER_TIMEOUT: Duration = Duration::from_secs(10);
const EXTERNAL_PEER_POLL: Duration = Duration::from_millis(50);

/// Run an external-peer DAP session: the editor connects to us on `editor_port`
/// (socket transport), we connect to the running debugger peer at `peer_addr`
/// (e.g. Devel::ptkdb), and translate DAP ↔ the Perl Debugger Peer Protocol.
fn run_external_peer_bridge(editor_port: u16, peer_addr: &str) -> anyhow::Result<()> {
    tracing::info!(port = editor_port, peer = peer_addr, "Starting external-peer DAP session");
    // Bind the editor-facing listener FIRST so the port is open (and the editor's
    // connect can queue in the listen backlog) while we connect to the peer, which
    // may take up to EXTERNAL_PEER_TIMEOUT. Otherwise an editor that spawns us and
    // immediately connects could fail before the port ever opened.
    let listener = bind_editor_listener(editor_port, &EditorListener::peer_bridge(peer_addr))?;
    let backend = ExternalDebuggerPeerBackend::connect(peer_addr, EXTERNAL_PEER_TIMEOUT)
        .map_err(|e| anyhow::anyhow!("failed to connect to debugger peer {peer_addr}: {e}"))?;
    let bridge = DapPeerBridge::new(Box::new(backend));

    // Bound the editor accept: a plain blocking `accept()` would hang the whole
    // process forever (holding the peer connection open, recoverable only by
    // SIGKILL) if the editor crashes before connecting or `--port` is
    // misconfigured. Poll non-blocking against a deadline, matching
    // `ExternalDebuggerPeerBackend::listen`.
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + EXTERNAL_PEER_TIMEOUT;
    let editor = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    anyhow::bail!("no editor connected within {EXTERNAL_PEER_TIMEOUT:?}");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(anyhow::anyhow!("editor accept failed: {e}")),
        }
    };
    // Restore blocking mode; the session driver manages its own read timeout.
    editor.set_nonblocking(false)?;
    run_external_peer_session(editor, bridge, EXTERNAL_PEER_POLL)?;
    Ok(())
}

/// Run an external-peer DAP session over **stdio**: the editor spawns us as a
/// child process and drives DAP over our stdin/stdout, while we connect to the
/// running debugger peer at `peer_addr` and translate DAP ↔ the Perl Debugger
/// Peer Protocol. This is the default transport when no `--socket`/`--port` is given.
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
/// the peer reads to find us, and serve DAP to the editor while queueing
/// breakpoints until the peer handshakes.
///
/// The peer process itself is out of scope for this wiring; this establishes the
/// host side of the mirror session, proven end-to-end against a fake peer in the
/// crate tests. The editor speaks DAP over stdio by default (add
/// `--socket`/`--port` for a socket editor connection).
fn run_external_peer_listen(spec: &str, editor_port: Option<u16>) -> anyhow::Result<()> {
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
    let expected_token = Some(endpoint.token.clone());

    match editor_port {
        Some(port) => {
            let editor_listener = bind_editor_listener(port, &EditorListener::peer_listen(spec))?;
            let (editor, _) = editor_listener.accept()?;
            run_mirror_listen_session_socket(
                editor,
                peer_listener,
                bridge,
                DEFAULT_LISTEN_HANDSHAKE_TIMEOUT,
                EXTERNAL_PEER_POLL,
                expected_token,
            )?;
        }
        None => {
            run_mirror_listen_session_stdio(
                peer_listener,
                bridge,
                DEFAULT_LISTEN_HANDSHAKE_TIMEOUT,
                EXTERNAL_PEER_POLL,
                expected_token,
            )?;
        }
    }
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

/// Perl Debug Adapter Protocol server
#[derive(Parser, Debug)]
#[command(name = "perl-dap", version, about, long_about = None)]
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
    /// relay DAP <-> the Perl Debugger Peer Protocol: the editor drives DAP, we
    /// drive the peer. Uses stdio by default (the editor spawns us); add
    /// `--socket`/`--port` for a socket editor connection instead.
    #[arg(long, value_name = "HOST:PORT")]
    external_peer: Option<String>,

    /// Listen for a mirror-mode external debugger peer to connect back (the
    /// `mode: "listen"` external-peer launch). Binds a loopback listener (a bare
    /// HOST or empty value allocates an ephemeral port), exposes the
    /// PERL_DAP_PEER* env contract, and serves DAP to the editor — queueing
    /// breakpoints until the peer handshakes. Editor control is mirror-rejected.
    /// Uses stdio by default; add `--socket`/`--port` for a socket editor link.
    #[arg(long, value_name = "HOST[:PORT]")]
    external_peer_listen: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

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

    init_logging(&args.log_level);
    log_server_startup("perl-dap", env!("CARGO_PKG_VERSION"), args.transport.mode(), None, None);

    // External-peer session: drive an explicitly selected debugger engine over
    // the peer protocol while the editor speaks DAP. The native default is unchanged.
    if let Some(peer_addr) = args.external_peer.as_deref() {
        return match resolve_socket_port(&args.transport) {
            Some(port) => run_external_peer_bridge(port, peer_addr),
            None => run_external_peer_bridge_stdio(peer_addr),
        };
    }

    // External-peer LISTEN mode (mirror): bind and wait for the peer to connect
    // back. The native default remains unchanged.
    if let Some(spec) = args.external_peer_listen.as_deref() {
        return run_external_peer_listen(spec, resolve_socket_port(&args.transport));
    }

    // The shipped binary always runs the native adapter. External
    // implementations may be compared in repository-only conformance tooling,
    // but no alternate DAP server is reachable from this CLI or crate runtime.
    let config =
        DapConfig { log_level: args.log_level, mode: DapMode::Native, workspace_root: None };

    let mut server = DapServer::new(config)?;

    if let Some(port) = resolve_socket_port(&args.transport) {
        tracing::info!("Starting DAP server on port {}", port);
        // `run_socket` marks the bind operation before accepting a client, so
        // every bind failure can be contextualized without relabelling later
        // session I/O failures.
        server.run_socket(port).map_err(|error| describe_native_socket_error(port, error))?;
        return Ok(());
    }

    tracing::info!("Starting DAP server on stdio");
    server.run()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Args, DEFAULT_DAP_PORT, DapSocketBindError, EditorListener, bind_editor_listener,
        describe_editor_bind_error, describe_native_socket_error, editor_port_in_use_message,
        resolve_socket_port, suggested_alternative_ports, windows_shell_quote,
    };
    use anyhow::Context as _;
    use clap::{CommandFactory, Parser};

    /// A taken port must produce an actionable message, not `os error 98`.
    #[test]
    fn taken_editor_port_is_explained_not_dumped() -> Result<(), Box<dyn std::error::Error>> {
        let occupied = std::net::TcpListener::bind(("127.0.0.1", 0))?;
        let port = occupied.local_addr()?.port();

        let Err(error) = bind_editor_listener(port, &EditorListener::native_socket()) else {
            return Err("binding an already-bound port must fail".into());
        };
        let rendered = error.to_string();

        assert!(rendered.contains(&port.to_string()), "must name the port: {rendered}");
        assert!(rendered.contains("DAP socket transport"));
        assert!(rendered.contains("perl-dap --socket --port"));
        assert!(!rendered.contains("perllsp"));
        assert!(!rendered.contains("os error"));
        Ok(())
    }

    #[test]
    fn remediation_preserves_the_selected_peer_mode() {
        let peer =
            editor_port_in_use_message(9000, &EditorListener::peer_bridge("127.0.0.1:5000"));
        assert!(peer.contains("perl-dap --external-peer 127.0.0.1:5000 --socket --port 9001"));

        let listen = editor_port_in_use_message(9000, &EditorListener::peer_listen("127.0.0.1"));
        assert!(listen.contains("perl-dap --external-peer-listen 127.0.0.1 --socket --port 9001"));

        let native = editor_port_in_use_message(9000, &EditorListener::native_socket());
        assert!(!native.contains("--external-peer"));
    }

    #[test]
    fn remediation_quotes_a_spec_that_needs_it() {
        let message =
            editor_port_in_use_message(9000, &EditorListener::peer_listen("a b; rm -rf /"));
        let expected = if cfg!(windows) {
            "--external-peer-listen \"a b; rm -rf /\""
        } else {
            "--external-peer-listen 'a b; rm -rf /'"
        };
        assert!(message.contains(expected));
    }

    #[test]
    fn windows_remediation_uses_cmd_quoting() {
        assert_eq!(windows_shell_quote("[::1]:13604"), "\"[::1]:13604\"");
        assert_eq!(windows_shell_quote("100% ready\"now"), "\"100% ready\"\"now\"");
    }

    #[test]
    fn other_bind_failures_still_name_the_port() {
        let error = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let rendered = describe_editor_bind_error(13_603, &EditorListener::native_socket(), &error)
            .to_string();
        assert!(rendered.contains("13603"));
        assert!(rendered.contains("DAP socket transport"));
        assert!(!rendered.contains("already in use"));
    }

    #[test]
    fn native_bind_failures_are_contextualized_without_touching_session_errors() {
        let bind_error = anyhow::Error::new(DapSocketBindError { port: 13_603 })
            .context(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        let rendered = describe_native_socket_error(13_603, bind_error).to_string();
        assert!(rendered.contains("13603"));
        assert!(rendered.contains("DAP socket transport"));
        assert!(!rendered.contains("os error"));

        let session_error = anyhow::anyhow!("accepted-session failure");
        assert_eq!(
            describe_native_socket_error(13_603, session_error).to_string(),
            "accepted-session failure"
        );
    }

    #[test]
    fn peer_bind_failures_name_their_distinct_roles() {
        let error = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        for (listener, expected_role) in [
            (EditorListener::peer_bridge("127.0.0.1:5000"), "editor-facing peer listener"),
            (EditorListener::peer_listen("127.0.0.1"), "editor-facing listen-mode listener"),
        ] {
            let rendered = describe_editor_bind_error(13_603, &listener, &error).to_string();
            assert!(rendered.contains(expected_role), "missing {expected_role}: {rendered}");
        }
    }

    #[test]
    fn suggested_ports_differ_from_the_failed_one() {
        let message =
            editor_port_in_use_message(DEFAULT_DAP_PORT, &EditorListener::native_socket());
        assert!(message.contains(&format!("perl-dap --socket --port {}", DEFAULT_DAP_PORT + 1)));
        assert!(message.contains(&format!("perl-dap --socket --port {}", DEFAULT_DAP_PORT + 10)));
    }

    #[test]
    fn suggested_ports_stay_usable_near_the_range_top() {
        for port in [u16::MAX, u16::MAX - 1, u16::MAX - 9] {
            let (alt1, alt2) = suggested_alternative_ports(port);
            for alt in [alt1, alt2] {
                assert!(alt >= 1024, "port {port} suggested unusable {alt}");
                assert_ne!(alt, port, "port {port} suggested itself");
            }
        }
    }

    #[test]
    fn cli_help_has_no_pls_product_surface() {
        let help = Args::command().render_long_help().to_string();
        assert!(!help.contains("--bridge"));
        assert!(!help.contains("Perl::LanguageServer"));
        assert!(!help.contains("BridgeAdapter"));
    }

    #[test]
    fn cli_rejects_removed_bridge_flag() {
        let result = Args::try_parse_from(["perl-dap", "--bridge"]);
        assert!(result.is_err());
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
}
