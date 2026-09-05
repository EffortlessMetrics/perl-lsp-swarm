//! Project-metadata invalidation for watched-file events (#13640).
//!
//! The watcher advertises a catch-all pattern (#13308/#14186) and the
//! source-index seam reclassifies each delivered path through one admission
//! authority. That authority only answers *is this Perl source?*, so a change
//! to `cpanfile`, `META.json`, `Makefile.PL`, `dist.ini`, `cpanfile.snapshot`,
//! or a Carmel/Carton marker reached the source index and never reached the
//! authority that owns dependency and environment facts
//! ([`WorkspaceFolderState::refresh_workspace_metadata`]).
//!
//! This module adds the second route. It is **additive**: classification here
//! decides whether a path feeds metadata facts, and never decides whether the
//! same path is Perl source. `Makefile.PL` and `Build.PL` carry the `.PL`
//! extension, which the shared admission authority matches case-insensitively
//! against `pl`, so they are genuinely both — they keep their source-index
//! facts *and* now refresh dependency facts. Suppressing source indexing for
//! them would be a regression, not a fix.
//!
//! # Coalescing
//!
//! Refresh happens once per batch, not once per event. Callers collect the
//! affected workspace-folder roots across a whole notification or debounced
//! batch and call [`LspServer::refresh_project_metadata_facts`] once, which
//! advances [`LspServer::dependency_facts_generation`] a single time when any
//! folder actually refreshed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use perl_lsp_rs_core::config::{DeclaredDependencySource, classify_project_metadata_path};
use perl_uri::uri_to_fs_path;

use super::LspServer;

/// Why a folder's metadata snapshot was retained instead of refreshed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetainedReason {
    /// A metadata file exists but could not be read right now.
    UnreadableOnDisk,
    /// An open editor buffer holds the metadata document's authoritative text.
    OpenBufferAuthority,
}

impl LspServer {
    /// Workspace-folder roots whose metadata facts `uri` affects.
    ///
    /// Empty when the path is not project metadata, is not inside any current
    /// workspace folder, or is not a filesystem path at all — so events from
    /// unrelated trees can never trigger a refresh.
    pub(crate) fn project_metadata_roots_for_uri(&self, uri: &str) -> Vec<PathBuf> {
        let Some(path) = uri_to_fs_path(uri) else {
            return Vec::new();
        };
        let folders = self.workspace_folders.lock();
        let mut roots = Vec::new();
        for folder in folders.iter() {
            let Some(root) = folder.path.clone().or_else(|| uri_to_fs_path(&folder.uri)) else {
                continue;
            };
            if classify_project_metadata_path(&root, &path).is_some() && !roots.contains(&root) {
                roots.push(root);
            }
        }
        roots
    }

    /// Refresh dependency and environment facts for `roots`, at most once each.
    ///
    /// Advances [`Self::dependency_facts_generation`] exactly once when at
    /// least one folder refreshed, so a coalesced burst of metadata writes is
    /// one observable generation step. A folder whose metadata cannot be read
    /// from disk right now keeps its previous snapshot and is recorded as
    /// stale instead of being erased.
    pub(crate) fn refresh_project_metadata_facts(&self, roots: &BTreeSet<PathBuf>) {
        if roots.is_empty() {
            return;
        }

        // Snapshot open-document paths before taking the folder lock. No
        // production path currently holds `documents` across a
        // `workspace_folders` acquisition, but nesting them here would
        // establish an order that any future `documents -> workspace_folders`
        // caller would deadlock against. Hoisting also drops the per-folder
        // re-lock this route would otherwise do inside the loop.
        let open_document_paths: BTreeSet<PathBuf> = {
            let documents = self.documents_guard();
            documents.keys().filter_map(|uri| uri_to_fs_path(uri)).collect()
        };

        let mut refreshed_any = false;
        let mut newly_stale: Vec<(String, RetainedReason)> = Vec::new();
        let mut now_current: Vec<String> = Vec::new();

        {
            let mut folders = self.workspace_folders.lock();
            for folder in folders.iter_mut() {
                let Some(root) = folder.path.clone().or_else(|| uri_to_fs_path(&folder.uri)) else {
                    continue;
                };
                if !roots.contains(&root) {
                    continue;
                }

                if let Some(reason) = Self::retained_snapshot_reason(&root, &open_document_paths) {
                    tracing::debug!(
                        folder = %folder.uri,
                        reason = ?reason,
                        "Retaining dependency/environment snapshot; metadata not readable from disk (#13640)"
                    );
                    newly_stale.push((folder.uri.clone(), reason));
                    continue;
                }

                folder.refresh_workspace_metadata();
                now_current.push(folder.uri.clone());
                refreshed_any = true;
                tracing::debug!(
                    folder = %folder.uri,
                    "Refreshed dependency/environment facts from project metadata (#13640)"
                );
            }
        }

        {
            let mut stale = self.stale_dependency_facts.lock();
            for uri in now_current {
                stale.remove(&uri);
            }
            for (uri, _reason) in newly_stale {
                stale.insert(uri);
            }
        }

        if refreshed_any {
            self.dependency_facts_generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Reason to retain `root`'s current snapshot rather than recompute it.
    ///
    /// Only the declared-dependency sources are consulted: they are the paths
    /// the detector actually reads, so they are the ones whose unreadability
    /// would silently erase facts. The marker paths behind include-root
    /// detection are probed by existence alone and cannot fail this way.
    ///
    /// The probe is the detector's own operation (`read_to_string`), so it
    /// observes exactly the failures that would otherwise yield zero
    /// dependencies: a permission or sharing error mid-save, and content that
    /// is not valid UTF-8. The two cannot be told apart from here, and both
    /// are handled the same way — retain the snapshot and mark it stale — so
    /// an unknown state is explicit rather than silently indistinguishable
    /// from "this project declares nothing".
    ///
    /// A file that is simply absent, with no buffer behind it, is not a
    /// failure — that is a genuine delete, and the refresh must run so its
    /// facts are downgraded.
    ///
    /// `open_document_paths` is the caller's snapshot of currently open
    /// documents. Open-buffer authority (#8041) applies to metadata documents
    /// too: while the editor holds staged text, its buffer — not the disk
    /// bytes — is authoritative, so a disk-derived refresh would record
    /// provenance that contradicts what the user sees. That authority does
    /// not depend on the backing file surviving, which is why openness is
    /// tested before existence below.
    fn retained_snapshot_reason(
        root: &Path,
        open_document_paths: &BTreeSet<PathBuf>,
    ) -> Option<RetainedReason> {
        for source in DeclaredDependencySource::ALL {
            let path = root.join(source.file_name());
            // Openness is checked before existence, matching
            // `process_file_watcher_uri_immediate`. Buffer authority does not
            // depend on the backing file still being present: an external
            // delete of an open metadata document leaves the staged text
            // authoritative until didSave/didClose completes the handoff, so
            // probing existence first would let a delete race erase facts the
            // open buffer still declares.
            if open_document_paths.contains(&path) {
                return Some(RetainedReason::OpenBufferAuthority);
            }
            if !path.is_file() {
                continue;
            }
            if std::fs::read_to_string(&path).is_err() {
                return Some(RetainedReason::UnreadableOnDisk);
            }
        }
        None
    }

    /// Current dependency/environment fact generation (#13640).
    #[must_use]
    pub fn dependency_facts_generation(&self) -> u64 {
        self.dependency_facts_generation.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Whether `folder_uri`'s dependency/environment snapshot is retained but
    /// not current (#13640).
    #[must_use]
    pub fn dependency_facts_are_stale(&self, folder_uri: &str) -> bool {
        self.stale_dependency_facts.lock().contains(folder_uri)
    }

    /// Collect metadata-affected folder roots for a batch of watched URIs.
    pub(crate) fn project_metadata_roots_for_batch<'a, I>(&self, uris: I) -> BTreeSet<PathBuf>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let mut roots = BTreeSet::new();
        for uri in uris {
            roots.extend(self.project_metadata_roots_for_uri(uri));
        }
        roots
    }
}
