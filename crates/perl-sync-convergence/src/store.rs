//! Durable file-backed persistence for convergence transactions.
//!
//! Layout under a caller-supplied root directory (deterministic, small,
//! survives expiring workflow artifacts; no secrets, credentials, raw host
//! paths, or unbounded logs are written):
//!
//! ```text
//! <root>/index.v1.json
//! <root>/transactions/<transaction_id>/events.v1.jsonl
//! <root>/transactions/<transaction_id>/generations/<generation_id>.json
//! ```
//!
//! Every load path is fail-closed: unsupported schema versions, malformed
//! JSON, and unknown enum spellings abort instead of degrading. Generation
//! receipts are immutable once written.

use crate::event::{ConvergenceEvent, ConvergenceView, JOURNAL_SCHEMA_VERSION};
use crate::generation::{
    ConvergenceGeneration, GENERATION_RECEIPT_SCHEMA_VERSION, GenerationReceiptFile,
};
use crate::ids::TransactionId;
use crate::model::{Direction, ReleaseContextMode};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Schema version of the store index format.
pub const INDEX_SCHEMA_VERSION: u32 = 1;

/// Store open/load failures; all variants fail closed.
#[derive(Debug)]
pub enum StoreError {
    /// Persisted index used an unsupported schema version.
    UnsupportedIndexVersion {
        /// Version found on disk.
        found: u32,
    },
    /// Persisted journal used an unsupported schema version.
    UnsupportedJournalVersion {
        /// Version found on disk.
        found: u32,
    },
    /// Persisted receipt used an unsupported schema version.
    UnsupportedReceiptVersion {
        /// Version found on disk.
        found: u32,
    },
    /// Filesystem I/O failure while reading persisted state.
    Io(String),
    /// Persisted bytes were not well-formed JSON of the expected shape.
    Malformed(String),
    /// Journal replay refused an event.
    Replay(crate::event::ReplayError),
    /// Attempted to rewrite an existing generation receipt with different
    /// bytes (negative control 1).
    ImmutableReceiptViolation {
        /// Offending generation identity.
        generation_id: String,
    },
    /// Receipt release-context mode disagreed with its transaction's mode
    /// (negative control 4).
    ReleaseModeConflict {
        /// Offending generation identity.
        generation_id: String,
    },
    /// Transaction ID was unsafe for use as a directory name.
    UnsafeTransactionId(String),
    /// Event belongs to a different transaction than the append target.
    ForeignEvent,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedIndexVersion { found } => {
                write!(
                    f,
                    "unsupported convergence store index schema version {found}; expected {INDEX_SCHEMA_VERSION}"
                )
            }
            Self::UnsupportedJournalVersion { found } => {
                write!(
                    f,
                    "unsupported convergence journal schema version {found}; expected {JOURNAL_SCHEMA_VERSION}"
                )
            }
            Self::UnsupportedReceiptVersion { found } => {
                write!(
                    f,
                    "unsupported generation receipt schema version {found}; expected {GENERATION_RECEIPT_SCHEMA_VERSION}"
                )
            }
            Self::Io(why) => write!(f, "convergence store i/o failure: {why}"),
            Self::Malformed(why) => write!(f, "malformed persisted convergence state: {why}"),
            Self::Replay(error) => write!(f, "persisted journal failed replay: {error}"),
            Self::ImmutableReceiptViolation { generation_id } => {
                write!(
                    f,
                    "refusing to edit existing generation receipt {generation_id}; moved inputs require a successor generation"
                )
            }
            Self::ReleaseModeConflict { generation_id } => {
                write!(f, "generation {generation_id} release mode conflicts with its transaction")
            }
            Self::UnsafeTransactionId(value) => {
                write!(f, "transaction id is not a safe directory name: {value:?}")
            }
            Self::ForeignEvent => {
                f.write_str("event belongs to a different transaction than the append target")
            }
        }
    }
}

impl std::error::Error for StoreError {}

/// Index record for one transaction in the store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionIndexEntry {
    /// Owning transaction.
    pub transaction_id: TransactionId,
    /// Fixed direction.
    pub direction: Direction,
    /// Fixed release-context mode.
    pub release_mode: ReleaseContextMode,
}

/// Versioned store index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreIndexFile {
    /// Persisted format version.
    pub schema_version: u32,
    /// All transactions known to the store.
    pub transactions: Vec<TransactionIndexEntry>,
}

/// A durable convergence store rooted at a directory.
pub struct ConvergenceStore {
    root: PathBuf,
}

impl ConvergenceStore {
    /// Open (creating directories on demand) the store at `root`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|e| StoreError::Io(e.to_string()))?;
        Ok(Self { root })
    }

    fn transaction_dir(&self, tx: &TransactionId) -> Result<PathBuf, StoreError> {
        let id = tx.as_str();
        let safe = !id.is_empty()
            && Path::new(id).file_name().is_some_and(|n| n.to_string_lossy() == id)
            && !id.starts_with('.');
        if !safe {
            return Err(StoreError::UnsafeTransactionId(id.to_string()));
        }
        Ok(self.root.join("transactions").join(id))
    }

    fn events_path(&self, tx: &TransactionId) -> Result<PathBuf, StoreError> {
        Ok(self.transaction_dir(tx)?.join("events.v1.jsonl"))
    }

    fn generation_path(
        &self,
        tx: &TransactionId,
        generation_id: &crate::ids::GenerationId,
    ) -> Result<PathBuf, StoreError> {
        // Wire forms contain ':' which is illegal in Windows filenames; the
        // encoded stem is a deterministic bijective substitution.
        let stem = generation_id.as_str().replace(':', "-");
        Ok(self.transaction_dir(tx)?.join("generations").join(format!("{stem}.json")))
    }

    /// Load and validate the store index; missing file yields an empty index.
    pub fn load_index(&self) -> Result<StoreIndexFile, StoreError> {
        let path = self.root.join("index.v1.json");
        if !path.exists() {
            return Ok(StoreIndexFile {
                schema_version: INDEX_SCHEMA_VERSION,
                transactions: Vec::new(),
            });
        }
        let bytes = fs::read(&path).map_err(|e| StoreError::Io(e.to_string()))?;
        let index: StoreIndexFile =
            serde_json::from_slice(&bytes).map_err(|e| StoreError::Malformed(e.to_string()))?;
        if index.schema_version != INDEX_SCHEMA_VERSION {
            return Err(StoreError::UnsupportedIndexVersion { found: index.schema_version });
        }
        Ok(index)
    }

    fn save_index(&self, index: &StoreIndexFile) -> Result<(), StoreError> {
        self.atomic_write(
            &self.root.join("index.v1.json"),
            &serde_json::to_vec_pretty(index).map_err(|e| StoreError::Malformed(e.to_string()))?,
        )
    }

    /// Append one event after validating it against the current journal.
    ///
    /// The event is folded into the loaded journal first; any replay rule it
    /// violates aborts the append without mutating disk state.
    pub fn append_event(
        &self,
        tx: &TransactionId,
        event: &ConvergenceEvent,
    ) -> Result<ConvergenceView, StoreError> {
        if event.transaction_id() != tx {
            return Err(StoreError::ForeignEvent);
        }
        let mut events = self.load_journal(tx)?;
        events.push(event.clone());
        // Validate the would-be extended journal before touching disk.
        let new_view = crate::event::replay(&events).map_err(StoreError::Replay)?;
        let envelope = JournalLine { schema_version: JOURNAL_SCHEMA_VERSION, event: event.clone() };
        let line =
            serde_json::to_string(&envelope).map_err(|e| StoreError::Malformed(e.to_string()))?;

        let dir = self.transaction_dir(tx)?;
        fs::create_dir_all(&dir).map_err(|e| StoreError::Io(e.to_string()))?;
        let path = self.events_path(tx)?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| StoreError::Io(e.to_string()))?;
        writeln!(file, "{line}").map_err(|e| StoreError::Io(e.to_string()))?;
        Ok(new_view)
    }

    /// Load and replay one transaction's journal fail-closed.
    pub fn load_journal(&self, tx: &TransactionId) -> Result<Vec<ConvergenceEvent>, StoreError> {
        let path = self.events_path(tx)?;
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = fs::read(&path).map_err(|e| StoreError::Io(e.to_string()))?;
        let text = std::str::from_utf8(&bytes).map_err(|e| StoreError::Malformed(e.to_string()))?;
        let mut events = Vec::new();
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let envelope: JournalLine = serde_json::from_str(line)
                .map_err(|e| StoreError::Malformed(format!("line {}: {e}", i + 1)))?;
            if envelope.schema_version != JOURNAL_SCHEMA_VERSION {
                return Err(StoreError::UnsupportedJournalVersion {
                    found: envelope.schema_version,
                });
            }
            events.push(envelope.event);
        }
        Ok(events)
    }

    /// Reconstruct the current transaction view from durable state alone.
    pub fn load_view(&self, tx: &TransactionId) -> Result<ConvergenceView, StoreError> {
        let events = self.load_journal(tx)?;
        crate::event::replay(&events).map_err(StoreError::Replay)
    }

    /// Write an immutable generation receipt.
    ///
    /// Writing identical canonical bytes is accepted (idempotent retry);
    /// writing different bytes over an existing receipt is refused.
    pub fn write_receipt(&self, receipt: &ConvergenceGeneration) -> Result<PathBuf, StoreError> {
        receipt.validate().map_err(|e| StoreError::Malformed(e.to_string()))?;
        let tx = &receipt.transaction_id;
        let path = self.generation_path(tx, &receipt.generation_id)?;
        if path.exists() {
            let existing = fs::read(&path).map_err(|e| StoreError::Io(e.to_string()))?;
            let fresh = serde_json::to_vec_pretty(&receipt_file(receipt))
                .map_err(|e| StoreError::Malformed(e.to_string()))?;
            if existing == fresh {
                return Ok(path);
            }
            return Err(StoreError::ImmutableReceiptViolation {
                generation_id: receipt.generation_id.as_str().to_string(),
            });
        }
        // Mode coherence across the transaction (negative control 4).
        let conflicting_mode = self.load_index().is_ok_and(|index| {
            index
                .transactions
                .iter()
                .find(|t| &t.transaction_id == tx)
                .is_some_and(|entry| entry.release_mode != receipt.release_context_mode)
        });
        if conflicting_mode {
            return Err(StoreError::ReleaseModeConflict {
                generation_id: receipt.generation_id.as_str().to_string(),
            });
        }
        let dir = path
            .parent()
            .ok_or_else(|| StoreError::UnsafeTransactionId(tx.as_str().to_string()))?;
        fs::create_dir_all(dir).map_err(|e| StoreError::Io(e.to_string()))?;
        let bytes = serde_json::to_vec_pretty(&receipt_file(receipt))
            .map_err(|e| StoreError::Malformed(e.to_string()))?;
        self.atomic_write(&path, &bytes)?;
        Ok(path)
    }

    /// Load one generation receipt, rejecting unsupported schema versions.
    pub fn read_receipt(
        &self,
        tx: &TransactionId,
        generation_id: &crate::ids::GenerationId,
    ) -> Result<ConvergenceGeneration, StoreError> {
        let path = self.generation_path(tx, generation_id)?;
        let bytes = fs::read(&path).map_err(|e| StoreError::Io(e.to_string()))?;
        let file: GenerationReceiptFile =
            serde_json::from_slice(&bytes).map_err(|e| StoreError::Malformed(e.to_string()))?;
        if file.schema_version != GENERATION_RECEIPT_SCHEMA_VERSION {
            return Err(StoreError::UnsupportedReceiptVersion { found: file.schema_version });
        }
        file.receipt.validate().map_err(|e| StoreError::Malformed(e.to_string()))?;
        Ok(file.receipt)
    }

    /// Register a transaction in the index; registering twice with a
    /// different direction/mode fails closed.
    pub fn register_transaction(&self, entry: TransactionIndexEntry) -> Result<(), StoreError> {
        let mut index = self.load_index()?;
        if let Some(existing) =
            index.transactions.iter_mut().find(|t| t.transaction_id == entry.transaction_id)
        {
            if existing.direction != entry.direction || existing.release_mode != entry.release_mode
            {
                return Err(StoreError::ReleaseModeConflict {
                    generation_id: entry.transaction_id.as_str().to_string(),
                });
            }
            return Ok(());
        }
        index.transactions.push(entry);
        self.save_index(&index)
    }

    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
        let parent = path.parent().ok_or_else(|| StoreError::Io("path without parent".into()))?;
        fs::create_dir_all(parent).map_err(|e| StoreError::Io(e.to_string()))?;
        let mut temp_name = path.file_name().map_or_else(
            || std::ffi::OsString::from("convergence.tmp"),
            |name| {
                let mut owned = name.to_os_string();
                owned.push(".tmp");
                owned
            },
        );
        temp_name.push(".partial");
        let temp_path = parent.join(temp_name);
        {
            let mut file =
                fs::File::create(&temp_path).map_err(|e| StoreError::Io(e.to_string()))?;
            file.write_all(bytes).map_err(|e| StoreError::Io(e.to_string()))?;
            file.sync_all().ok();
        }
        // std::fs::rename replaces existing destinations on both POSIX and
        // Windows (MoveFileEx with MOVEFILE_REPLACE_EXISTING).
        fs::rename(&temp_path, path).map_err(|e| StoreError::Io(e.to_string()))?;
        Ok(())
    }
}

fn receipt_file(receipt: &ConvergenceGeneration) -> GenerationReceiptFile {
    GenerationReceiptFile {
        schema_version: GENERATION_RECEIPT_SCHEMA_VERSION,
        receipt: receipt.clone(),
    }
}

/// One versioned journal line.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalLine {
    schema_version: u32,
    #[serde(flatten)]
    event: ConvergenceEvent,
}
