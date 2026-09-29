//! Aggregate NativeCritic publication characterization for #16756.
//! These real handlers do not establish per-root policy isolation.

use super::{LspServer, WorkspaceFolderState};
use anyhow::{Context, Result, anyhow, ensure};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::io::{self, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::thread::JoinHandle;
use std::time::Duration;

const SOURCE: &str = "my $out = `ls -la`;\nopen(my $fh, '<', 'file.txt');\nprint 1;\n";
const NATIVE_CODE: &str = "native.testing.require_use_strict";
const PUSH_NATIVE_CODE: &str = "native.io.unchecked_open_close";
const DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct Capture {
    bytes: Arc<Mutex<Vec<u8>>>,
    flushed: mpsc::Sender<()>,
}

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes.lock().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushed.send(()).map_err(|error| io::Error::other(error.to_string()))
    }
}

struct Fixture {
    server: Arc<LspServer>,
    capture: Capture,
    flushed: mpsc::Receiver<()>,
    document: String,
    sibling: String,
    _root: tempfile::TempDir,
    sibling_dir: tempfile::TempDir,
    fence: u64,
}

impl Fixture {
    fn new() -> Result<Self> {
        let root = tempfile::tempdir()?;
        let sibling_dir = tempfile::tempdir()?;
        let root_uri = url::Url::from_directory_path(root.path())
            .map_err(|()| anyhow!("root URI"))?
            .to_string();
        let sibling = url::Url::from_directory_path(sibling_dir.path())
            .map_err(|()| anyhow!("sibling URI"))?
            .to_string();
        let document = url::Url::from_file_path(root.path().join("main.pl"))
            .map_err(|()| anyhow!("document URI"))?
            .to_string();
        let document = perl_uri::uri_key(&document);
        let (tx, flushed) = mpsc::channel();
        let capture = Capture { bytes: Arc::new(Mutex::new(Vec::new())), flushed: tx };
        let server = Arc::new(LspServer::with_output(Arc::new(Mutex::new(
            Box::new(capture.clone()) as Box<dyn Write + Send>,
        ))));
        server.publish_position_encoding_session_context();
        server.test_configure_critic_engine(perl_lsp_rs_core::config::CriticEngine::Native);
        server.test_configure_native_critic_profile("strict");
        // Keep the explicit native positive premise through project-config resets.
        *server.server_config_baseline.lock() = Some(server.config.lock().clone());
        server
            .workspace_folders
            .lock()
            .push(WorkspaceFolderState::new(root_uri).with_path(root.path().to_path_buf()));
        server.test_apply_did_open(&document, SOURCE, 1)?;
        Ok(Self { server, capture, flushed, document, sibling, _root: root, sibling_dir, fence: 0 })
    }

    fn messages(&mut self) -> Result<Vec<Value>> {
        // An actual outbound marker gives FIFO writer acknowledgment. Absence
        // assertions never depend on a sleep or on an unflushed byte snapshot.
        self.fence = self.fence.checked_add(1).context("fence overflow")?;
        let marker = self.fence;
        self.server
            .outbound
            .send_notification("$/criticProofFence", json!({ "sequence": marker }))?;
        let deadline = std::time::Instant::now() + DEADLINE;
        loop {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .context("outbound proof fence timed out")?;
            self.flushed.recv_timeout(remaining).context("outbound writer did not acknowledge")?;
            let mut framer = perl_lsp_rs_core::transport::framing::ContentLengthFramer::new();
            framer.push(&self.capture.bytes.lock());
            let mut messages = Vec::new();
            while let Some(body) = framer.try_next()? {
                messages.push(serde_json::from_slice::<Value>(&body)?);
            }
            if messages.iter().any(|message| {
                message.get("method").and_then(Value::as_str) == Some("$/criticProofFence")
                    && message.pointer("/params/sequence").and_then(Value::as_u64) == Some(marker)
            }) {
                return Ok(messages);
            }
        }
    }

    fn document_report(&self, previous: Option<&str>) -> Result<Value> {
        self.server
            .test_handle_document_diagnostic(Some(json!({
                "textDocument": { "uri": self.document }, "previousResultId": previous,
            })))?
            .context("missing document report")
    }

    fn workspace_report(&self, previous: Option<&str>) -> Result<Value> {
        let previous = previous
            .map(|value| json!([{ "uri": self.document, "value": value }]))
            .unwrap_or_else(|| json!([]));
        let report = self
            .server
            .test_handle_workspace_diagnostic(Some(json!({
                "previousResultIds": previous,
            })))?
            .context("missing workspace report")?;
        Ok(report)
    }

    fn actions(&self) -> Result<Value> {
        actions(&self.server, &self.document)
    }

    fn push_count(&mut self) -> Result<usize> {
        Ok(self.messages()?.iter().filter(|message| is_push(message, &self.document)).count())
    }

    fn assert_automatic_recovery_push(&mut self, previous_count: usize) -> Result<()> {
        // This must precede assert_positive's explicit publish. Historical
        // frames cannot satisfy the automatic recomputation trigger oracle.
        let messages = self.messages()?;
        let newer = messages
            .iter()
            .filter(|message| is_push(message, &self.document))
            .skip(previous_count)
            .collect::<Vec<_>>();
        ensure!(!newer.is_empty(), "recovery did not publish a new target frame");
        ensure!(
            newer.iter().any(|message| message.get("params").is_some_and(|params| has_code(
                params,
                "diagnostics",
                PUSH_NATIVE_CODE
            ))),
            "recovery's new frames lack the native IO finding"
        );
        Ok(())
    }

    fn assert_positive(&mut self) -> Result<String> {
        let report = self.document_report(None)?;
        ensure!(has_native(&report, "items"), "document native positive missing: {report}");
        let id = result_id(&report)?.to_owned();
        let reused = self.document_report(Some(&id))?;
        ensure!(
            reused.get("kind").and_then(Value::as_str) == Some("unchanged"),
            "document reuse missing: {reused}"
        );
        let workspace = self.workspace_report(None)?;
        let workspace = workspace_item(&workspace, &self.document)?;
        ensure!(has_native(workspace, "items"), "workspace native positive missing: {workspace}");
        let workspace_id = result_id(workspace)?;
        let reused = self.workspace_report(Some(workspace_id))?;
        let reused = workspace_item(&reused, &self.document)?;
        ensure!(
            reused.get("kind").and_then(Value::as_str) == Some("unchanged"),
            "workspace reuse missing: {reused}"
        );
        let actions = self.actions()?;
        ensure!(has_native_action(&actions), "native action positive missing: {actions}");
        self.server.publish_diagnostics(&self.document);
        let messages = self.messages()?;
        ensure!(
            messages.iter().any(|message| is_push(message, &self.document)
                && message.get("params").is_some_and(|params| has_code(
                    params,
                    "diagnostics",
                    PUSH_NATIVE_CODE
                ))),
            "real non-overlapping native push row missing"
        );
        Ok(id)
    }

    fn assert_unavailable(&mut self, previous: &str) -> Result<()> {
        let before =
            self.messages()?.iter().filter(|message| is_push(message, &self.document)).count();
        self.server.publish_diagnostics(&self.document);
        let report = self.document_report(Some(previous))?;
        unavailable(&report, "document")?;
        let workspace = self.workspace_report(Some(previous))?;
        workspace_unavailable(&workspace, &self.document)?;
        let actions = self.actions()?;
        ensure!(!has_any_native_action(&actions), "unstable native action published: {actions}");
        let after =
            self.messages()?.iter().filter(|message| is_push(message, &self.document)).count();
        ensure!(before == after, "unstable push enqueued a frame ({before} -> {after})");
        Ok(())
    }

    fn begin_transition(&self) -> Result<Transition> {
        let (started_tx, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        self.server.test_gate_workspace_topology_transition(started_tx, release_rx);
        let server = Arc::clone(&self.server);
        let sibling = self.sibling.clone();
        let worker = std::thread::Builder::new().name("critic-topology-transition".into()).spawn(
            move || {
                server
                    .handle_did_change_workspace_folders(Some(json!({ "event": {
                        "added": [{ "uri": sibling, "name": "unrelated-sibling" }], "removed": [],
                    }})))
                    .map_err(anyhow::Error::from)
            },
        )?;
        let transition = Transition { release: Some(release), worker: Some(worker) };
        if let Err(error) = started_rx.recv_timeout(DEADLINE) {
            transition.finish()?;
            return Err(error).context("membership/config barrier not acknowledged");
        }
        Ok(transition)
    }
}

// Explicit finish propagates worker panics/errors. Drop only releases a blocked
// worker on an earlier fixture error; it does not manufacture a proof pass.
struct Transition {
    release: Option<mpsc::Sender<()>>,
    worker: Option<JoinHandle<Result<()>>>,
}

impl Transition {
    fn finish(mut self) -> Result<()> {
        let sent = self.release.take().context("missing transition release")?.send(());
        let joined = self
            .worker
            .take()
            .context("missing transition worker")?
            .join()
            .map_err(|_| anyhow!("workspace notification worker panicked"))?;
        sent.context("transition release failed")?;
        joined
    }
}

impl Drop for Transition {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn workspace_item<'a>(report: &'a Value, uri: &str) -> Result<&'a Value> {
    report
        .get("items")
        .and_then(Value::as_array)
        .context("workspace items missing")?
        .iter()
        .find(|item| item.get("uri").and_then(Value::as_str) == Some(uri))
        .with_context(|| format!("target {uri} omitted from workspace report: {report}"))
}

fn result_id(report: &Value) -> Result<&str> {
    report.get("resultId").and_then(Value::as_str).context("reusable result ID missing")
}

fn has_native(report: &Value, field: &str) -> bool {
    has_code(report, field, NATIVE_CODE)
}

fn has_code(report: &Value, field: &str, code: &str) -> bool {
    report.get(field).and_then(Value::as_array).is_some_and(|rows| {
        rows.iter().any(|row| row.get("code").and_then(Value::as_str) == Some(code))
    })
}

fn has_native_action(actions: &Value) -> bool {
    actions.as_array().is_some_and(|rows| {
        rows.iter().any(|action| {
            action.pointer("/diagnostics/0/code").and_then(Value::as_str) == Some(NATIVE_CODE)
        })
    })
}

fn has_any_native_action(actions: &Value) -> bool {
    actions.as_array().is_some_and(|rows| {
        rows.iter().any(|action| {
            action.get("diagnostics").and_then(Value::as_array).is_some_and(|diagnostics| {
                diagnostics.iter().any(|diagnostic| {
                    diagnostic
                        .get("code")
                        .and_then(Value::as_str)
                        .is_some_and(|code| code.starts_with("native."))
                })
            })
        })
    })
}

fn is_push(message: &Value, uri: &str) -> bool {
    message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
        && message.pointer("/params/uri").and_then(Value::as_str) == Some(uri)
}

fn unavailable(report: &Value, consumer: &str) -> Result<()> {
    ensure!(
        report.get("kind").and_then(Value::as_str) == Some("full")
            && report.get("resultId").is_none()
            && report.get("items").and_then(Value::as_array).is_some_and(Vec::is_empty),
        "{consumer} retained native rows/reuse authority: {report}"
    );
    Ok(())
}

fn actions(server: &LspServer, uri: &str) -> Result<Value> {
    server.test_handle_code_action(Some(json!({
        "textDocument": { "uri": uri },
        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
        "context": { "diagnostics": [] },
    })))?.context("missing action response")
}

fn workspace_unavailable(report: &Value, uri: &str) -> Result<()> {
    let items = report.get("items").and_then(Value::as_array).context("workspace items missing")?;
    for item in items {
        if item.get("uri").and_then(Value::as_str) == Some(uri) {
            unavailable(item, "workspace")?;
        }
    }
    Ok(())
}

#[test]
fn native_critic_sibling_transition_withholds_all_consumers_then_recovers() -> Result<()> {
    let mut fixture = Fixture::new()?;
    std::fs::write(
        fixture.sibling_dir.path().join(".perl-lsp.toml"),
        "[diagnostics]\nperlcritic_severity = 1\n",
    )?;
    let previous = fixture.assert_positive()?;
    let accepted = fixture.server.capture_accepted_critic(&fixture.document);
    ensure!(accepted.owning_root().is_some(), "fixture has no owning root authority");
    let old_identity = fixture.server.workspace_identity_generation.load(Ordering::SeqCst);
    let old_topology = fixture.server.workspace_topology_generation.load(Ordering::SeqCst);
    let before = fixture.push_count()?;
    let premature_recovery = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&premature_recovery);
    let stable = Arc::clone(&fixture.server.workspace_topology_stable);
    let topology = Arc::clone(&fixture.server.workspace_topology_generation);
    let identity = Arc::clone(&fixture.server.workspace_identity_generation);
    let publication = Arc::clone(&fixture.server.workspace_identity_lock);
    *fixture.server.diagnostic_after_snapshot_hook.lock() = Some(Box::new(move || {
        let _guard = publication.lock();
        if stable.load(Ordering::SeqCst)
            && topology.load(Ordering::SeqCst) != old_topology
            && identity.load(Ordering::SeqCst) == old_identity
        {
            observed.store(true, Ordering::SeqCst);
        }
    }));
    let transition = fixture.begin_transition()?;
    let during = fixture.assert_unavailable(&previous);
    transition.finish()?;
    *fixture.server.diagnostic_after_snapshot_hook.lock() = None;
    during?;
    ensure!(
        !premature_recovery.load(Ordering::SeqCst),
        "recovery began with stable topology and old aggregate epoch"
    );
    ensure!(
        fixture.server.workspace_topology_stable.load(Ordering::SeqCst),
        "successful transition unavailable"
    );
    ensure!(
        fixture.server.workspace_identity_generation.load(Ordering::SeqCst) != old_identity,
        "aggregate identity did not advance"
    );
    let fresh = fixture.server.capture_accepted_critic(&fixture.document);
    ensure!(
        fresh.owning_root() == accepted.owning_root(),
        "target root rebound during sibling transition"
    );
    ensure!(fresh != accepted, "sibling policy did not move aggregate authority");
    fixture.assert_automatic_recovery_push(before)?;
    fixture.assert_positive()?;
    Ok(())
}

#[test]
fn native_critic_failed_sibling_config_withholds_all_consumers_until_recovery() -> Result<()> {
    let mut fixture = Fixture::new()?;
    let previous = fixture.assert_positive()?;
    let config = fixture.sibling_dir.path().join(".perl-lsp.toml");
    std::fs::write(&config, "[critic\n")?;
    let transition = fixture.begin_transition()?;
    transition.finish()?;
    ensure!(
        !fixture.server.workspace_topology_stable.load(Ordering::SeqCst),
        "failed configuration became current"
    );
    fixture.assert_unavailable(&previous)?;
    std::fs::write(config, "[diagnostics]\nperlcritic_severity = 1\n")?;
    fixture.server.handle_did_change_workspace_folders(Some(json!({ "event": {
        "removed": [{ "uri": fixture.sibling }], "added": [],
    }})))?;
    let before = fixture.push_count()?;
    let transition = fixture.begin_transition()?;
    transition.finish()?;
    fixture.assert_automatic_recovery_push(before)?;
    fixture.assert_positive()?;
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum Consumer {
    Push,
    Document,
    Workspace,
    Actions,
}

fn old_subject_rejected(consumer: Consumer) -> Result<()> {
    let mut fixture = Fixture::new()?;
    fixture.assert_positive()?;
    let accepted = fixture.server.capture_accepted_critic(&fixture.document);
    ensure!(accepted.owning_root().is_some(), "staged fixture has no owning root");
    let old_identity = fixture.server.workspace_identity_generation.load(Ordering::SeqCst);
    let old_topology = fixture.server.workspace_topology_generation.load(Ordering::SeqCst);
    let before =
        fixture.messages()?.iter().filter(|message| is_push(message, &fixture.document)).count();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&calls);
    // Push/workspace have an early snapshot hook plus the complete-payload
    // hook. Document/actions expose only the completed projection barrier.
    let selected_call = match consumer {
        Consumer::Push | Consumer::Workspace => 1,
        _ => 0,
    };
    let hook_error = Arc::new(Mutex::new(None));
    let reported_error = Arc::clone(&hook_error);
    *fixture.server.diagnostic_after_snapshot_hook.lock() = Some(Box::new(move || {
        if observed_calls.fetch_add(1, Ordering::SeqCst) == selected_call {
            let result = started_tx.send(()).map_err(anyhow::Error::from).and_then(|()| {
                release_rx.lock().recv_timeout(DEADLINE).map_err(anyhow::Error::from)
            });
            if let Err(error) = result {
                *reported_error.lock() = Some(error.to_string());
            }
        }
    }));
    let server = Arc::clone(&fixture.server);
    let uri = fixture.document.clone();
    let request = std::thread::Builder::new().name(format!("critic-{consumer:?}")).spawn(
        move || -> Result<Value> {
            match consumer {
                Consumer::Push => {
                    server.publish_diagnostics(&uri);
                    Ok(Value::Null)
                }
                Consumer::Document => server
                    .test_handle_document_diagnostic(Some(
                        json!({ "textDocument": { "uri": uri } }),
                    ))?
                    .context("old document report missing"),
                Consumer::Workspace => server
                    .test_handle_workspace_diagnostic(Some(json!({})))?
                    .context("old workspace report missing"),
                Consumer::Actions => actions(&server, &uri),
            }
        },
    )?;
    let started = started_rx.recv_timeout(DEADLINE);
    // Release/join the request even if either barrier cannot be installed.
    let transition = if started.is_ok() { Some(fixture.begin_transition()) } else { None };
    let premises = (|| -> Result<()> {
        ensure!(
            calls.load(Ordering::SeqCst) == selected_call + 1,
            "selected completed-payload callback not acknowledged"
        );
        ensure!(
            fixture.server.workspace_topology_generation.load(Ordering::SeqCst) != old_topology,
            "sibling membership did not move topology"
        );
        ensure!(
            fixture.server.workspace_identity_generation.load(Ordering::SeqCst) == old_identity,
            "aggregate epoch moved before config installation"
        );
        ensure!(
            fixture.server.capture_accepted_critic(&fixture.document) == accepted,
            "root/config policy moved before installation; wrong staged-subject premise"
        );
        Ok(())
    })();
    let released = release_tx.send(());
    let response = request.join().map_err(|_| anyhow!("{consumer:?} request worker panicked"))?;
    *fixture.server.diagnostic_after_snapshot_hook.lock() = None;
    let during_frames = fixture.messages()?;
    let recovery_baseline =
        during_frames.iter().filter(|message| is_push(message, &fixture.document)).count();
    if let Some(transition) = transition {
        transition?.finish()?;
    }
    started.context("old-subject barrier not acknowledged")?;
    released.context("old-subject release failed")?;
    premises?;
    ensure!(hook_error.lock().is_none(), "old-subject gate failed: {:?}", hook_error.lock());
    let response = response?;
    match consumer {
        Consumer::Push => ensure!(
            during_frames.iter().filter(|message| is_push(message, &fixture.document)).count()
                == before,
            "old push subject emitted a frame"
        ),
        Consumer::Document => unavailable(&response, "old document")?,
        Consumer::Workspace => workspace_unavailable(&response, &fixture.document)?,
        Consumer::Actions => ensure!(
            !has_any_native_action(&response),
            "old action subject published native edits: {response}"
        ),
    }
    fixture.assert_automatic_recovery_push(recovery_baseline)?;
    fixture.assert_positive()?;
    Ok(())
}

#[test]
fn native_critic_staged_push_subject_rejected_after_sibling_membership_moves() -> Result<()> {
    old_subject_rejected(Consumer::Push)
}
#[test]
fn native_critic_staged_document_subject_rejected_after_sibling_membership_moves() -> Result<()> {
    old_subject_rejected(Consumer::Document)
}
#[test]
fn native_critic_staged_workspace_subject_rejected_after_sibling_membership_moves() -> Result<()> {
    old_subject_rejected(Consumer::Workspace)
}
#[test]
fn native_critic_staged_actions_rejected_after_sibling_membership_moves() -> Result<()> {
    old_subject_rejected(Consumer::Actions)
}
