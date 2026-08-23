//! Document store for managing in-memory text content
//!
//! Maintains the current state of all open documents, tracking
//! versions and content without relying on filesystem state.

use crate::line_index::LineIndex;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// A document in the store
#[derive(Debug, Clone)]
pub struct Document {
    /// The document URI
    pub uri: String,
    /// LSP version number
    pub version: i32,
    /// Line index for position calculations. This also owns the full source
    /// text, which is exposed via [`Document::text`]; the text is stored here
    /// once rather than in a separate field to avoid a redundant per-document
    /// copy.
    pub line_index: LineIndex,
}

impl Document {
    /// Create a new document
    pub fn new(uri: String, version: i32, text: String) -> Self {
        let line_index = LineIndex::new(text);
        Self { uri, version, line_index }
    }

    /// Update the document content
    pub fn update(&mut self, version: i32, text: String) {
        self.version = version;
        self.line_index = LineIndex::new(text);
    }

    /// The full source text of the document.
    #[must_use]
    pub fn text(&self) -> &str {
        self.line_index.text()
    }
}

/// Thread-safe document store
#[derive(Debug, Clone)]
pub struct DocumentStore {
    documents: Arc<RwLock<HashMap<String, Document>>>,
}

/// Result of atomically accepting a candidate document.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DocumentCommitResult {
    /// The candidate replaced or opened the document.
    Accepted,
    /// The candidate was older than the tracked document version.
    RejectedStale,
}

impl DocumentStore {
    /// Create a new empty store
    pub fn new() -> Self {
        Self { documents: Arc::new(RwLock::new(HashMap::new())) }
    }

    /// Normalize a URI to a consistent key
    /// This handles platform differences and ensures consistent lookups
    pub fn uri_key(uri: &str) -> String {
        perl_uri::uri_key(uri)
    }

    /// Open or update a document
    pub fn open(&self, uri: String, version: i32, text: String) {
        let key = Self::uri_key(&uri);
        let doc = Document::new(uri, version, text);

        if let Ok(mut docs) = self.documents.write() {
            docs.insert(key, doc);
        }
    }

    /// Update a document's content
    pub fn update(&self, uri: &str, version: i32, text: String) -> bool {
        let key = Self::uri_key(uri);

        let Ok(mut docs) = self.documents.write() else {
            return false;
        };
        if let Some(doc) = docs.get_mut(&key) {
            if version < doc.version {
                return false;
            }
            doc.update(version, text);
            true
        } else {
            false
        }
    }

    /// Atomically decide whether a candidate may become the stored document.
    /// Tracked callers enforce monotonic versions; untracked callers explicitly
    /// represent refreshes and bypass that numeric check.
    pub fn accept_candidate(
        &self,
        uri: String,
        version: i32,
        text: String,
        enforce_version: bool,
    ) -> DocumentCommitResult {
        let key = Self::uri_key(&uri);
        let Ok(mut docs) = self.documents.write() else {
            return DocumentCommitResult::RejectedStale;
        };
        match docs.get_mut(&key) {
            Some(doc) if enforce_version && version < doc.version => {
                DocumentCommitResult::RejectedStale
            }
            Some(doc) => {
                doc.update(version, text);
                DocumentCommitResult::Accepted
            }
            None => {
                docs.insert(key, Document::new(uri, version, text));
                DocumentCommitResult::Accepted
            }
        }
    }

    /// Close a document
    pub fn close(&self, uri: &str) -> bool {
        let key = Self::uri_key(uri);
        let Ok(mut docs) = self.documents.write() else {
            return false;
        };
        docs.remove(&key).is_some()
    }

    /// Restore a previous document only when the current entry still matches
    /// the rejected update that produced it.
    pub fn restore_if_current(
        &self,
        uri: &str,
        expected_version: i32,
        expected_text: &str,
        previous: Option<&Document>,
    ) -> bool {
        let key = Self::uri_key(uri);
        let Ok(mut docs) = self.documents.write() else {
            return false;
        };
        let Some(current) = docs.get(&key) else {
            return false;
        };
        if current.version != expected_version || current.text() != expected_text {
            return false;
        }

        match previous {
            Some(document) => {
                docs.insert(key, document.clone());
            }
            None => {
                docs.remove(&key);
            }
        }
        true
    }

    /// Get a document by URI
    pub fn get(&self, uri: &str) -> Option<Document> {
        let key = Self::uri_key(uri);
        let docs = self.documents.read().ok()?;
        docs.get(&key).cloned()
    }

    /// Get the text content of a document
    pub fn get_text(&self, uri: &str) -> Option<String> {
        let key = Self::uri_key(uri);
        let docs = self.documents.read().ok()?;
        docs.get(&key).map(|doc| doc.text().to_string())
    }

    /// Get all open documents
    pub fn all_documents(&self) -> Vec<Document> {
        let Ok(docs) = self.documents.read() else {
            return Vec::new();
        };
        docs.values().cloned().collect()
    }

    /// Check if a document is open
    pub fn is_open(&self, uri: &str) -> bool {
        let key = Self::uri_key(uri);
        let Ok(docs) = self.documents.read() else {
            return false;
        };
        docs.contains_key(&key)
    }

    /// Get the count of open documents
    pub fn count(&self) -> usize {
        let Ok(docs) = self.documents.read() else {
            return 0;
        };
        docs.len()
    }

    /// Estimate total bytes used by all stored document texts.
    ///
    /// Only available when the `memory-profiling` feature is enabled.
    /// Returns the sum of `text.len()` for every open document; does not
    /// account for `Document` struct overhead or other metadata overhead.
    #[cfg(feature = "memory-profiling")]
    pub fn total_text_bytes(&self) -> usize {
        let Ok(docs) = self.documents.read() else {
            return 0;
        };
        docs.values().map(|d| d.text().len()).sum()
    }
}

impl Default for DocumentStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::must_some;

    #[test]
    fn test_document_lifecycle() {
        let store = DocumentStore::new();
        let uri = "file:///test.pl".to_string();

        // Open document
        store.open(uri.clone(), 1, "print 'hello';".to_string());
        assert!(store.is_open(&uri));
        assert_eq!(store.count(), 1);

        // Get document
        let doc = must_some(store.get(&uri));
        assert_eq!(doc.version, 1);
        assert_eq!(doc.text(), "print 'hello';");

        // Update document
        assert!(store.update(&uri, 2, "print 'world';".to_string()));
        let doc = must_some(store.get(&uri));
        assert_eq!(doc.version, 2);
        assert_eq!(doc.text(), "print 'world';");

        // Close document
        assert!(store.close(&uri));
        assert!(!store.is_open(&uri));
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn test_uri_drive_letter_normalization() {
        let uri1 = "file:///C:/test.pl";
        let uri2 = "file:///c:/test.pl";
        assert_eq!(DocumentStore::uri_key(uri1), DocumentStore::uri_key(uri2));
    }

    #[test]
    fn test_drive_letter_lookup() {
        let store = DocumentStore::new();
        let uri_upper = "file:///C:/test.pl".to_string();
        let uri_lower = "file:///c:/test.pl".to_string();

        store.open(uri_upper.clone(), 1, "# test".to_string());
        assert!(store.is_open(&uri_lower));
        assert_eq!(store.get_text(&uri_lower), Some("# test".to_string()));
        assert!(store.close(&uri_lower));
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn test_multiple_documents() {
        let store = DocumentStore::new();

        let uri1 = "file:///a.pl".to_string();
        let uri2 = "file:///b.pl".to_string();

        store.open(uri1.clone(), 1, "# file a".to_string());
        store.open(uri2.clone(), 1, "# file b".to_string());

        assert_eq!(store.count(), 2);
        assert_eq!(store.get_text(&uri1), Some("# file a".to_string()));
        assert_eq!(store.get_text(&uri2), Some("# file b".to_string()));

        let all = store.all_documents();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_uri_with_spaces() {
        let store = DocumentStore::new();
        let uri = "file:///path%20with%20spaces/test.pl".to_string();

        store.open(uri.clone(), 1, "# test".to_string());
        assert!(store.is_open(&uri));

        let doc = must_some(store.get(&uri));
        assert_eq!(doc.text(), "# test");
    }

    #[test]
    fn test_update_rejects_stale_version() {
        let store = DocumentStore::new();
        let uri = "file:///versioned.pl".to_string();

        store.open(uri.clone(), 3, "current".to_string());
        assert!(!store.update(&uri, 2, "stale".to_string()));

        let doc = must_some(store.get(&uri));
        assert_eq!(doc.version, 3);
        assert_eq!(doc.text(), "current");
    }

    #[test]
    fn test_accept_candidate_separates_tracked_and_untracked_versions() {
        let store = DocumentStore::new();
        let uri = "file:///candidate.pl".to_string();
        store.open(uri.clone(), 9, "current".to_string());

        assert_eq!(
            store.accept_candidate(uri.clone(), 8, "stale".to_string(), true),
            DocumentCommitResult::RejectedStale
        );
        assert_eq!(
            store.accept_candidate(uri.clone(), 1, "refresh".to_string(), false),
            DocumentCommitResult::Accepted
        );
        let doc = must_some(store.get(&uri));
        assert_eq!(doc.version, 1);
        assert_eq!(doc.text(), "refresh");
    }

    #[test]
    fn test_update_accepts_same_version() {
        let store = DocumentStore::new();
        let uri = "file:///same-version.pl".to_string();

        store.open(uri.clone(), 5, "first".to_string());
        assert!(store.update(&uri, 5, "second".to_string()));

        let doc = must_some(store.get(&uri));
        assert_eq!(doc.version, 5);
        assert_eq!(doc.text(), "second");
    }

    #[test]
    fn test_open_replaces_existing_document() {
        let store = DocumentStore::new();
        let uri = "file:///replace.pl".to_string();

        store.open(uri.clone(), 1, "old".to_string());
        store.open(uri.clone(), 7, "new".to_string());

        assert_eq!(store.count(), 1);
        let doc = must_some(store.get(&uri));
        assert_eq!(doc.version, 7);
        assert_eq!(doc.text(), "new");
    }

    #[test]
    fn test_close_returns_false_for_missing_document() {
        let store = DocumentStore::new();
        assert!(!store.close("file:///missing.pl"));
    }

    #[test]
    fn test_update_rebuilds_line_index() {
        let store = DocumentStore::new();
        let uri = "file:///lines.pl".to_string();

        store.open(uri.clone(), 1, "line1\nline2".to_string());
        assert!(store.update(&uri, 2, "line1\nline2\nline3".to_string()));

        let doc = must_some(store.get(&uri));
        assert_eq!(doc.line_index.offset_to_position(12), (2, 0));
    }

    #[test]
    fn test_text_is_single_source_of_truth() {
        // Regression guard for #1660: the document text is stored once (inside
        // `line_index`) and exposed via `text()`. `text()` must exactly reflect
        // the source after open and after update, and must agree with the text
        // owned by `line_index`.
        let store = DocumentStore::new();
        let uri = "file:///single-source.pl".to_string();

        let opened = "use strict;\nmy $x = 1;";
        store.open(uri.clone(), 1, opened.to_string());
        let doc = must_some(store.get(&uri));
        assert_eq!(doc.text(), opened);
        assert_eq!(store.get_text(&uri).as_deref(), Some(opened));
        // The index must have been built from the same bytes `text()` returns:
        // the start of the second line maps to (line 1, col 0). This would fail
        // if `text()` ever exposed a buffer that diverged from `line_index`.
        let second_line = doc.text().find('\n').map(|nl| nl + 1).unwrap_or(0);
        assert_eq!(doc.line_index.offset_to_position(second_line), (1, 0));

        // Unicode content must round-trip byte-for-byte through the sole copy.
        let updated = "my $s = \"café\";\nprint $s;\n";
        assert!(store.update(&uri, 2, updated.to_string()));
        let doc = must_some(store.get(&uri));
        assert_eq!(doc.text(), updated);
        assert_eq!(doc.text().len(), updated.len());
        // After update the index tracks the new text: byte offset just past the
        // multi-byte "café" line's newline is the start of line 1.
        let updated_second_line = doc.text().find('\n').map(|nl| nl + 1).unwrap_or(0);
        assert_eq!(doc.line_index.offset_to_position(updated_second_line), (1, 0));
    }
}
