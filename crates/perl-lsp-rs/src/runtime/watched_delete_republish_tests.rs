//! Watched-file / file-operation DELETED republish contract (#17332).
//!
//! Internal proof for the external-mutation republish surface: when a
//! dependency file is deleted (watched `didChangeWatchedFiles` DELETED, or a
//! client `workspace/didDeleteFiles` event), eviction drops the dependency's
//! indexed facts, so open consumer buffers whose diagnostics resolved through
//! it must receive a fresh `textDocument/publishDiagnostics` push with no
//! buffer edit in between.
#![cfg(feature = "workspace")]

use super::{LspServer, WorkspaceFolderState};
use parking_lot::Mutex;
use perl_lsp_rs_core::transport::framing::ContentLengthFramer;
use serde_json::{Value, json};
use std::io::{self, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
struct OutputCapture {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl OutputCapture {
    fn messages(&self) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        let bytes = self.buffer.lock().clone();
        let mut framer = ContentLengthFramer::new();
        framer.push(&bytes);
        let mut messages = Vec::new();
        while let Some(body) = framer.try_next()? {
            messages.push(serde_json::from_slice::<Value>(&body)?);
        }
        Ok(messages)
    }
}

impl Write for OutputCapture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.lock().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn server_with_output_capture() -> (LspServer, OutputCapture) {
    let output = OutputCapture::default();
    let server = LspServer::with_output(Arc::new(Mutex::new(
        Box::new(output.clone()) as Box<dyn Write + Send>
    )));
    (server, output)
}

/// Shared fixture for the #17332 regression tests: a workspace whose open
/// consumer `app.pl` resolves `use Local::Alpha` through `lib/Local/Alpha.pm`,
/// mirroring the issue's fixture sequence. Returns the capture buffer, the
/// consumer URI, the dependency URI and path.
#[cfg(feature = "workspace")]
fn open_consumer_fixture() -> Result<
    (LspServer, OutputCapture, String, String, std::path::PathBuf, tempfile::TempDir),
    Box<dyn std::error::Error>,
> {
    let (server, output) = server_with_output_capture();
    let dir = tempfile::tempdir()?;
    std::fs::create_dir_all(dir.path().join("lib").join("Local"))?;
    let alpha_path = dir.path().join("lib").join("Local").join("Alpha.pm");
    std::fs::write(&alpha_path, "package Local::Alpha;\nsub helper { 1 }\n1;\n")?;

    let folder_uri =
        url::Url::from_directory_path(dir.path()).map_err(|_| "invalid folder URI")?.to_string();
    let alpha_uri =
        url::Url::from_file_path(&alpha_path).map_err(|_| "invalid Alpha URI")?.to_string();
    let app_uri = url::Url::from_file_path(dir.path().join("app.pl"))
        .map_err(|_| "invalid app URI")?
        .to_string();
    // Explicit non-probing include state keeps the module resolution of the
    // fixture deterministic: `Local::Alpha` exists exactly while the fixture
    // file does.
    let mut config = perl_lsp_rs_core::config::WorkspaceConfig::default();
    config.include_paths = vec!["lib".to_string()];
    config.use_perl5lib = false;
    config.use_system_inc = false;
    server.workspace_folders.lock().push(
        WorkspaceFolderState::new(folder_uri)
            .with_path(dir.path().to_path_buf())
            .with_effective_workspace_config(config),
    );

    let source = "use strict;\nuse warnings;\nuse Local::Alpha;\n\
                  print Local::Alpha::helper(), \"\\n\";\n";
    server.test_apply_did_open(&app_uri, source, 1)?;
    server.publish_diagnostics(&app_uri);

    // The TempDir guard travels with the fixture: dropping it would delete
    // the on-disk dependency the test still needs to remove explicitly.
    Ok((server, output, app_uri, alpha_uri, alpha_path, dir))
}

/// #17332 regression: a watched `didChangeWatchedFiles` DELETED event for a
/// dependency must push fresh `textDocument/publishDiagnostics` to the
/// still-open consumer buffer. Before the repair the eviction removed the
/// dependency's index facts but nothing republished, so the editor kept the
/// stale clean display until the user edited the buffer — even though pull
/// diagnostics already returned `Module 'Local::Alpha' not found`.
#[cfg(feature = "workspace")]
#[test]
fn watched_dependency_delete_republishes_open_consumer_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, output, app_uri, alpha_uri, alpha_path, _dir) = open_consumer_fixture()?;

    // The publish URI spelling is the document map key (drive letter
    // lowercased on Windows), which can differ from the didOpen spelling.
    let publish_keys = [app_uri.clone(), server.normalize_uri_key(&app_uri)];
    let is_publish_for = |message: &Value| -> bool {
        message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
            && message
                .pointer("/params/uri")
                .and_then(Value::as_str)
                .is_some_and(|uri| publish_keys.iter().any(|key| key == uri))
    };
    let has_pl701 = |message: &Value| -> bool {
        message.pointer("/params/diagnostics").and_then(Value::as_array).is_some_and(
            |diagnostics| {
                diagnostics.iter().any(|diagnostic| diagnostic.get("code") == Some(&json!("PL701")))
            },
        )
    };

    // Baseline: with the dependency on disk the open consumer resolves, so
    // the published set must be clean of PL701 before the delete arrives.
    let deadline = Instant::now() + Duration::from_secs(5);
    let baseline_len = loop {
        let messages = output.messages()?;
        let clean = messages.iter().any(|message| is_publish_for(message) && !has_pl701(message));
        if clean {
            break messages.len();
        }
        assert!(
            Instant::now() < deadline,
            "baseline publish for the consumer never settled clean: {messages:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    };

    // Delete the dependency on disk, then deliver the watched DELETED event.
    std::fs::remove_file(&alpha_path)?;
    server.handle_did_change_watched_files(Some(json!({
        "changes": [{ "uri": alpha_uri, "type": 3 }]
    })))?;

    // The open consumer must receive a fresh publish carrying the
    // module-not-found diagnostic, with no buffer edit in between.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let messages = output.messages()?;
        let republished = messages
            .iter()
            .skip(baseline_len)
            .any(|message| is_publish_for(message) && has_pl701(message));
        if republished {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "watched DELETED never republished the open consumer: tail {messages:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

/// #17332 regression, symmetric delivery path: a client
/// `workspace/didDeleteFiles` file-operation event for a dependency must
/// also push fresh diagnostics to the still-open consumer buffer.
#[cfg(feature = "workspace")]
#[test]
fn file_operation_delete_republishes_open_consumer_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, output, app_uri, alpha_uri, alpha_path, _dir) = open_consumer_fixture()?;

    // The publish URI spelling is the document map key (drive letter
    // lowercased on Windows), which can differ from the didOpen spelling.
    let publish_keys = [app_uri.clone(), server.normalize_uri_key(&app_uri)];
    let is_publish_for = |message: &Value| -> bool {
        message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
            && message
                .pointer("/params/uri")
                .and_then(Value::as_str)
                .is_some_and(|uri| publish_keys.iter().any(|key| key == uri))
    };
    let has_pl701 = |message: &Value| -> bool {
        message.pointer("/params/diagnostics").and_then(Value::as_array).is_some_and(
            |diagnostics| {
                diagnostics.iter().any(|diagnostic| diagnostic.get("code") == Some(&json!("PL701")))
            },
        )
    };

    let deadline = Instant::now() + Duration::from_secs(5);
    let baseline_len = loop {
        let messages = output.messages()?;
        let clean = messages.iter().any(|message| is_publish_for(message) && !has_pl701(message));
        if clean {
            break messages.len();
        }
        assert!(
            Instant::now() < deadline,
            "baseline publish for the consumer never settled clean: {messages:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    };

    std::fs::remove_file(&alpha_path)?;
    server.handle_did_delete_files(Some(json!({
        "files": [{ "uri": alpha_uri }]
    })))?;

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let messages = output.messages()?;
        let republished = messages
            .iter()
            .skip(baseline_len)
            .any(|message| is_publish_for(message) && has_pl701(message));
        if republished {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "didDeleteFiles never republished the open consumer: tail {messages:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
