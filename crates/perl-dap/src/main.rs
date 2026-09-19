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
use perl_dap::security::{UnboundedGrant, WorkspaceAuthority};
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

    /// Confine debug launches to DIR. Repeat for a multi-root workspace.
    ///
    /// This is the adapter's trusted startup authority: launch arguments may
    /// narrow it but can never widen it, and neither the debugged program's own
    /// directory nor its `cwd` can establish it.
    #[arg(long, value_name = "DIR")]
    workspace_root: Vec<PathBuf>,

    /// Allow debug launches anywhere on this machine.
    ///
    /// Single-file debugging outside an opened workspace is legitimate, but it
    /// must be the operator's deliberate choice rather than the silent
    /// consequence of an unconfigured adapter. Conflicts with
    /// `--workspace-root`.
    #[arg(long)]
    allow_unbounded_workspace: bool,
}

/// Refuse workspace-authority flags in modes that cannot honour them.
///
/// Returns `None` when the combination is fine, so `main` reads as a guard
/// rather than a branch on two unrelated booleans.
fn workspace_flags_unsupported_in_peer_mode(args: &Args) -> Option<anyhow::Error> {
    let peer_mode = if args.external_peer.is_some() {
        "--external-peer"
    } else if args.external_peer_listen.is_some() {
        "--external-peer-listen"
    } else {
        return None;
    };

    let workspace_flag = if !args.workspace_root.is_empty() {
        "--workspace-root"
    } else if args.allow_unbounded_workspace {
        "--allow-unbounded-workspace"
    } else {
        return None;
    };

    Some(anyhow::anyhow!(
        "{workspace_flag} is not supported with {peer_mode}.\n\
         Workspace confinement applies to the native launch path, which resolves and \
         spawns the program itself.\n\
         In external-peer mode the selected debugger engine owns the runtime, so the \
         adapter cannot confine what it launches.\n\
         \n\
         Drop {workspace_flag}, or run the native adapter:\n\
         \n\
         \x20 perl-dap --stdio {workspace_flag}{}",
        if workspace_flag == "--workspace-root" { " <DIR>" } else { "" }
    ))
}

/// Build the server configuration the shipped binary runs under.
///
/// Extracted from `main` so the wiring itself is provable. The defect this
/// crate's `--workspace-root` work exists to fix was precisely that
/// `DapConfig`'s workspace field was hardcoded and no flag reached it: a test
/// that only checks `clap` parsing and `WorkspaceAuthority::from_startup`
/// passes just as happily when `main` drops the authority on the floor. Asserting
/// on the returned config is what closes that gap.
///
/// The shipped binary always runs the native adapter. External implementations
/// may be compared in repository-only conformance tooling, but no alternate DAP
/// server is reachable from this CLI or crate runtime.
///
/// # Errors
///
/// Propagates [`WorkspaceAuthority::from_startup`]'s refusal of a contradictory
/// or unusable workspace configuration, so a bad configuration fails startup
/// rather than serving requests under an authority the operator did not ask for.
fn build_config(args: &Args) -> anyhow::Result<DapConfig> {
    // Establish the trust boundary before the server exists.
    let workspace_authority =
        WorkspaceAuthority::from_startup(&args.workspace_root, args.allow_unbounded_workspace)?;

    Ok(DapConfig { log_level: args.log_level.clone(), mode: DapMode::Native, workspace_authority })
}

/// How the startup log should report an authority.
///
/// Split out from the emit so the classification is provable. The operator-facing
/// signal is half of what naming the unconfigured state buys: an adapter that is
/// silently unconfined looks exactly like a confined one in a log that does not
/// say so. Asserting on `tracing` output is awkward and brittle, but the decision
/// this enum records is the part that can be wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthorityLogRecord {
    /// Confined to `roots` trusted directories — informational.
    Confined { roots: usize },
    /// Unconfined because the operator asked for it — a warning.
    OperatorGrant,
    /// Unconfined because nothing was configured — a warning naming the remedy.
    Unconfigured,
}

/// Classify an authority for the startup log.
fn classify_workspace_authority(authority: &WorkspaceAuthority) -> AuthorityLogRecord {
    match authority.unbounded_grant() {
        None => AuthorityLogRecord::Confined { roots: authority.trusted_roots().len() },
        Some(UnboundedGrant::OperatorFlag) => AuthorityLogRecord::OperatorGrant,
        Some(UnboundedGrant::UnconfiguredDefault) => AuthorityLogRecord::Unconfigured,
    }
}

/// Record the launch authority this process is running under.
///
/// The unconfigured case is a warning, not an info line: it is the state
/// #8145 exists to remove, and it is indistinguishable from the bounded case in
/// a log that does not say so.
fn log_workspace_authority(authority: &WorkspaceAuthority) {
    // Dispatch on the classification the tests assert on. Emitting from a second
    // `match` over the authority would let the two drift, and the test would then
    // be proving something the shipped binary does not do.
    match classify_workspace_authority(authority) {
        AuthorityLogRecord::Confined { roots } => {
            tracing::info!(
                target = "perl_dap.security",
                mode = authority.mode_identity(),
                roots,
                "Debug launches are confined to the configured workspace roots"
            );
        }
        AuthorityLogRecord::OperatorGrant => {
            tracing::warn!(
                target = "perl_dap.security",
                mode = authority.mode_identity(),
                grant = UnboundedGrant::OperatorFlag.as_str(),
                "--allow-unbounded-workspace was passed: debug launches are not confined \
                 to any workspace root"
            );
        }
        AuthorityLogRecord::Unconfigured => {
            tracing::warn!(
                target = "perl_dap.security",
                mode = authority.mode_identity(),
                grant = UnboundedGrant::UnconfiguredDefault.as_str(),
                "No workspace authority configured: debug launches are not confined to any \
                 workspace root. Pass --workspace-root <DIR> to confine them, or \
                 --allow-unbounded-workspace to make this explicit."
            );
        }
    }
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

    // Workspace authority governs the *native* launch path, which is the only
    // path that resolves a `program` and spawns it. External-peer modes hand
    // runtime control to a separately selected debugger engine and never build a
    // `DapConfig`, so these flags would be silently ignored — and a security
    // flag that silently does nothing is worse than no flag: an operator who
    // passes `--workspace-root` would believe launches are confined when nothing
    // is confining them. Refuse before any session starts, like the retired
    // editor-socket flags above.
    if let Some(error) = workspace_flags_unsupported_in_peer_mode(&args) {
        return Err(error);
    }

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

    let config = build_config(&args)?;
    log_workspace_authority(&config.workspace_authority);

    let mut server = DapServer::new(config)?;

    tracing::info!("Starting DAP server on stdio");
    server.run()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Args, AuthorityLogRecord, DEFAULT_DAP_PORT, UnboundedGrant, WorkspaceAuthority,
        build_config, classify_workspace_authority, editor_socket_retired,
        native_editor_socket_retired, resolve_socket_port, windows_shell_quote,
        workspace_flags_unsupported_in_peer_mode,
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

    // --- workspace launch authority (#14587) ---

    /// The shipped binary can actually reach workspace-bound mode.
    ///
    /// Before this flag existed, `DapConfig.workspace_root` was hardcoded to
    /// `None` in `main`, so the bounded launch path was unreachable in the
    /// product and every real session ran unbounded.
    ///
    /// This asserts on the `DapConfig` the binary actually hands to
    /// `DapServer::new`, not on `WorkspaceAuthority::from_startup` in isolation:
    /// parsing the flag correctly and building the authority correctly prove
    /// nothing if `main` then discards it, which is the exact shape of the
    /// defect being fixed.
    #[test]
    fn workspace_root_flag_establishes_bounded_authority() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp = tempfile::tempdir()?;
        let alpha = temp.path().join("alpha");
        let beta = temp.path().join("beta");
        std::fs::create_dir_all(&alpha)?;
        std::fs::create_dir_all(&beta)?;

        let args = Args::parse_from([
            "perl-dap",
            "--stdio",
            "--workspace-root",
            alpha.to_str().ok_or("non-utf8 fixture path")?,
            "--workspace-root",
            beta.to_str().ok_or("non-utf8 fixture path")?,
        ]);
        assert_eq!(args.workspace_root.len(), 2, "the flag must be repeatable for multi-root");

        let config = build_config(&args)?;
        assert!(
            config.workspace_authority.is_bounded(),
            "--workspace-root must reach the config the server is built from"
        );
        assert_eq!(config.workspace_authority.trusted_roots().len(), 2);
        Ok(())
    }

    /// An unconfigured adapter is unbounded, but nameably so.
    #[test]
    fn no_authority_flags_resolve_to_the_named_legacy_default()
    -> Result<(), Box<dyn std::error::Error>> {
        let args = Args::parse_from(["perl-dap", "--stdio"]);
        assert!(args.workspace_root.is_empty());
        assert!(!args.allow_unbounded_workspace);

        let config = build_config(&args)?;
        assert_eq!(
            config.workspace_authority.unbounded_grant(),
            Some(UnboundedGrant::UnconfiguredDefault)
        );
        assert!(!config.workspace_authority.is_bounded());
        Ok(())
    }

    /// The operator's explicit opt-in is distinguishable from the default.
    #[test]
    fn allow_unbounded_flag_records_an_operator_grant() -> Result<(), Box<dyn std::error::Error>> {
        let args = Args::parse_from(["perl-dap", "--stdio", "--allow-unbounded-workspace"]);
        assert!(args.allow_unbounded_workspace);

        let config = build_config(&args)?;
        assert_eq!(
            config.workspace_authority.unbounded_grant(),
            Some(UnboundedGrant::OperatorFlag)
        );
        assert!(!config.workspace_authority.is_bounded());
        Ok(())
    }

    /// Asking to confine and to unconfine at once is a startup error, not a
    /// silent precedence rule.
    #[test]
    fn contradictory_authority_flags_fail_startup() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("ws");
        std::fs::create_dir_all(&root)?;

        let args = Args::parse_from([
            "perl-dap",
            "--stdio",
            "--workspace-root",
            root.to_str().ok_or("non-utf8 fixture path")?,
            "--allow-unbounded-workspace",
        ]);
        assert!(build_config(&args).is_err(), "both flags together must fail startup");
        Ok(())
    }

    /// A workspace root that does not exist fails startup rather than silently
    /// degrading the adapter to unbounded.
    #[test]
    fn a_nonexistent_workspace_root_fails_startup() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let missing = temp.path().join("absent");

        let args = Args::parse_from([
            "perl-dap",
            "--stdio",
            "--workspace-root",
            missing.to_str().ok_or("non-utf8 fixture path")?,
        ]);
        assert!(build_config(&args).is_err(), "a missing root must fail startup");
        Ok(())
    }

    /// The startup log distinguishes all three authority states.
    ///
    /// An operator reads this line to learn whether launches are confined. If
    /// the unconfigured default were reported like the bounded case, the state
    /// #8145 exists to remove would be invisible — which is the whole reason
    /// `UnconfiguredDefault` is a named variant rather than a silent `None`.
    #[test]
    fn the_startup_log_distinguishes_every_authority_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let alpha = temp.path().join("alpha");
        let beta = temp.path().join("beta");
        std::fs::create_dir_all(&alpha)?;
        std::fs::create_dir_all(&beta)?;

        let bound = WorkspaceAuthority::from_startup(&[alpha, beta], false)?;
        assert_eq!(
            classify_workspace_authority(&bound),
            AuthorityLogRecord::Confined { roots: 2 },
            "a bounded adapter must report how many roots confine it"
        );

        let operator = WorkspaceAuthority::from_startup(&[], true)?;
        assert_eq!(classify_workspace_authority(&operator), AuthorityLogRecord::OperatorGrant);

        let legacy = WorkspaceAuthority::unconfigured();
        assert_eq!(classify_workspace_authority(&legacy), AuthorityLogRecord::Unconfigured);

        // The two unconfined states must not collapse: one is a deliberate
        // operator choice, the other is the gap #8145 tracks.
        assert_ne!(
            classify_workspace_authority(&operator),
            classify_workspace_authority(&legacy),
            "an operator grant and the legacy default must be distinguishable in the log"
        );
        Ok(())
    }

    /// Workspace flags are refused in modes that cannot honour them.
    ///
    /// Both external-peer paths return from `main` before `build_config` runs,
    /// so these flags would otherwise be silently ignored — including the
    /// contradictory-flags refusal. An operator who passes `--workspace-root`
    /// and gets a running adapter would reasonably believe launches are
    /// confined; in peer mode the selected debugger engine owns the runtime and
    /// nothing is confining them.
    #[test]
    fn workspace_flags_are_refused_in_external_peer_modes() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("ws");
        std::fs::create_dir_all(&root)?;
        let root = root.to_str().ok_or("non-utf8 fixture path")?;

        for (peer_flag, peer_value) in
            [("--external-peer", "127.0.0.1:9000"), ("--external-peer-listen", "127.0.0.1:0")]
        {
            for workspace in [vec!["--workspace-root", root], vec!["--allow-unbounded-workspace"]] {
                let mut argv = vec!["perl-dap", "--stdio", peer_flag, peer_value];
                argv.extend(workspace.iter().copied());
                let args = Args::parse_from(argv);

                let refusal = workspace_flags_unsupported_in_peer_mode(&args).ok_or_else(|| {
                    format!("{peer_flag} with {workspace:?} must be refused, not silently ignored")
                })?;
                let message = refusal.to_string();
                assert!(
                    message.contains(peer_flag) && message.contains(workspace[0]),
                    "the refusal must name both flags, got: {message}"
                );
            }
        }

        // The native path keeps working: no peer flag, no refusal.
        let native = Args::parse_from(["perl-dap", "--stdio", "--workspace-root", root]);
        assert!(
            workspace_flags_unsupported_in_peer_mode(&native).is_none(),
            "the native adapter must still accept --workspace-root"
        );
        Ok(())
    }
}
