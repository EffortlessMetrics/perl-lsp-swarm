//! Prevents fake-peer host proof from being promoted into an unearned real
//! ptkdb compatibility claim.

use perl_dap::backend::external_peer::{ExternalDebuggerPeerBackend, PeerSessionToken};
use perl_dap::backend::{DebugBackend, DebugBackendCapabilities, InitializeBackendParams};
use perl_dap::model::{DebugEvent, OutputCategory, StopReason};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PEER_TOKEN: &str = "0123456789abcdef0123456789abcdef";
const PTKDB_MODULE_SHA256: &str =
    "2da4a792a732c134f8f4fa3b6b482da9e5df8dec8cd7ae424ad3b6e06c0bceab";
const PTKDB_DIST_SHA256: &str = "889bfc25d107f46718963023cc9662d3d779896a48d729d0327beec0502c226e";

struct ChildCleanup {
    child: Child,
    paths: Vec<PathBuf>,
}

impl ChildCleanup {
    fn new(child: Child, paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self { child, paths: paths.into_iter().collect() }
    }

    fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill()
    }
}

impl Drop for ChildCleanup {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        for path in &self.paths {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(std::fs::read_to_string(repo_root().join(path))?)
}

fn child_stderr(child: &mut Child) -> String {
    let mut output = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut output);
    }
    output
}

fn unique_temp_marker(name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(std::env::temp_dir().join(format!("perl-dap-ptkdb-{name}-{}-{nonce}", std::process::id())))
}

fn accept_plugin(
    listener: &TcpListener,
    child: &mut Child,
    timeout: Duration,
) -> Result<TcpStream, Box<dyn std::error::Error>> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                return Ok(stream);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(error.into()),
        }

        if let Some(status) = child.try_wait()? {
            let stderr = child_stderr(child);
            return Err(format!(
                "ptkdb mirror harness exited before connecting ({status}): {stderr}"
            )
            .into());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let stderr = child_stderr(child);
            return Err(format!("timed out waiting for ptkdb mirror plugin: {stderr}").into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn ptkdb_docs_separate_bootstrap_host_protocol_and_live_partner_proof()
-> Result<(), Box<dyn std::error::Error>> {
    let quickstart = read("docs/how-to/EXTERNAL_DEBUGGER_PEER_QUICKSTART.md")?;
    assert!(quickstart.contains("Experimental / developer preview"));
    assert!(quickstart.contains("Stock `Devel::ptkdb` live peer"));
    assert!(quickstart.contains("Not yet proven"));
    assert!(quickstart.contains("Marked ptkdb-shaped `Devel::ptkdb 1.1091` reference adapter"));
    assert!(quickstart.contains("SHA-256"));
    assert!(quickstart.contains("This is one-way setup"));
    assert!(quickstart.contains("A peer session starts with no assumed capabilities"));

    let target = read("docs/reference/PTKDB_PEER_INTEGRATION_TARGET.md")?;
    assert!(target.contains("mirror_minimum"));
    assert!(target.contains("Fake-peer conformance establishes host correctness only"));
    assert!(target.contains("does **not** require ptkdb to accept editor control"));
    assert!(target.contains("ptkdb live peer experimental"));
    assert!(target.contains("set_file"));
    assert!(target.contains("not a stock ptkdb or Tk session and cannot promote compatibility"));
    assert!(target.contains(PTKDB_MODULE_SHA256));

    let decisions = read("docs/reference/EXTERNAL_DEBUGGER_PEER_DECISIONS.md")?;
    assert!(decisions.contains("stock ptkdb live compatibility  not proven"));
    assert!(decisions.contains("A session starts with `none`"));
    assert!(
        decisions.contains("The Microsoft DAP implementor listing names `perl-dap`, not ptkdb")
    );

    Ok(())
}

#[test]
fn ptkdb_docs_do_not_reintroduce_optimistic_v1_defaults() -> Result<(), Box<dyn std::error::Error>>
{
    for path in [
        "docs/how-to/EXTERNAL_DEBUGGER_PEER_QUICKSTART.md",
        "docs/reference/PTKDB_PEER_INTEGRATION_TARGET.md",
        "docs/reference/EXTERNAL_DEBUGGER_PEER_DECISIONS.md",
    ] {
        let content = read(path)?;
        for forbidden in [
            "✅ against a peer that speaks the protocol",
            "realistic v1 capability report for ptkdb",
            "stock ptkdb is supported",
            "fully supported ptkdb integration",
        ] {
            assert!(
                !content.contains(forbidden),
                "{path} contains unearned ptkdb claim {forbidden:?}"
            );
        }
    }

    Ok(())
}

#[test]
fn reference_ptkdb_adapter_emits_harness_stop_locations_without_control_capabilities()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let peer_addr = listener.local_addr()?;
    let plugin = repo_root().join("fixtures/debug-peer/perl/minimal_ptkdb_peer.pl");

    let harness = r#"
package Devel::ptkdb;
our $VERSION = '1.1091';
our $PERL_DAP_MIRROR_SOURCE = 'CPAN:AEPAGE/Devel-ptkdb-1.1091';
our $PERL_DAP_MIRROR_SHA256 = '2da4a792a732c134f8f4fa3b6b482da9e5df8dec8cd7ae424ad3b6e06c0bceab';
our $PERL_DAP_MIRROR_DIST_SHA256 = '889bfc25d107f46718963023cc9662d3d779896a48d729d0327beec0502c226e';
sub set_file {
    my ($self, $path, $line) = @_;
    push @main::original_calls, "$path:$line";
    return $line;
}
package DB;
our $on = 1;
package main;
our @original_calls;
my $loaded = do $ENV{PTKDB_PLUGIN_UNDER_TEST};
die "plugin load failed: " . ($@ || $!) unless $loaded;
my $window = bless {}, 'Devel::ptkdb';
sub DB::DB {
    $window->set_file('/work/first.pl', 7);
    $window->set_file('/work/second.pl', 11);
}
DB::DB();
die "original set_file was not preserved"
    unless join(',', @original_calls) eq '/work/first.pl:7,/work/second.pl:11';
exit 0;
"#;

    let child = Command::new("perl")
        .arg("-e")
        .arg(harness)
        .env("PERL_DAP_PEER", peer_addr.to_string())
        .env("PERL_DAP_PEER_TOKEN", PEER_TOKEN)
        .env("PERL_DAP_PEER_MODE", "mirror")
        .env("PTKDB_PLUGIN_UNDER_TEST", plugin)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut child = ChildCleanup::new(child, Vec::<PathBuf>::new());

    let stream = accept_plugin(&listener, &mut child.child, Duration::from_secs(10))?;
    let token = PeerSessionToken::try_from(PEER_TOKEN)?;
    let mut backend = ExternalDebuggerPeerBackend::from_connected_stream_with_token(
        stream,
        Duration::from_secs(3),
        token,
    )?;
    backend.initialize(InitializeBackendParams::default())?;
    assert_eq!(
        backend.capabilities(),
        DebugBackendCapabilities::none(),
        "empty peer capabilities must remain empty after negotiation"
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut events = Vec::new();
    while Instant::now() < deadline {
        events.extend(backend.drain_events());
        if events.iter().any(|event| matches!(event, DebugEvent::Terminated { .. })) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let status = loop {
        if let Some(status) = child.child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            break child.child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let stderr = child_stderr(&mut child.child);
    assert!(status.success(), "ptkdb mirror harness failed: {stderr}");

    let console_connected = events.iter().any(|event| {
        matches!(
            event,
            DebugEvent::Output { category, output }
                if *category == OutputCategory::Console
                    && output.contains("Devel::ptkdb 1.1091 reference mirror connected")
        )
    });
    assert!(console_connected, "missing plugin connection event: {events:?}");

    let stops: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            DebugEvent::Stopped { reason, position: Some(position), .. } => Some((
                reason.clone(),
                position.source.path.to_string_lossy().to_string(),
                position.line,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        stops,
        vec![
            (StopReason::Pause, "/work/first.pl".to_string(), 7),
            (StopReason::Pause, "/work/second.pl".to_string(), 11),
        ]
    );
    assert!(
        events.iter().any(|event| matches!(event, DebugEvent::Terminated { exit_code: None })),
        "missing bounded termination event: {events:?}"
    );

    Ok(())
}

#[test]
fn reference_ptkdb_adapter_rejects_unpinned_version_without_touching_ptkdb()
-> Result<(), Box<dyn std::error::Error>> {
    let plugin = repo_root().join("fixtures/debug-peer/perl/minimal_ptkdb_peer.pl");
    let harness = r#"
package Devel::ptkdb;
our $VERSION = '1.1090';
our $PERL_DAP_MIRROR_SOURCE = 'CPAN:AEPAGE/Devel-ptkdb-1.1091';
our $PERL_DAP_MIRROR_SHA256 = '2da4a792a732c134f8f4fa3b6b482da9e5df8dec8cd7ae424ad3b6e06c0bceab';
our $PERL_DAP_MIRROR_DIST_SHA256 = '889bfc25d107f46718963023cc9662d3d779896a48d729d0327beec0502c226e';
sub set_file { return "original:$_[2]"; }
package main;
my $loaded = do $ENV{PTKDB_PLUGIN_UNDER_TEST};
die "plugin load failed: " . ($@ || $!) unless $loaded;
my $window = bless {}, 'Devel::ptkdb';
my $value = $window->set_file('/work/rejected.pl', 13);
die "unpinned plugin touched set_file: $value"
    unless $value eq 'original:13';
exit 0;
"#;

    let output = Command::new("perl")
        .arg("-e")
        .arg(harness)
        .env("PERL_DAP_PEER", "127.0.0.1:1")
        .env("PERL_DAP_PEER_TOKEN", PEER_TOKEN)
        .env("PERL_DAP_PEER_MODE", "mirror")
        .env("PTKDB_PLUGIN_UNDER_TEST", plugin)
        .output()?;

    let stderr = String::from_utf8(output.stderr)?;
    assert!(output.status.success(), "unpinned ptkdb rejection harness failed: {stderr}");
    assert!(
        stderr.contains(
            "reference mirror adapter requires Devel::ptkdb 1.1091; observed 1.1090 -- leaving ptkdb untouched"
        ),
        "missing exact-version rejection diagnostic: {stderr}"
    );
    assert!(
        !stderr.contains("cannot connect"),
        "version rejection must happen before any peer connection: {stderr}"
    );

    Ok(())
}

#[test]
fn reference_ptkdb_adapter_rejects_malformed_authenticated_response_without_touching_ptkdb()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let peer_addr = listener.local_addr()?;
    let plugin = repo_root().join("fixtures/debug-peer/perl/minimal_ptkdb_peer.pl");
    let harness = r#"
package Devel::ptkdb;
our $VERSION = '1.1091';
our $PERL_DAP_MIRROR_SOURCE = 'CPAN:AEPAGE/Devel-ptkdb-1.1091';
our $PERL_DAP_MIRROR_SHA256 = '2da4a792a732c134f8f4fa3b6b482da9e5df8dec8cd7ae424ad3b6e06c0bceab';
our $PERL_DAP_MIRROR_DIST_SHA256 = '889bfc25d107f46718963023cc9662d3d779896a48d729d0327beec0502c226e';
sub set_file { return "original:$_[2]"; }
package main;
my $loaded = do $ENV{PTKDB_PLUGIN_UNDER_TEST};
die "plugin load failed: " . ($@ || $!) unless $loaded;
my $window = bless {}, 'Devel::ptkdb';
my $value = $window->set_file('/work/rejected.pl', 13);
die "malformed response touched set_file: $value" unless $value eq 'original:13';
exit 0;
"#;

    let mut child = ChildCleanup::new(
        Command::new("perl")
            .arg("-e")
            .arg(harness)
            .env("PERL_DAP_PEER", peer_addr.to_string())
            .env("PERL_DAP_PEER_TOKEN", PEER_TOKEN)
            .env("PERL_DAP_PEER_MODE", "mirror")
            .env("PTKDB_PLUGIN_UNDER_TEST", plugin)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?,
        Vec::<PathBuf>::new(),
    );

    let (mut stream, _) = listener.accept()?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    let mut hello = [0_u8; 4096];
    let read = stream.read(&mut hello)?;
    assert!(
        std::str::from_utf8(&hello[..read])?.contains("peer/hello"),
        "plugin did not send peer/hello: {:?}",
        &hello[..read]
    );
    // JSON numbers must not be accepted as the protocol's boolean success flag.
    let malformed_response =
        br#"{"type":"response","requestSeq":1,"command":"peer/hello","success":1}"#;
    write!(stream, "Content-Length: {}\r\n\r\n", malformed_response.len())?;
    stream.write_all(malformed_response)?;
    drop(stream);

    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            break child.child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let stderr = child_stderr(&mut child.child);
    assert!(status.success(), "malformed-response harness failed: {stderr}");
    assert!(
        stderr.contains(
            "invalid peer/hello response: peer/hello response has an invalid success flag"
        ),
        "missing fail-closed diagnostic: {stderr}"
    );
    Ok(())
}

#[test]
fn reference_ptkdb_adapter_rejects_string_success_flag_without_touching_ptkdb()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let peer_addr = listener.local_addr()?;
    let plugin = repo_root().join("fixtures/debug-peer/perl/minimal_ptkdb_peer.pl");
    let harness = r#"
package Devel::ptkdb;
our $VERSION = '1.1091';
our $PERL_DAP_MIRROR_SOURCE = 'CPAN:AEPAGE/Devel-ptkdb-1.1091';
our $PERL_DAP_MIRROR_SHA256 = '2da4a792a732c134f8f4fa3b6b482da9e5df8dec8cd7ae424ad3b6e06c0bceab';
our $PERL_DAP_MIRROR_DIST_SHA256 = '889bfc25d107f46718963023cc9662d3d779896a48d729d0327beec0502c226e';
sub set_file { return "original:$_[2]"; }
package main;
my $loaded = do $ENV{PTKDB_PLUGIN_UNDER_TEST};
die "plugin load failed: " . ($@ || $!) unless $loaded;
my $window = bless {}, 'Devel::ptkdb';
my $value = $window->set_file('/work/rejected.pl', 13);
die "string response touched set_file: $value" unless $value eq 'original:13';
exit 0;
"#;

    let mut child = ChildCleanup::new(
        Command::new("perl")
            .arg("-e")
            .arg(harness)
            .env("PERL_DAP_PEER", peer_addr.to_string())
            .env("PERL_DAP_PEER_TOKEN", PEER_TOKEN)
            .env("PERL_DAP_PEER_MODE", "mirror")
            .env("PTKDB_PLUGIN_UNDER_TEST", plugin)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?,
        Vec::<PathBuf>::new(),
    );

    let (mut stream, _) = listener.accept()?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    let mut hello = [0_u8; 4096];
    let read = stream.read(&mut hello)?;
    assert!(std::str::from_utf8(&hello[..read])?.contains("peer/hello"));
    let malformed_response =
        br#"{"type":"response","requestSeq":1,"command":"peer/hello","success":"1"}"#;
    write!(stream, "Content-Length: {}\r\n\r\n", malformed_response.len())?;
    stream.write_all(malformed_response)?;
    drop(stream);

    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            break child.child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let stderr = child_stderr(&mut child.child);
    assert!(status.success(), "string-response harness failed: {stderr}");
    assert!(
        stderr.contains(
            "invalid peer/hello response: peer/hello response has an invalid success flag"
        ),
        "missing strict-boolean diagnostic: {stderr}"
    );
    Ok(())
}

#[test]
fn reference_peer_rejects_non_object_post_handshake_frame_without_terminating()
-> Result<(), Box<dyn std::error::Error>> {
    for malformed in [b"[]".as_slice(), b"1".as_slice()] {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let peer_addr = listener.local_addr()?;
        let plugin = repo_root().join("fixtures/debug-peer/perl/minimal_ptkdb_peer.pl");
        let mut child = ChildCleanup::new(
            Command::new("perl")
                .arg(&plugin)
                .env("PERL_DAP_PEER", peer_addr.to_string())
                .env("PERL_DAP_PEER_MODE", "mirror")
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()?,
            Vec::<PathBuf>::new(),
        );

        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        let mut hello = [0_u8; 4096];
        let read = stream.read(&mut hello)?;
        assert!(std::str::from_utf8(&hello[..read])?.contains("peer/hello"));
        let response = br#"{"type":"response","requestSeq":1,"command":"peer/hello","success":true,"body":{"sessionId":"test"}}"#;
        write!(stream, "Content-Length: {}\r\n\r\n", response.len())?;
        stream.write_all(response)?;
        let mut events = Vec::new();
        while !String::from_utf8_lossy(&events).contains("debugger/stopped") {
            let mut chunk = [0_u8; 4096];
            let read = stream.read(&mut chunk)?;
            events.extend_from_slice(&chunk[..read]);
        }
        write!(stream, "Content-Length: {}\r\n\r\n", malformed.len())?;
        stream.write_all(malformed)?;
        drop(stream);

        let status = child.child.wait()?;
        let stderr = child_stderr(&mut child.child);
        assert!(status.success(), "non-object post-handshake frame terminated the peer: {stderr}");
        assert!(
            stderr.contains("post-handshake frame must be a JSON object; closing peer session"),
            "missing non-object frame diagnostic for {:?}: {stderr}",
            malformed
        );
    }
    Ok(())
}

#[test]
fn reference_peer_fails_closed_cleanly_on_handshake_failures()
-> Result<(), Box<dyn std::error::Error>> {
    let plugin = repo_root().join("fixtures/debug-peer/perl/minimal_ptkdb_peer.pl");
    for (name, response, expected_error) in [
        (
            "array handshake",
            Some(br#"[]"#.as_slice()),
            "invalid peer/hello response: peer/hello response must be a JSON object",
        ),
        (
            "false scalar handshake",
            Some(br#"0"#.as_slice()),
            "invalid peer/hello response: peer/hello response must be a JSON object",
        ),
        ("EOF handshake", None, "peer/hello response failed: connection closed"),
        ("timeout handshake", None, "peer/hello response failed: read timed out"),
    ] {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let peer_addr = listener.local_addr()?;
        let mut child = ChildCleanup::new(
            Command::new("perl")
                .arg(&plugin)
                .env("PERL_DAP_PEER", peer_addr.to_string())
                .env("PERL_DAP_PEER_MODE", "mirror")
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()?,
            Vec::<PathBuf>::new(),
        );

        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        let mut hello = [0_u8; 4096];
        let read = stream.read(&mut hello)?;
        assert!(std::str::from_utf8(&hello[..read])?.contains("peer/hello"));

        match (name, response) {
            ("array handshake" | "false scalar handshake", Some(response)) => {
                write!(stream, "Content-Length: {}\r\n\r\n", response.len())?;
                stream.write_all(response)?;
                drop(stream);
            }
            ("EOF handshake", None) => drop(stream),
            ("timeout handshake", None) => {
                let deadline = Instant::now() + Duration::from_secs(5);
                while child.child.try_wait()?.is_none() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            _ => return Err("handshake case setup must be internally consistent".into()),
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                break child.child.wait()?;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let stderr = child_stderr(&mut child.child);
        assert!(status.success(), "{name} terminated nonzero: {stderr}");
        assert!(
            stderr.contains("reference peer disabled:") && stderr.contains(expected_error),
            "{name} missing clean fail-closed diagnostic: {stderr}"
        );
        assert!(!stderr.contains(" at "), "{name} emitted a Perl die traceback: {stderr}");
    }

    Ok(())
}

#[test]
fn reference_ptkdb_adapter_rejects_wrong_source_and_bad_rendezvous_without_touching_ptkdb()
-> Result<(), Box<dyn std::error::Error>> {
    let plugin = repo_root().join("fixtures/debug-peer/perl/minimal_ptkdb_peer.pl");
    let harness = r#"
package Devel::ptkdb;
our $VERSION = '1.1091';
our $PERL_DAP_MIRROR_SOURCE = $ENV{PTKDB_SOURCE_MARKER};
our $PERL_DAP_MIRROR_SHA256 = $ENV{PTKDB_SOURCE_SHA256};
our $PERL_DAP_MIRROR_DIST_SHA256 = $ENV{PTKDB_DIST_SHA256};
sub set_file { return "original:$_[2]"; }
package main;
my $loaded = do $ENV{PTKDB_PLUGIN_UNDER_TEST};
die "plugin load failed: " . ($@ || $!) unless $loaded;
my $window = bless {}, 'Devel::ptkdb';
my $value = $window->set_file('/work/rejected.pl', 13);
die "adapter touched set_file: $value" unless $value eq 'original:13';
exit 0;
"#;

    let cases = [
        (
            "wrong source",
            "not-the-pinned-cpan-source",
            Some("127.0.0.1:1"),
            Some(PEER_TOKEN),
            Some("mirror"),
            "provenance check failed",
        ),
        (
            "wrong source digest",
            "CPAN:AEPAGE/Devel-ptkdb-1.1091",
            Some("127.0.0.1:1"),
            Some(PEER_TOKEN),
            Some("mirror"),
            "provenance check failed",
        ),
        (
            "wrong distribution digest",
            "CPAN:AEPAGE/Devel-ptkdb-1.1091",
            Some("127.0.0.1:1"),
            Some(PEER_TOKEN),
            Some("mirror"),
            "distribution digest does not match",
        ),
        (
            "missing token",
            "CPAN:AEPAGE/Devel-ptkdb-1.1091",
            Some("127.0.0.1:1"),
            None,
            Some("mirror"),
            "PERL_DAP_PEER_TOKEN is required",
        ),
        (
            "non-loopback peer",
            "CPAN:AEPAGE/Devel-ptkdb-1.1091",
            Some("0.0.0.0:1"),
            Some(PEER_TOKEN),
            Some("mirror"),
            "malformed or non-loopback",
        ),
        (
            "missing peer",
            "CPAN:AEPAGE/Devel-ptkdb-1.1091",
            None,
            Some(PEER_TOKEN),
            Some("mirror"),
            "PERL_DAP_PEER not set",
        ),
    ];

    for (name, source, peer, token, mode, diagnostic) in cases {
        let mut command = Command::new("perl");
        command
            .arg("-e")
            .arg(harness)
            .env("PTKDB_PLUGIN_UNDER_TEST", &plugin)
            .env("PTKDB_SOURCE_MARKER", source)
            .env(
                "PTKDB_SOURCE_SHA256",
                if name == "wrong source digest" {
                    "0000000000000000000000000000000000000000000000000000000000000000"
                } else {
                    PTKDB_MODULE_SHA256
                },
            )
            .env(
                "PTKDB_DIST_SHA256",
                if name == "wrong distribution digest" {
                    "0000000000000000000000000000000000000000000000000000000000000000"
                } else {
                    PTKDB_DIST_SHA256
                },
            )
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        match peer {
            Some(value) => {
                command.env("PERL_DAP_PEER", value);
            }
            None => {
                command.env_remove("PERL_DAP_PEER");
            }
        }
        match token {
            Some(value) => {
                command.env("PERL_DAP_PEER_TOKEN", value);
            }
            None => {
                command.env_remove("PERL_DAP_PEER_TOKEN");
            }
        }
        match mode {
            Some(value) => {
                command.env("PERL_DAP_PEER_MODE", value);
            }
            None => {
                command.env_remove("PERL_DAP_PEER_MODE");
            }
        }

        let output = command.output()?;
        let stderr = String::from_utf8(output.stderr)?;
        assert!(output.status.success(), "{name} rejection harness failed: {stderr}");
        assert!(stderr.contains(diagnostic), "{name} diagnostic missing: {stderr}");
        assert!(!stderr.contains(PEER_TOKEN), "{name} leaked the peer token: {stderr}");
        assert!(!stderr.contains("cannot connect"), "{name} reached the network: {stderr}");
    }

    Ok(())
}

#[test]
fn reference_ptkdb_adapter_rejects_loaded_module_without_artifact_binding()
-> Result<(), Box<dyn std::error::Error>> {
    let plugin = repo_root().join("fixtures/debug-peer/perl/minimal_ptkdb_peer.pl");
    let module_dir = unique_temp_marker("loaded-module")?;
    let module_path = module_dir.join("Devel/ptkdb.pm");
    std::fs::create_dir_all(module_path.parent().ok_or("missing module parent")?)?;
    std::fs::write(
        &module_path,
        format!(
            "package Devel::ptkdb;\nour $VERSION = '1.1091';\nour $PERL_DAP_MIRROR_SOURCE = '{}';\nour $PERL_DAP_MIRROR_SHA256 = '{}';\nour $PERL_DAP_MIRROR_DIST_SHA256 = '{}';\nsub set_file {{ return \"original:$_[2]\"; }}\n1;\n",
            "CPAN:AEPAGE/Devel-ptkdb-1.1091", PTKDB_MODULE_SHA256, PTKDB_DIST_SHA256
        ),
    )?;
    let harness = r#"
BEGIN { unshift @INC, $ENV{PTKDB_MODULE_DIR}; }
require Devel::ptkdb;
package main;
my $loaded = do $ENV{PTKDB_PLUGIN_UNDER_TEST};
die "plugin load failed: " . ($@ || $!) unless $loaded;
my $window = bless {}, 'Devel::ptkdb';
my $value = $window->set_file('/work/rejected.pl', 13);
die "loaded-module rejection touched set_file: $value" unless $value eq 'original:13';
exit 0;
"#;
    let output = Command::new("perl")
        .arg("-e")
        .arg(harness)
        .env("PTKDB_MODULE_DIR", &module_dir)
        .env("PTKDB_PLUGIN_UNDER_TEST", &plugin)
        .env("PTKDB_SOURCE_MARKER", "CPAN:AEPAGE/Devel-ptkdb-1.1091")
        .env("PTKDB_SOURCE_SHA256", PTKDB_MODULE_SHA256)
        .env("PTKDB_DIST_SHA256", PTKDB_DIST_SHA256)
        .env("PERL_DAP_PEER", "127.0.0.1:1")
        .env("PERL_DAP_PEER_TOKEN", PEER_TOKEN)
        .env("PERL_DAP_PEER_MODE", "mirror")
        .output()?;
    let stderr = String::from_utf8(output.stderr)?;
    std::fs::remove_file(&module_path)?;
    std::fs::remove_dir_all(&module_dir)?;
    assert!(output.status.success(), "loaded-module rejection harness failed: {stderr}");
    assert!(
        stderr.contains("loaded Devel/ptkdb.pm bytes cannot be bound to this provenance check"),
        "missing artifact-binding rejection diagnostic: {stderr}"
    );
    assert!(!stderr.contains("cannot connect"), "rejection must precede peer connection: {stderr}");
    Ok(())
}

#[test]
fn reference_ptkdb_adapter_rejects_false_inc_entry_before_connecting()
-> Result<(), Box<dyn std::error::Error>> {
    let plugin = repo_root().join("fixtures/debug-peer/perl/minimal_ptkdb_peer.pl");
    let harness = r#"
package Devel::ptkdb;
our $VERSION = '1.1091';
our $PERL_DAP_MIRROR_SOURCE = 'CPAN:AEPAGE/Devel-ptkdb-1.1091';
our $PERL_DAP_MIRROR_SHA256 = '2da4a792a732c134f8f4fa3b6b482da9e5df8dec8cd7ae424ad3b6e06c0bceab';
our $PERL_DAP_MIRROR_DIST_SHA256 = '889bfc25d107f46718963023cc9662d3d779896a48d729d0327beec0502c226e';
sub set_file { return "original:$_[2]"; }
package main;
$INC{'Devel/ptkdb.pm'} = '';
my $loaded = do $ENV{PTKDB_PLUGIN_UNDER_TEST};
die "plugin load failed: " . ($@ || $!) unless $loaded;
my $window = bless {}, 'Devel::ptkdb';
my $value = $window->set_file('/work/rejected.pl', 13);
die "false %INC rejection touched set_file: $value" unless $value eq 'original:13';
exit 0;
"#;
    let output = Command::new("perl")
        .arg("-e")
        .arg(harness)
        .env("PTKDB_PLUGIN_UNDER_TEST", plugin)
        .env("PTKDB_SOURCE_MARKER", "CPAN:AEPAGE/Devel-ptkdb-1.1091")
        .env("PTKDB_SOURCE_SHA256", PTKDB_MODULE_SHA256)
        .env("PTKDB_DIST_SHA256", PTKDB_DIST_SHA256)
        .env("PERL_DAP_PEER", "127.0.0.1:1")
        .env("PERL_DAP_PEER_TOKEN", PEER_TOKEN)
        .env("PERL_DAP_PEER_MODE", "mirror")
        .output()?;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(output.status.success(), "false %INC rejection harness failed: {stderr}");
    assert!(
        stderr.contains("loaded Devel/ptkdb.pm bytes cannot be bound to this provenance check"),
        "missing false-%INC rejection diagnostic: {stderr}"
    );
    assert!(
        !stderr.contains("cannot connect"),
        "false %INC must fail before peer connection: {stderr}"
    );
    Ok(())
}

#[test]
fn ptkdb_plugin_survives_host_disconnect_and_later_event_write()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let peer_addr = listener.local_addr()?;
    let plugin = repo_root().join("fixtures/debug-peer/perl/minimal_ptkdb_peer.pl");
    let release = unique_temp_marker("release")?;
    let survived = unique_temp_marker("survived")?;
    let _ = std::fs::remove_file(&release);
    let _ = std::fs::remove_file(&survived);

    let harness = r#"
package Devel::ptkdb;
our $VERSION = '1.1091';
our $PERL_DAP_MIRROR_SOURCE = 'CPAN:AEPAGE/Devel-ptkdb-1.1091';
our $PERL_DAP_MIRROR_SHA256 = '2da4a792a732c134f8f4fa3b6b482da9e5df8dec8cd7ae424ad3b6e06c0bceab';
our $PERL_DAP_MIRROR_DIST_SHA256 = '889bfc25d107f46718963023cc9662d3d779896a48d729d0327beec0502c226e';
sub set_file { return $_[2]; }
package DB;
our $on = 1;
package main;
our ($window, $path, $line);
my $loaded = do $ENV{PTKDB_PLUGIN_UNDER_TEST};
die "plugin load failed: " . ($@ || $!) unless $loaded;
$window = bless {}, 'Devel::ptkdb';
sub DB::DB { $window->set_file($path, $line); }
$path = '/work/before_disconnect.pl';
$line = 3;
DB::DB();
my $deadline = time + 10;
while (!-e $ENV{PTKDB_RELEASE_FILE} && time < $deadline) {
    select undef, undef, undef, 0.01;
}
die "release timeout" unless -e $ENV{PTKDB_RELEASE_FILE};
$path = '/work/after_disconnect.pl';
$line = 5;
DB::DB();
open my $fh, '>', $ENV{PTKDB_SURVIVED_FILE}
    or die "survival marker: $!";
print {$fh} "survived\n";
close $fh or die "close survival marker: $!";
exit 0;
"#;

    let child = Command::new("perl")
        .arg("-e")
        .arg(harness)
        .env("PERL_DAP_PEER", peer_addr.to_string())
        .env("PERL_DAP_PEER_TOKEN", PEER_TOKEN)
        .env("PERL_DAP_PEER_MODE", "mirror")
        .env("PTKDB_PLUGIN_UNDER_TEST", plugin)
        .env("PTKDB_RELEASE_FILE", &release)
        .env("PTKDB_SURVIVED_FILE", &survived)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut child = ChildCleanup::new(child, [release.clone(), survived.clone()]);

    let stream = accept_plugin(&listener, &mut child.child, Duration::from_secs(10))?;
    let token = PeerSessionToken::try_from(PEER_TOKEN)?;
    let mut backend = ExternalDebuggerPeerBackend::from_connected_stream_with_token(
        stream,
        Duration::from_secs(3),
        token,
    )?;
    backend.initialize(InitializeBackendParams::default())?;

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut saw_initial_stop = false;
    while Instant::now() < deadline {
        saw_initial_stop = backend.drain_events().iter().any(|event| {
            matches!(
                event,
                DebugEvent::Stopped {
                    position: Some(position),
                    ..
                } if position.source.path.to_string_lossy() == "/work/before_disconnect.pl"
                    && position.line == 3
            )
        });
        if saw_initial_stop {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(saw_initial_stop, "plugin did not report the pre-disconnect stop");

    backend.disconnect(false)?;
    drop(backend);
    std::thread::sleep(Duration::from_millis(100));
    std::fs::write(&release, b"release\n")?;

    let child_deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.child.try_wait()? {
            break status;
        }
        if Instant::now() >= child_deadline {
            let _ = child.kill();
            break child.child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let stderr = child_stderr(&mut child.child);
    let survived_disconnect = survived.exists();
    assert!(
        status.success(),
        "host disconnect killed the ptkdb debuggee during a later peer write: {stderr}"
    );
    assert!(
        survived_disconnect,
        "debuggee did not continue after the mirror transport closed: {stderr}"
    );

    Ok(())
}

#[test]
fn vscode_keeps_native_as_the_default_debugger_backend() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = read("vscode-extension/package.json")?;
    assert!(manifest.contains("debuggerBackend"));
    assert!(manifest.contains("\"default\": \"native\""));
    assert!(manifest.contains("Experimental external live peer"));
    assert!(manifest.contains("stock Devel::ptkdb compatibility is not proven"));
    assert!(manifest.contains("Perl: External Debugger Peer (experimental)"));
    Ok(())
}
