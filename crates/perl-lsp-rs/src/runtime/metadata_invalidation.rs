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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use perl_lsp_rs_core::config::{
    DeclaredDependencySource, MetadataSourceRead, classify_project_metadata_path,
};
use perl_uri::uri_to_fs_path;

use super::LspServer;

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
    /// Advances [`Self::dependency_facts_generation`] exactly once per batch in
    /// which at least one folder refreshed, so a coalesced burst of metadata
    /// writes is one observable generation step.
    ///
    /// Each declared-dependency source is resolved exactly once, preferring an
    /// open buffer's staged text over disk bytes. A source that cannot be read
    /// keeps its own previous entries and marks the folder stale; it does not
    /// block the sources that could be read, and it never blocks
    /// dependency-manager include-root reconciliation.
    pub(crate) fn refresh_project_metadata_facts(&self, roots: &BTreeSet<PathBuf>) {
        if roots.is_empty() {
            return;
        }

        // Snapshot open-document text before taking the folder lock. No
        // production path currently holds `documents` across a
        // `workspace_folders` acquisition, but nesting them here would
        // establish an order that any future `documents -> workspace_folders`
        // caller would deadlock against. Hoisting also drops the per-folder
        // re-lock this route would otherwise do inside the loop.
        let open_document_text: BTreeMap<PathBuf, String> = {
            let documents = self.documents_guard();
            documents
                .iter()
                .filter_map(|(uri, document)| {
                    uri_to_fs_path(uri).map(|path| (path, document.text_str().to_string()))
                })
                .collect()
        };

        let mut refreshed_any = false;
        let mut newly_stale: Vec<String> = Vec::new();
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

                let reads = Self::capture_metadata_reads(&root, &open_document_text);
                let unreadable =
                    reads.iter().any(|(_, read)| matches!(read, MetadataSourceRead::Unreadable));

                folder.refresh_workspace_metadata_from_reads(&reads);
                refreshed_any = true;

                if unreadable {
                    tracing::debug!(
                        folder = %folder.uri,
                        "Retained facts for unreadable metadata sources; snapshot is stale (#13640)"
                    );
                    newly_stale.push(folder.uri.clone());
                } else {
                    now_current.push(folder.uri.clone());
                }
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
            for uri in newly_stale {
                stale.insert(uri);
            }
        }

        if refreshed_any {
            self.dependency_facts_generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Resolve each declared-dependency source under `root` exactly once.
    ///
    /// Open-buffer authority (#8041) applies to metadata documents: while the
    /// editor holds staged text, that text — not the disk bytes — is the
    /// authoritative content, so facts follow what the user actually sees and
    /// stay current as they edit. The buffer is consulted before the existence
    /// probe, matching `process_file_watcher_uri_immediate`, because that
    /// authority does not depend on the backing file surviving: an external
    /// delete of an open document must not erase what its buffer still
    /// declares.
    ///
    /// A source with no buffer is read once and that exact text is what
    /// detection consumes, so there is no window in which a source passes a
    /// readability probe and then fails a second read. Absence is a definite
    /// answer (a genuine delete downgrades its declarations); a read error is
    /// not, and is reported as [`MetadataSourceRead::Unreadable`] so the
    /// caller retains that source's prior facts rather than recording "this
    /// project declares nothing".
    fn capture_metadata_reads(
        root: &Path,
        open_document_text: &BTreeMap<PathBuf, String>,
    ) -> Vec<(DeclaredDependencySource, MetadataSourceRead)> {
        DeclaredDependencySource::ALL
            .into_iter()
            .map(|source| {
                let path = root.join(source.file_name());
                if let Some(text) = open_document_text.get(&path) {
                    return (source, MetadataSourceRead::Text(text.clone()));
                }
                if !path.is_file() {
                    return (source, MetadataSourceRead::Absent);
                }
                match std::fs::read_to_string(&path) {
                    Ok(text) => (source, MetadataSourceRead::Text(text)),
                    Err(_) => (source, MetadataSourceRead::Unreadable),
                }
            })
            .collect()
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
