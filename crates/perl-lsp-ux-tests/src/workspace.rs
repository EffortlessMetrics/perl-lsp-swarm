//! Temporary workspace helpers for UX scenario tests.
//!
//! `FakeWorkspace` creates an isolated temporary directory with Perl project
//! structure and provides helpers for writing source files and producing
//! `file://` URIs suitable for the LSP protocol.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::sync::Mutex;
use tempfile::TempDir;
use url::Url;

/// An isolated temporary directory that acts as a Perl project root.
pub struct FakeWorkspace {
    /// The underlying temporary directory (auto-deleted on drop).
    pub dir: TempDir,
    /// The `file://` URI of the workspace root, suitable for `rootUri` in
    /// LSP `initialize` requests.
    pub root_uri: String,
}

impl FakeWorkspace {
    /// Create a new empty workspace in a fresh temporary directory.
    pub fn new() -> Result<Self> {
        let dir = TempDir::new().context("Failed to create temporary workspace directory")?;
        let root_uri = Url::from_directory_path(dir.path())
            .map_err(|_| anyhow::anyhow!("Failed to produce file:// URI for workspace root"))?
            .to_string();
        Ok(Self { dir, root_uri })
    }

    /// Write a file at `relative_path` (e.g., `"lib/Foo.pm"`) with `content`.
    /// Creates intermediate directories automatically.
    pub fn write(&self, relative_path: &str, content: &str) -> Result<()> {
        let path = self.dir.path().join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create dirs for {:?}", parent))?;
        }
        fs::write(&path, content)
            .with_context(|| format!("Failed to write workspace file {:?}", path))?;
        Ok(())
    }

    /// Ensure a directory exists at `relative_path`.
    pub fn ensure_dir(&self, relative_path: &str) -> Result<()> {
        let path = self.dir.path().join(relative_path);
        fs::create_dir_all(&path)
            .with_context(|| format!("Failed to create workspace directory {:?}", path))?;
        Ok(())
    }

    /// Delete a file at `relative_path` if it exists.
    pub fn delete(&self, relative_path: &str) -> Result<()> {
        let path = self.dir.path().join(relative_path);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete workspace file {:?}", path))?;
        }
        Ok(())
    }

    /// Get the full `file://` URI for a relative path.
    pub fn uri(&self, relative_path: &str) -> String {
        let path = self.dir.path().join(relative_path);
        match Url::from_file_path(&path) {
            Ok(url) => url.to_string(),
            Err(_) => {
                // Fallback: manually construct the URI (handles edge cases on Windows).
                format!("{}/{}", self.root_uri.trim_end_matches('/'), relative_path)
            }
        }
    }

    /// Get the full `file://` URI for a relative directory path.
    pub fn dir_uri(&self, relative_path: &str) -> Result<String> {
        let path = self.dir.path().join(relative_path);
        Url::from_directory_path(&path)
            .map(|url| url.to_string())
            .map_err(|_| anyhow::anyhow!("Failed to produce file:// URI for directory {:?}", path))
    }

    /// Get the filesystem path for a relative path.
    pub fn path(&self, relative_path: &str) -> std::path::PathBuf {
        self.dir.path().join(relative_path)
    }

    /// Write a minimal `cpanfile` to the workspace root.
    /// Useful for scenarios that test multi-file workspace detection.
    pub fn write_cpanfile(&self, requires: &[(&str, &str)]) -> Result<()> {
        let mut content = String::new();
        for (module, version) in requires {
            content.push_str(&format!("requires '{}', '{}';\n", module, version));
        }
        self.write("cpanfile", &content)
    }

    /// Seed a minimal Perl project with a single script and lib directory.
    pub fn seed_minimal_project(&self) -> Result<()> {
        self.write(
            "script.pl",
            "#!/usr/bin/env perl\nuse strict;\nuse warnings;\nprint \"hello\\n\";\n",
        )?;
        self.write(
            "lib/MyApp.pm",
            "package MyApp;\nuse strict;\nuse warnings;\nsub new { bless {}, shift }\n1;\n",
        )?;
        Ok(())
    }
}

impl crate::UxHarness {
    /// Open an editor buffer without changing its backing workspace file.
    ///
    /// This is the buffer-authoritative counterpart to [`crate::UxHarness::open_file`].
    /// It records version 1 only after `textDocument/didOpen` is written
    /// successfully and refuses to reset an already-open version owner.
    pub fn open_editor_buffer(&self, relative_path: &str, content: &str) -> Result<()> {
        let uri = self.workspace.uri(relative_path);
        open_tracked_document(&self.document_versions, &uri, || self.client.did_open(&uri, content))
    }

    /// Replace an open editor buffer without changing its backing workspace file.
    ///
    /// The next client version is committed to the harness map only after the
    /// `textDocument/didChange` notification is written successfully.
    pub fn change_editor_buffer_full(
        &self,
        relative_path: &str,
        updated_content: &str,
    ) -> Result<i32> {
        let uri = self.workspace.uri(relative_path);
        change_tracked_document(&self.document_versions, &uri, |version| {
            self.client.did_change_full(&uri, version, updated_content)
        })
    }

    /// Close an open editor buffer and retire its client-version owner.
    ///
    /// The map entry is removed only after `textDocument/didClose` is written
    /// successfully. This operation does not delete or rewrite the backing file.
    pub fn close_editor_buffer(&self, relative_path: &str) -> Result<()> {
        let uri = self.workspace.uri(relative_path);
        close_tracked_document(&self.document_versions, &uri, || {
            self.client.notify(
                "textDocument/didClose",
                json!({
                    "textDocument": {
                        "uri": uri
                    }
                }),
            )
        })
    }

    /// Return the client version currently owned for an open editor buffer.
    pub fn tracked_document_version(&self, relative_path: &str) -> Option<i32> {
        let uri = self.workspace.uri(relative_path);
        self.document_versions.lock().unwrap_or_else(|error| error.into_inner()).get(&uri).copied()
    }
}

fn open_tracked_document(
    versions: &Mutex<HashMap<String, i32>>,
    uri: &str,
    notify: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let mut versions = versions.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(version) = versions.get(uri) {
        bail!("editor buffer is already open at version {version}: {uri}");
    }
    // The version map stays untouched until the transport succeeds; a panic in
    // `notify` leaves the pre-notify state behind under the poisoned lock, so
    // every helper here fails closed for later callers.
    notify().with_context(|| format!("failed to open editor buffer: {uri}"))?;
    versions.insert(uri.to_string(), 1);
    Ok(())
}

fn change_tracked_document(
    versions: &Mutex<HashMap<String, i32>>,
    uri: &str,
    notify: impl FnOnce(i32) -> Result<()>,
) -> Result<i32> {
    let mut versions = versions.lock().unwrap_or_else(|error| error.into_inner());
    let current = versions
        .get(uri)
        .copied()
        .ok_or_else(|| anyhow!("cannot change editor buffer before open: {uri}"))?;
    let next = current
        .checked_add(1)
        .ok_or_else(|| anyhow!("editor buffer version overflow for {uri}: {current}"))?;
    notify(next)
        .with_context(|| format!("failed to change editor buffer at version {next}: {uri}"))?;
    versions.insert(uri.to_string(), next);
    Ok(next)
}

fn close_tracked_document(
    versions: &Mutex<HashMap<String, i32>>,
    uri: &str,
    notify: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let mut versions = versions.lock().unwrap_or_else(|error| error.into_inner());
    let version = versions
        .get(uri)
        .copied()
        .ok_or_else(|| anyhow!("cannot close editor buffer before open: {uri}"))?;
    notify().with_context(|| format!("failed to close editor buffer version {version}: {uri}"))?;
    versions.remove(uri);
    Ok(())
}

#[cfg(test)]
mod editor_buffer_version_tests {
    use super::{change_tracked_document, close_tracked_document, open_tracked_document};
    use anyhow::{Result, anyhow};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn version(versions: &Mutex<HashMap<String, i32>>, uri: &str) -> Option<i32> {
        versions.lock().unwrap_or_else(|error| error.into_inner()).get(uri).copied()
    }

    #[test]
    fn open_change_close_reopen_owns_versions_transactionally() -> Result<()> {
        let uri = "file:///workspace/live.pl";
        let versions = Mutex::new(HashMap::new());

        open_tracked_document(&versions, uri, || Ok(()))?;
        assert_eq!(version(&versions, uri), Some(1));

        let changed = change_tracked_document(&versions, uri, |next| {
            assert_eq!(next, 2);
            Ok(())
        })?;
        assert_eq!(changed, 2);
        assert_eq!(version(&versions, uri), Some(2));

        close_tracked_document(&versions, uri, || Ok(()))?;
        assert_eq!(version(&versions, uri), None);

        open_tracked_document(&versions, uri, || Ok(()))?;
        assert_eq!(version(&versions, uri), Some(1));
        assert_eq!(change_tracked_document(&versions, uri, |_| Ok(()))?, 2);
        Ok(())
    }

    #[test]
    fn failed_notifications_do_not_mutate_local_version_ownership() -> Result<()> {
        let uri = "file:///workspace/live.pl";
        let versions = Mutex::new(HashMap::new());

        assert!(open_tracked_document(&versions, uri, || Err(anyhow!("open transport"))).is_err());
        assert_eq!(version(&versions, uri), None);

        open_tracked_document(&versions, uri, || Ok(()))?;
        assert!(
            change_tracked_document(&versions, uri, |_| Err(anyhow!("change transport"))).is_err()
        );
        assert_eq!(version(&versions, uri), Some(1));

        assert!(
            close_tracked_document(&versions, uri, || Err(anyhow!("close transport"))).is_err()
        );
        assert_eq!(version(&versions, uri), Some(1));
        Ok(())
    }

    #[test]
    fn change_overflow_fails_without_mutating_version() -> Result<()> {
        let uri = "file:///workspace/live.pl";
        let versions = Mutex::new(HashMap::from([(uri.to_string(), i32::MAX)]));

        assert!(change_tracked_document(&versions, uri, |_| Ok(())).is_err());
        assert_eq!(version(&versions, uri), Some(i32::MAX));
        Ok(())
    }

    #[test]
    fn invalid_lifecycle_transitions_fail_without_notification() -> Result<()> {
        let uri = "file:///workspace/live.pl";
        let versions = Mutex::new(HashMap::new());
        let notifications = AtomicUsize::new(0);

        // Rejected transitions must fail before a notification closure is ever
        // invoked, so every accepted notification below is counted and the
        // counts around each rejection pin the no-send invariant.
        assert!(
            change_tracked_document(&versions, uri, count_notify_change(&notifications)).is_err()
        );
        assert!(close_tracked_document(&versions, uri, count_notify(&notifications)).is_err());
        assert_eq!(notifications.load(Ordering::SeqCst), 0);

        open_tracked_document(&versions, uri, count_notify(&notifications))?;
        assert_eq!(notifications.load(Ordering::SeqCst), 1);

        assert!(open_tracked_document(&versions, uri, count_notify(&notifications)).is_err());
        assert_eq!(notifications.load(Ordering::SeqCst), 1);

        close_tracked_document(&versions, uri, count_notify(&notifications))?;
        assert_eq!(notifications.load(Ordering::SeqCst), 2);

        assert!(close_tracked_document(&versions, uri, count_notify(&notifications)).is_err());
        assert_eq!(notifications.load(Ordering::SeqCst), 2);
        Ok(())
    }

    fn count_notify(notifications: &AtomicUsize) -> impl FnOnce() -> anyhow::Result<()> + '_ {
        move || {
            notifications.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn count_notify_change(
        notifications: &AtomicUsize,
    ) -> impl FnOnce(i32) -> anyhow::Result<()> + '_ {
        move |_| {
            notifications.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
}
