//! Real framed LSP close/discard journeys against Cargo's exact public binary.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::TempDir;

const DISK: &str = "package Journey;\nuse strict;\nuse warnings;\nsub disk_only { 1 }\n1;\n";
const BUFFER: &str =
    "package Journey;\nuse strict;\nuse warnings;\nmy $label = \"😀\"; sub buffer_only { 2 }\n1;\n";
const CURRENT_DISK: &str =
    "package Journey;\nuse strict;\nuse warnings;\n\nsub current_disk { 3 }\n1;\n";

fn timeout() -> Duration {
    Duration::from_secs(20)
}

fn uri(path: &Path) -> Result<String> {
    url::Url::from_file_path(path)
        .map(String::from)
        .map_err(|()| anyhow::anyhow!("fixture path cannot form file URI: {}", path.display()))
}

struct Project {
    root: TempDir,
    path: PathBuf,
    uri: String,
}

impl Project {
    fn new(name: &str, text: Option<&str>) -> Result<Self> {
        let root = tempfile::tempdir()?;
        let path = root.path().join(name);
        if let Some(text) = text {
            std::fs::write(&path, text)?;
        }
        let uri = uri(&path)?;
        Ok(Self { root, path, uri })
    }

    fn start(&self) -> Result<RealProcessClient> {
        // This is a public runtime option, preserving normal eager discovery even
        // when a surrounding CI job selects E2E timing defaults.
        let mut client =
            RealProcessClient::spawn_exact_with_env(&[("PERL_LSP_EAGER_WORKSPACE_INDEXING", "1")])?;
        let root_uri = uri(self.root.path())?;
        let response = client.request(
            json!("initialize"),
            "initialize",
            json!({
                "processId": null,
                "rootUri": root_uri,
                "workspaceFolders": [{"uri": root_uri, "name": "close-journey"}],
                "capabilities": {"general": {"positionEncodings": ["utf-16"]}}
            }),
            timeout(),
        )?;
        ensure!(
            response.pointer("/result/capabilities/textDocumentSync/change") == Some(&json!(1)),
            "journey must use the advertised FULL sync envelope: {response}"
        );
        client.notify("initialized", json!({}))?;
        let deadline = Instant::now() + timeout();
        loop {
            let notification = client.receive_server_notification(
                "perl-lsp/index-ready",
                deadline.saturating_duration_since(Instant::now()),
            )?;
            if notification.pointer("/params/ready") == Some(&json!(true)) {
                break;
            }
        }
        Ok(client)
    }

    fn open(&self, client: &mut RealProcessClient, text: &str) -> Result<()> {
        client.notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": self.uri, "languageId": "perl", "version": 1, "text": text
            }}),
        )?;
        self.ready_for(client, &self.uri, 1)
    }

    fn edit(&self, client: &mut RealProcessClient) -> Result<()> {
        client.notify(
            "textDocument/didChange",
            json!({"textDocument": {"uri": self.uri, "version": 2},
                "contentChanges": [{"text": BUFFER}]}),
        )?;
        self.ready_for(client, &self.uri, 2)?;
        // Readiness covers parser effects, so prove the actual indexed buffer
        // result too. Literal UTF-16 range: emoji is two units; sub starts at 18.
        self.assert_symbol(client, "buffer_only", Some((3, 18, 39)))?;
        self.assert_symbol(client, "disk_only", None)?;
        Ok(())
    }

    fn ready_for(
        &self,
        client: &mut RealProcessClient,
        document_uri: &str,
        generation: u64,
    ) -> Result<()> {
        let deadline = Instant::now() + timeout();
        loop {
            let notification = client.receive_server_notification(
                "perl-lsp/active-document-ready",
                deadline.saturating_duration_since(Instant::now()),
            )?;
            if notification.pointer("/params/uri") == Some(&json!(document_uri))
                && notification.pointer("/params/generation") == Some(&json!(generation))
            {
                break;
            }
        }
        // Consume the parser diagnostic publication before didClose, making
        // the next diagnostic clear an acknowledgement of the close handoff.
        let diagnostic =
            client.receive_server_notification("textDocument/publishDiagnostics", timeout())?;
        ensure!(diagnostic.pointer("/params/uri") == Some(&json!(document_uri)), "{diagnostic}");
        ensure!(
            diagnostic.pointer("/params/version") == Some(&json!(generation)),
            "fixture version and accepted generation must match: {diagnostic}"
        );
        Ok(())
    }

    fn open_caller(&self, client: &mut RealProcessClient) -> Result<String> {
        let caller = self.root.path().join("caller.pl");
        let text = "use Journey;\nJourney::disk_only();\n";
        std::fs::write(&caller, text)?;
        let caller_uri = uri(&caller)?;
        client.notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": caller_uri, "languageId": "perl", "version": 1, "text": text
            }}),
        )?;
        self.ready_for(client, &caller_uri, 1)?;
        Ok(caller_uri)
    }

    fn assert_disk_definition(
        &self,
        client: &mut RealProcessClient,
        caller_uri: &str,
    ) -> Result<()> {
        let response = client.request(
            json!("disk-definition"),
            "textDocument/definition",
            json!({"textDocument": {"uri": caller_uri},
                "position": {"line": 1, "character": 12}}),
            timeout(),
        )?;
        ensure!(
            response.get("result")
                == Some(&json!([{
                    "uri": self.uri, "range": {
                        "start": {"line": 3, "character": 0},
                        "end": {"line": 3, "character": 19}
                    }
                }])),
            "cross-file definition must return the independent disk declaration: {response}"
        );
        Ok(())
    }

    fn close(&self, client: &mut RealProcessClient) -> Result<()> {
        self.close_as(client, &self.uri)
    }

    fn close_as(&self, client: &mut RealProcessClient, close_uri: &str) -> Result<()> {
        client.notify("textDocument/didClose", json!({"textDocument": {"uri": close_uri}}))?;
        let deadline = Instant::now() + timeout();
        loop {
            let notification = client.receive_server_notification(
                "textDocument/publishDiagnostics",
                deadline.saturating_duration_since(Instant::now()),
            )?;
            ensure!(
                notification.pointer("/params/uri") == Some(&json!(close_uri)),
                "{notification}"
            );
            // A save can publish versioned diagnostics immediately before close.
            // Only close emits this exact unversioned clear for this fixture.
            if notification.pointer("/params/version").is_none() {
                ensure!(
                    notification.get("params")
                        == Some(&json!({"uri": close_uri, "diagnostics": []})),
                    "close must clear diagnostics after the handoff: {notification}"
                );
                break;
            }
        }
        Ok(())
    }

    fn assert_symbol(
        &self,
        client: &mut RealProcessClient,
        name: &str,
        range: Option<(u32, u32, u32)>,
    ) -> Result<Value> {
        let response = client.request(
            json!(format!("symbol-{name}")),
            "workspace/symbol",
            json!({"query": name}),
            timeout(),
        )?;
        let symbols = response
            .get("result")
            .and_then(Value::as_array)
            .with_context(|| format!("workspace symbol request failed: {response}"))?;
        if let Some((line, start, end)) = range {
            ensure!(symbols.len() == 1, "expected exactly one {name}: {response}");
            ensure!(symbols[0].get("name") == Some(&json!(name)), "{response}");
            ensure!(
                symbols[0].get("location")
                    == Some(&json!({
                        "uri": self.uri, "range": {
                            "start": {"line": line, "character": start},
                            "end": {"line": line, "character": end}
                        }
                    })),
                "expected independent literal location for {name}: {response}"
            );
        } else {
            ensure!(symbols.is_empty(), "discarded symbol {name} remains: {response}");
        }
        Ok(Value::Array(symbols.clone()))
    }
}

fn shutdown(mut client: RealProcessClient) -> Result<()> {
    let response = client.request(json!("shutdown"), "shutdown", Value::Null, timeout())?;
    ensure!(response.get("result") == Some(&Value::Null), "{response}");
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(timeout())?.success(), "clean shutdown failed");
    client.assert_transport_clean()
}

#[test]
fn discard_close_restores_disk_and_matches_restart() -> Result<()> {
    let project = Project::new("Journey.pm", Some(DISK))?;
    let mut client = project.start()?;
    let caller_uri = project.open_caller(&mut client)?;
    project.assert_symbol(&mut client, "disk_only", Some((3, 0, 19)))?;
    project.assert_disk_definition(&mut client, &caller_uri)?;
    project.open(&mut client, DISK)?;
    project.edit(&mut client)?;
    project.close(&mut client)?;
    let closed = project.assert_symbol(&mut client, "disk_only", Some((3, 0, 19)))?;
    project.assert_symbol(&mut client, "buffer_only", None)?;
    project.assert_disk_definition(&mut client, &caller_uri)?;
    // A successor version-1 session must acquire fresh buffer authority.
    project.open(&mut client, BUFFER)?;
    project.assert_symbol(&mut client, "buffer_only", Some((3, 18, 39)))?;
    project.close(&mut client)?;
    project.assert_symbol(&mut client, "disk_only", Some((3, 0, 19)))?;
    project.assert_disk_definition(&mut client, &caller_uri)?;
    shutdown(client)?;
    let mut restarted = project.start()?;
    let caller_uri = project.open_caller(&mut restarted)?;
    let after_restart = project.assert_symbol(&mut restarted, "disk_only", Some((3, 0, 19)))?;
    ensure!(closed == after_restart, "close and restart must agree on disk authority");
    project.assert_symbol(&mut restarted, "buffer_only", None)?;
    project.assert_disk_definition(&mut restarted, &caller_uri)?;
    shutdown(restarted)
}

#[test]
fn close_reads_current_disk_without_watcher_notification() -> Result<()> {
    let project = Project::new("Journey.pm", Some(DISK))?;
    let mut client = project.start()?;
    project.open(&mut client, DISK)?;
    project.edit(&mut client)?;
    std::fs::write(&project.path, CURRENT_DISK)?;
    project.close(&mut client)?;
    project.assert_symbol(&mut client, "current_disk", Some((4, 0, 22)))?;
    project.assert_symbol(&mut client, "disk_only", None)?;
    project.assert_symbol(&mut client, "buffer_only", None)?;
    shutdown(client)
}

#[test]
fn unchanged_and_saved_close_keep_real_disk_symbols() -> Result<()> {
    let project = Project::new("Journey.pm", Some(DISK))?;
    let mut client = project.start()?;
    project.open(&mut client, DISK)?;
    project.close(&mut client)?;
    project.assert_symbol(&mut client, "disk_only", Some((3, 0, 19)))?;
    project.open(&mut client, DISK)?;
    project.edit(&mut client)?;
    std::fs::write(&project.path, BUFFER)?;
    client.notify(
        "textDocument/didSave",
        json!({"textDocument": {"uri": project.uri}, "text": BUFFER}),
    )?;
    project.close(&mut client)?;
    project.assert_symbol(&mut client, "buffer_only", Some((3, 18, 39)))?;
    project.assert_symbol(&mut client, "disk_only", None)?;
    shutdown(client)
}

#[test]
fn close_missing_or_nonregular_backing_clears_buffer_facts() -> Result<()> {
    for nonregular in [false, true] {
        let project = Project::new("Journey.pm", Some(DISK))?;
        let mut client = project.start()?;
        project.open(&mut client, DISK)?;
        project.edit(&mut client)?;
        std::fs::remove_file(&project.path)?;
        if nonregular {
            std::fs::create_dir(&project.path)?;
        }
        project.close(&mut client)?;
        project.assert_symbol(&mut client, "buffer_only", None)?;
        project.assert_symbol(&mut client, "disk_only", None)?;
        shutdown(client)?;
    }
    Ok(())
}

#[test]
fn close_reclassifies_extensionless_disk_and_removes_virtual_symbols() -> Result<()> {
    for backed in [false, true] {
        let initial = format!("#!/usr/bin/env perl\n{DISK}");
        let project = Project::new("journey", backed.then_some(initial.as_str()))?;
        let mut client = project.start()?;
        project.open(&mut client, &initial)?;
        project.edit(&mut client)?;
        if backed {
            std::fs::write(&project.path, "plain text artifact\n")?;
        }
        project.close(&mut client)?;
        project.assert_symbol(&mut client, "buffer_only", None)?;
        project.assert_symbol(&mut client, "disk_only", None)?;
        shutdown(client)?;
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn close_with_writerless_fifo_does_not_block_or_retain_buffer() -> Result<()> {
    let project = Project::new("Journey.pm", Some(DISK))?;
    let mut client = project.start()?;
    project.open(&mut client, DISK)?;
    project.edit(&mut client)?;
    std::fs::remove_file(&project.path)?;
    // Same real native fixture utility used by the workspace scan FIFO tests.
    // No writer ever opens the pipe: an unchecked disk read would hang here.
    let status = std::process::Command::new("mkfifo").arg(&project.path).status()?;
    ensure!(status.success(), "mkfifo could not create the close fixture");
    project.close(&mut client)?;
    project.assert_symbol(&mut client, "buffer_only", None)?;
    project.assert_symbol(&mut client, "disk_only", None)?;
    shutdown(client)
}

#[test]
fn close_refuses_oversized_and_binary_disk_replacements() -> Result<()> {
    // The normal public default is 1 MiB. Use short comment lines so the
    // oversized case exercises total size rather than a per-line bound.
    let oversized = format!("sub guarded_disk {{ 1 }}\n{}", "# padding\n".repeat(110_000));
    let binary = format!("sub guarded_disk {{ 1 }}\n{}", "\0".repeat(500));
    for text in [oversized, binary] {
        let project = Project::new("Journey.pm", Some(DISK))?;
        let mut client = project.start()?;
        project.open(&mut client, DISK)?;
        project.edit(&mut client)?;
        std::fs::write(&project.path, text)?;
        project.close(&mut client)?;
        project.assert_symbol(&mut client, "guarded_disk", None)?;
        project.assert_symbol(&mut client, "buffer_only", None)?;
        project.assert_symbol(&mut client, "disk_only", None)?;
        shutdown(client)?;
    }
    Ok(())
}

#[test]
fn close_decodes_utf16_disk_and_preserves_binary_data_sections() -> Result<()> {
    for utf16 in [false, true] {
        let project = Project::new("Journey.pm", Some(DISK))?;
        let mut client = project.start()?;
        project.open(&mut client, DISK)?;
        project.edit(&mut client)?;
        if utf16 {
            let mut bytes = vec![0xff, 0xfe];
            bytes.extend(CURRENT_DISK.encode_utf16().flat_map(u16::to_le_bytes));
            std::fs::write(&project.path, bytes)?;
        } else {
            let text = format!("{CURRENT_DISK}__DATA__\n{}", "\0".repeat(500));
            std::fs::write(&project.path, text)?;
        }
        project.close(&mut client)?;
        project.assert_symbol(&mut client, "current_disk", Some((4, 0, 22)))?;
        project.assert_symbol(&mut client, "buffer_only", None)?;
        project.assert_symbol(&mut client, "disk_only", None)?;
        shutdown(client)?;
    }
    Ok(())
}

/// Compatibility close spelling: didOpen/edit use proper file URIs. Raw-path
/// didOpen is rejected; the existing close eviction accepts equivalent paths.
#[test]
fn absolute_path_close_alias_restores_canonical_disk_symbols() -> Result<()> {
    let project = Project::new("Journey.pm", Some(DISK))?;
    let mut client = project.start()?;
    let caller_uri = project.open_caller(&mut client)?;
    project.assert_symbol(&mut client, "disk_only", Some((3, 0, 19)))?;
    project.open(&mut client, DISK)?;
    project.edit(&mut client)?;
    let close_uri = project.path.to_str().context("fixture path must be UTF-8")?;
    project.close_as(&mut client, close_uri)?;
    project.assert_symbol(&mut client, "disk_only", Some((3, 0, 19)))?;
    project.assert_symbol(&mut client, "buffer_only", None)?;
    project.assert_disk_definition(&mut client, &caller_uri)?;
    shutdown(client)
}

#[test]
fn encoded_file_uri_close_alias_restores_canonical_disk_symbols() -> Result<()> {
    let project = Project::new("Journey.pm", Some(DISK))?;
    let mut client = project.start()?;
    project.open(&mut client, DISK)?;
    project.edit(&mut client)?;
    let close_uri = project.uri.replace("/Journey.pm", "/%4Aourney.pm");
    project.close_as(&mut client, &close_uri)?;
    project.assert_symbol(&mut client, "disk_only", Some((3, 0, 19)))?;
    project.assert_symbol(&mut client, "buffer_only", None)?;
    shutdown(client)
}
