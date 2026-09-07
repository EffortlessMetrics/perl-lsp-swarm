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
    project_metadata_relative_paths,
};
use perl_uri::uri_to_fs_path;

use super::LspServer;

impl LspServer {
    /// Workspace-folder roots whose metadata facts `uri` affects.
    ///
    /// Empty when the path neither is nor contains project metadata, is not
    /// inside any current workspace folder, or is not a filesystem path at all
    /// — so events from unrelated trees can never trigger a refresh.
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
            if !roots.contains(&root) && Self::subtree_touches_metadata(&root, &path) {
                roots.push(root);
            }
        }
        roots
    }

    /// Whether `path` is, or contains, project metadata for `root`.
    ///
    /// Watcher and file-operation notifications can name a directory rather
    /// than each descendant (the catch-all watcher delivers directory delete
    /// and rename events, and `didDeleteFiles`/`didRenameFiles` accept
    /// directory subjects). Matching only exact metadata files would leave
    /// facts stale when a workspace root, `local/`, or `.carmel/` is removed
    /// or moved, so a governed path *under* the event subject counts too.
    ///
    /// Component-wise matching keeps sibling names safe: a `cpanfile2`
    /// event does not match `cpanfile`. For an exact metadata file this is
    /// identical to classification, which the shared path set guarantees.
    ///
    /// Containment is ASCII case-insensitive for the same reason
    /// classification is (`config::project_metadata::relative_matches`): the
    /// detectors address metadata by `workspace_root.join(..)`, which resolves
    /// a differently-cased name on a case-insensitive filesystem. A
    /// byte-exact containment check would classify a `LOCAL/` delete as
    /// irrelevant while the detector would still read `local/lib/perl5`,
    /// leaving the include root stale. Matching too eagerly costs one extra
    /// refresh that recomputes from disk; missing the event does not recover.
    fn subtree_touches_metadata(root: &Path, path: &Path) -> bool {
        if classify_project_metadata_path(root, path).is_some() {
            return true;
        }
        project_metadata_relative_paths().any(|relative| {
            let mut metadata_path = root.to_path_buf();
            for component in relative.split('/') {
                metadata_path.push(component);
            }
            Self::path_contains_ignore_ascii_case(path, &metadata_path)
        })
    }

    /// Whether `prefix` is a component-wise, ASCII case-insensitive prefix of
    /// `full`.
    ///
    /// Component-wise rather than string-wise so `local` does not "contain"
    /// `localhost`, and ASCII-only so the comparison stays locale-independent.
    fn path_contains_ignore_ascii_case(prefix: &Path, full: &Path) -> bool {
        let mut prefix_components = prefix.components();
        let mut full_components = full.components();
        loop {
            match (prefix_components.next(), full_components.next()) {
                (None, _) => return true,
                (Some(_), None) => return false,
                (Some(expected), Some(actual)) => {
                    let (Some(expected), Some(actual)) =
                        (expected.as_os_str().to_str(), actual.as_os_str().to_str())
                    else {
                        return false;
                    };
                    if !expected.eq_ignore_ascii_case(actual) {
                        return false;
                    }
                }
            }
        }
    }

    /// Staged editor text for `path`, if any document is open for it.
    ///
    /// An exact match wins. Otherwise a differently-cased document counts only
    /// when the filesystem says it is *the same file*, established by
    /// canonicalizing both and comparing — not by assuming that a
    /// case-insensitive name match implies identity.
    ///
    /// The distinction is load-bearing in both directions. On a
    /// case-insensitive filesystem the editor may spell the document
    /// `CPANFILE` while the detector addresses `cpanfile`; they canonicalize
    /// to one path, and without this the staged text would lose to disk bytes,
    /// breaking open-buffer authority (#8041) on exactly the platforms the
    /// case-insensitive classifier exists for. On a case-sensitive filesystem
    /// `CPANFILE` and `cpanfile` are two different files that canonicalize
    /// apart, so an open `CPANFILE` must never supply text for the real
    /// `cpanfile` — a name-only match would let an unrelated buffer replace
    /// genuine metadata.
    ///
    /// Canonicalization needs both paths to exist. When the metadata file has
    /// been deleted while a case-variant buffer stays open, identity cannot be
    /// established and no text is returned; the exact-match path above still
    /// covers the ordinary "open buffer outlives its backing file" case, which
    /// is the one the delete-race regression depends on.
    fn staged_metadata_text<'a>(
        open_document_text: &'a BTreeMap<PathBuf, String>,
        path: &Path,
    ) -> Option<&'a String> {
        if let Some(text) = open_document_text.get(path) {
            return Some(text);
        }
        let canonical_target = std::fs::canonicalize(path).ok()?;
        open_document_text.iter().find_map(|(candidate, text)| {
            if candidate == path || !Self::paths_equal_ignore_ascii_case(candidate, path) {
                return None;
            }
            let canonical_candidate = std::fs::canonicalize(candidate).ok()?;
            (canonical_candidate == canonical_target).then_some(text)
        })
    }

    /// Component-wise, ASCII case-insensitive path equality.
    ///
    /// A cheap pre-filter for the canonicalization above: it keeps the
    /// syscall off every open document and off any candidate that is not even
    /// a case variant of the target.
    fn paths_equal_ignore_ascii_case(left: &Path, right: &Path) -> bool {
        left.components().count() == right.components().count()
            && Self::path_contains_ignore_ascii_case(left, right)
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

        // Serialize the whole refresh. The snapshot below is deliberately
        // taken outside `workspace_folders` (nesting `documents` inside it
        // would invert the established order), which on its own would let two
        // concurrent refreshes snapshot in one order and apply in the other —
        // an older buffer snapshot committing last and overwriting newer
        // facts. This guard is acquired before both other locks and only by
        // this route, so it orders refreshes against each other without
        // joining the lock graph the other two participate in. A refresh that
        // waits here snapshots only after the previous one has fully applied.
        let _refresh_order = self.metadata_refresh_serialization.lock();

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
                if let Some(text) = Self::staged_metadata_text(open_document_text, &path) {
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
    ///
    /// Test/observation seam only: deliberately not public API, and compiled
    /// only under test because no production consumer reads it yet.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn dependency_facts_generation(&self) -> u64 {
        self.dependency_facts_generation.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Whether `folder_uri`'s dependency/environment snapshot is retained but
    /// not current (#13640).
    ///
    /// Test/observation seam only: deliberately not public API, and compiled
    /// only under test because no production consumer reads it yet.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn dependency_facts_are_stale(&self, folder_uri: &str) -> bool {
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

    /// Refresh metadata facts after a text-document lifecycle change (#13640).
    ///
    /// Open-buffer authority (#8041) makes the staged text the authority for a
    /// metadata document, but the buffer only becomes *the* authority when
    /// something re-reads it. The watcher cannot supply that: an unsaved edit
    /// changes no bytes on disk, so no filesystem notification is delivered and
    /// the facts would sit at the last disk-or-watcher-driven read until the
    /// user saved. This is the text-document half of the same route.
    ///
    /// Cheap for ordinary source: `project_metadata_roots_for_uri` returns
    /// empty for anything that is not workspace-root project metadata, so a
    /// keystroke in a `.pm` file costs one classification and nothing else.
    ///
    /// Called after the change is committed and with no document lock held —
    /// `refresh_project_metadata_facts` takes `documents` and then
    /// `workspace_folders`, so calling it mid-edit would invert that order.
    pub(crate) fn refresh_metadata_for_document_uri(&self, uri: &str) {
        let roots: BTreeSet<PathBuf> =
            self.project_metadata_roots_for_uri(uri).into_iter().collect();
        if roots.is_empty() {
            return;
        }
        self.refresh_project_metadata_facts(&roots);
    }
}

/// Focused proof for the two free helpers above.
///
/// `metadata_invalidation_tests` is a sibling module, so it can only reach
/// these through the server and its strongest nearby assertion is a negative
/// control. That leaves the per-source resolution in `capture_metadata_reads`
/// and the subtree predicate in `subtree_touches_metadata` exercised but not
/// *discriminated*: a test that asserts indexing did not happen cannot fail
/// when one of these branches returns the wrong variant. These tests call both
/// helpers directly and assert the exact variant or boolean each input must
/// produce, so a wrong branch is caught here rather than surviving to a
/// behavioural test that happens not to look.
#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "test-only policy proof: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]

    use super::LspServer;
    use perl_lsp_rs_core::config::{DeclaredDependencySource, MetadataSourceRead};
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    fn read_for(
        reads: &[(DeclaredDependencySource, MetadataSourceRead)],
        source: DeclaredDependencySource,
    ) -> MetadataSourceRead {
        reads
            .iter()
            .find(|(candidate, _)| *candidate == source)
            .map(|(_, read)| read.clone())
            .expect("every declared source must be resolved")
    }

    /// An open buffer is authoritative even with no backing file (#8041).
    #[test]
    fn an_open_buffer_supplies_text_without_any_backing_file() {
        let temp = tempfile::tempdir().expect("temp dir");
        let mut open = BTreeMap::new();
        open.insert(temp.path().join("cpanfile"), "requires 'Buffer';\n".to_string());

        let reads = LspServer::capture_metadata_reads(temp.path(), &open);

        assert_eq!(
            read_for(&reads, DeclaredDependencySource::Cpanfile),
            MetadataSourceRead::Text("requires 'Buffer';\n".to_string()),
            "an open buffer outlives the absence of its backing file"
        );
    }

    /// Openness is checked before existence, so staged text wins over disk.
    #[test]
    fn an_open_buffer_wins_over_disagreeing_disk_bytes() {
        let temp = tempfile::tempdir().expect("temp dir");
        std::fs::write(temp.path().join("cpanfile"), "requires 'Disk';\n").expect("write disk");
        let mut open = BTreeMap::new();
        open.insert(temp.path().join("cpanfile"), "requires 'Buffer';\n".to_string());

        let reads = LspServer::capture_metadata_reads(temp.path(), &open);

        assert_eq!(
            read_for(&reads, DeclaredDependencySource::Cpanfile),
            MetadataSourceRead::Text("requires 'Buffer';\n".to_string()),
            "disk and buffer deliberately disagree; the buffer is the authority"
        );
    }

    #[test]
    fn a_missing_source_is_absent_so_a_delete_can_downgrade() {
        let temp = tempfile::tempdir().expect("temp dir");

        let reads = LspServer::capture_metadata_reads(temp.path(), &BTreeMap::new());

        assert_eq!(
            read_for(&reads, DeclaredDependencySource::Cpanfile),
            MetadataSourceRead::Absent
        );
    }

    #[test]
    fn a_readable_source_yields_exactly_its_disk_bytes() {
        let temp = tempfile::tempdir().expect("temp dir");
        std::fs::write(temp.path().join("META.json"), "{\"x\":1}\n").expect("write disk");

        let reads = LspServer::capture_metadata_reads(temp.path(), &BTreeMap::new());

        assert_eq!(
            read_for(&reads, DeclaredDependencySource::MetaJson),
            MetadataSourceRead::Text("{\"x\":1}\n".to_string())
        );
    }

    /// A file that exists but is not UTF-8 must be `Unreadable`, not `Absent`:
    /// the caller retains prior facts for `Unreadable` and erases them for
    /// `Absent`, so collapsing the two would silently drop declarations.
    #[test]
    fn an_existing_but_undecodable_source_is_unreadable_not_absent() {
        let temp = tempfile::tempdir().expect("temp dir");
        std::fs::write(temp.path().join("cpanfile"), [0x66, 0x6f, 0xff, 0xfe, 0x6f])
            .expect("write invalid utf-8");

        let reads = LspServer::capture_metadata_reads(temp.path(), &BTreeMap::new());

        assert_eq!(
            read_for(&reads, DeclaredDependencySource::Cpanfile),
            MetadataSourceRead::Unreadable,
            "an unreadable source must retain prior facts, not declare nothing"
        );
    }

    /// Every source is resolved exactly once, in `ALL` order.
    #[test]
    fn every_declared_source_is_resolved_exactly_once_in_order() {
        let temp = tempfile::tempdir().expect("temp dir");

        let reads = LspServer::capture_metadata_reads(temp.path(), &BTreeMap::new());

        let resolved: Vec<DeclaredDependencySource> =
            reads.iter().map(|(source, _)| *source).collect();
        assert_eq!(resolved, DeclaredDependencySource::ALL.to_vec());
    }

    /// One unreadable source must not change how the others resolve.
    #[test]
    fn an_unreadable_source_does_not_disturb_its_siblings() {
        let temp = tempfile::tempdir().expect("temp dir");
        std::fs::write(temp.path().join("cpanfile"), [0xff, 0xfe]).expect("write invalid utf-8");
        std::fs::write(temp.path().join("META.yml"), "requires: {}\n").expect("write disk");

        let reads = LspServer::capture_metadata_reads(temp.path(), &BTreeMap::new());

        assert_eq!(
            read_for(&reads, DeclaredDependencySource::Cpanfile),
            MetadataSourceRead::Unreadable
        );
        assert_eq!(
            read_for(&reads, DeclaredDependencySource::MetaYml),
            MetadataSourceRead::Text("requires: {}\n".to_string()),
            "a readable sibling still refreshes"
        );
        assert_eq!(
            read_for(&reads, DeclaredDependencySource::BuildPl),
            MetadataSourceRead::Absent,
            "an absent sibling is still absent"
        );
    }

    fn root() -> PathBuf {
        PathBuf::from(if cfg!(windows) { r"C:\ws" } else { "/ws" })
    }

    fn joined(relative: &str) -> PathBuf {
        let mut path = root();
        for component in relative.split('/') {
            path.push(component);
        }
        path
    }

    #[test]
    fn an_exact_metadata_file_touches_metadata() {
        assert!(LspServer::subtree_touches_metadata(&root(), &joined("cpanfile")));
    }

    /// A directory delete or rename names the container, not each descendant.
    #[test]
    fn a_containing_directory_touches_metadata() {
        for relative in ["local", ".carmel"] {
            assert!(
                LspServer::subtree_touches_metadata(&root(), &joined(relative)),
                "{relative}/ contains governed metadata and must invalidate"
            );
        }
    }

    #[test]
    fn the_workspace_root_itself_touches_metadata() {
        assert!(LspServer::subtree_touches_metadata(&root(), &root()));
    }

    /// Component-wise matching: a sibling sharing a name prefix is not a
    /// subtree, so it must not trigger a refresh.
    #[test]
    fn a_sibling_sharing_a_name_prefix_does_not_touch_metadata() {
        for relative in ["cpanfile2", "localhost", "src"] {
            assert!(
                !LspServer::subtree_touches_metadata(&root(), &joined(relative)),
                "{relative} is not a governed metadata path or container"
            );
        }
    }

    #[test]
    fn a_path_outside_the_workspace_does_not_touch_metadata() {
        let outside = if cfg!(windows) { r"C:\other\local" } else { "/other/local" };
        assert!(!LspServer::subtree_touches_metadata(&root(), Path::new(outside)));
    }

    /// Containment must share the classifier's ASCII case-insensitive identity.
    /// Deleting `LOCAL/` on a case-insensitive filesystem removes the tree the
    /// detector reads as `local/lib/perl5`, so a byte-exact check would leave
    /// the include root stale.
    #[test]
    fn a_case_variant_container_directory_touches_metadata() {
        for relative in ["LOCAL", ".CARMEL", "Local"] {
            assert!(
                LspServer::subtree_touches_metadata(&root(), &joined(relative)),
                "{relative}/ resolves to a governed container on a case-insensitive filesystem"
            );
        }
    }

    /// Case-insensitivity must not widen containment into unrelated names.
    #[test]
    fn case_insensitive_containment_does_not_admit_unrelated_directories() {
        for relative in ["LOCALHOST", "CARMEL", "lib"] {
            assert!(
                !LspServer::subtree_touches_metadata(&root(), &joined(relative)),
                "{relative} contains no governed metadata"
            );
        }
    }

    /// Whether this filesystem resolves `cpanfile` and `CPANFILE` to one file.
    ///
    /// Probed rather than inferred from `cfg!(windows)`, because macOS is
    /// case-insensitive by default and a Linux volume can be either.
    fn filesystem_is_case_insensitive(root: &Path) -> bool {
        std::fs::write(root.join("case-probe"), "x").expect("write probe");
        let insensitive = root.join("CASE-PROBE").is_file();
        std::fs::remove_file(root.join("case-probe")).expect("remove probe");
        insensitive
    }

    /// A case-variant buffer is authoritative only when the filesystem says it
    /// is the *same file*, which is why identity is canonicalized rather than
    /// assumed from the name.
    ///
    /// Where `CPANFILE` and `cpanfile` are one file, the staged text must win
    /// or open-buffer authority breaks on that platform. Where they are two
    /// files, the open `CPANFILE` is unrelated content and must not replace
    /// the real `cpanfile` — a name-only match would do exactly that.
    #[test]
    fn a_case_variant_buffer_supplies_text_only_when_it_is_the_same_file() {
        let temp = tempfile::tempdir().expect("temp dir");
        std::fs::write(temp.path().join("cpanfile"), "requires 'Disk';\n").expect("write disk");
        let mut open = BTreeMap::new();
        open.insert(temp.path().join("CPANFILE"), "requires 'Staged';\n".to_string());

        let reads = LspServer::capture_metadata_reads(temp.path(), &open);
        let expected = if filesystem_is_case_insensitive(temp.path()) {
            "requires 'Staged';\n"
        } else {
            "requires 'Disk';\n"
        };

        assert_eq!(
            read_for(&reads, DeclaredDependencySource::Cpanfile),
            MetadataSourceRead::Text(expected.to_string()),
            "a case variant is the same document only where the filesystem says so"
        );
    }

    /// On a case-sensitive filesystem `cpanfile` and `CPANFILE` are genuinely
    /// different documents, so an exact match must win over a case variant.
    #[test]
    fn an_exact_open_buffer_wins_over_a_case_variant() {
        let temp = tempfile::tempdir().expect("temp dir");
        let mut open = BTreeMap::new();
        open.insert(temp.path().join("CPANFILE"), "requires 'Variant';\n".to_string());
        open.insert(temp.path().join("cpanfile"), "requires 'Exact';\n".to_string());

        let reads = LspServer::capture_metadata_reads(temp.path(), &open);

        assert_eq!(
            read_for(&reads, DeclaredDependencySource::Cpanfile),
            MetadataSourceRead::Text("requires 'Exact';\n".to_string()),
            "the exactly-spelled document is the one the detector addresses"
        );
    }

    /// A case-variant lookup must not reach a different file in the same
    /// directory, nor a same-named file at a different depth.
    #[test]
    fn a_case_variant_lookup_does_not_reach_an_unrelated_document() {
        let temp = tempfile::tempdir().expect("temp dir");
        let mut open = BTreeMap::new();
        open.insert(temp.path().join("cpanfile.snapshot"), "snapshot\n".to_string());
        open.insert(temp.path().join("t").join("cpanfile"), "requires 'Nested';\n".to_string());

        let reads = LspServer::capture_metadata_reads(temp.path(), &open);

        assert_eq!(
            read_for(&reads, DeclaredDependencySource::Cpanfile),
            MetadataSourceRead::Absent,
            "neither a longer sibling name nor a nested same-name file is the root cpanfile"
        );
    }
}
