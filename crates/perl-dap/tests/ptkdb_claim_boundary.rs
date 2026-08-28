//! Prevents fake-peer host proof from being promoted into an unearned real
//! ptkdb compatibility claim.

use perl_dap::backend::external_peer::{ExternalDebuggerPeerBackend, PeerSessionToken};
use perl_dap::backend::{DebugBackend, InitializeBackendParams};
use perl_dap::model::{DebugEvent, OutputCategory, StopReason};
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PEER_TOKEN: &str = "0123456789abcdef0123456789abcdef";

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
    assert!(quickstart.contains("Pinned `Devel::ptkdb 1.1091` mirror plugin"));
    assert!(quickstart.contains("This is one-way setup"));
    assert!(quickstart.contains("A peer session starts with no assumed capabilities"));

    let target = read("docs/reference/PTKDB_PEER_INTEGRATION_TARGET.md")?;
    assert!(target.contains("mirror_minimum"));
    assert!(target.contains("Fake-peer conformance establishes host correctness only"));
    assert!(target.contains("does **not** require ptkdb to accept editor control"));
    assert!(target.contains("ptkdb live peer experimental"));
    assert!(target.contains("Devel::ptkdb::set_file"));
    assert!(target.contains("cannot promote stock ptkdb compatibility"));

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
fn pinned_ptkdb_plugin_emits_real_stop_locations_without_control_capabilities()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let peer_addr = listener.local_addr()?;
    let plugin = repo_root().join("fixtures/debug-peer/perl/minimal_ptkdb_peer.pl");

    let harness = r#"
package Devel::ptkdb;
our $VERSION = '1.1091';
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

    let mut child = Command::new("perl")
        .arg("-e")
        .arg(harness)
        .env("PERL_DAP_PEER", peer_addr.to_string())
        .env("PERL_DAP_PEER_TOKEN", PEER_TOKEN)
        .env("PERL_DAP_PEER_MODE", "mirror")
        .env("PTKDB_PLUGIN_UNDER_TEST", plugin)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let stream = accept_plugin(&listener, &mut child, Duration::from_secs(10))?;
    let token = PeerSessionToken::try_from(PEER_TOKEN)?;
    let mut backend = ExternalDebuggerPeerBackend::from_connected_stream_with_token(
        stream,
        Duration::from_secs(3),
        token,
    )?;
    backend.initialize(InitializeBackendParams::default())?;

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut events = Vec::new();
    while Instant::now() < deadline {
        events.extend(backend.drain_events());
        if events
            .iter()
            .any(|event| matches!(event, DebugEvent::Terminated { .. }))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let stderr = child_stderr(&mut child);
    assert!(status.success(), "ptkdb mirror harness failed: {stderr}");

    let console_connected = events.iter().any(|event| {
        matches!(
            event,
            DebugEvent::Output { category, output }
                if *category == OutputCategory::Console
                    && output.contains("Devel::ptkdb 1.1091 mirror connected")
        )
    });
    assert!(console_connected, "missing plugin connection event: {events:?}");

    let stops: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            DebugEvent::Stopped {
                reason,
                position: Some(position),
                ..
            } => Some((
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
        events
            .iter()
            .any(|event| matches!(event, DebugEvent::Terminated { exit_code: None })),
        "missing bounded termination event: {events:?}"
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
