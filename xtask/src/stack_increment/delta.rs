//! Child-only semantic delta between two exact trees.
//!
//! The delta is computed from tree SHA to tree SHA, never from branch names
//! or commit counts, and its fingerprint binds it to both endpoint trees so
//! endpoint movement can never leave a stale delta current.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// One changed path in the child-only delta.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeltaPath {
    /// Change classification from the closed [`DeltaStatus`] vocabulary.
    pub status: DeltaStatus,
    /// New path of the change.
    pub path: String,
    /// Previous path when the status is [`DeltaStatus::Renamed`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renamed_from: Option<String>,
}

/// Closed child-only change-status vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaStatus {
    /// Path exists only in the child tree.
    Added,
    /// Content differs between the trees.
    Modified,
    /// Path exists only in the parent tree.
    Deleted,
    /// Path was renamed or copied with edits.
    Renamed,
    /// File type or mode changed.
    TypeChange,
}

impl DeltaStatus {
    /// Git `--name-status` letter for this status.
    #[must_use]
    pub const fn as_git_letter(self) -> char {
        match self {
            Self::Added => 'A',
            Self::Modified => 'M',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::TypeChange => 'T',
        }
    }

    fn parse(letter: char) -> Option<Self> {
        match letter {
            'A' => Some(Self::Added),
            'M' => Some(Self::Modified),
            'D' => Some(Self::Deleted),
            'R' | 'C' => Some(Self::Renamed),
            'T' => Some(Self::TypeChange),
            _ => None,
        }
    }
}

/// The exact semantic surface the child adds to the parent, bound to both
/// tree identities by fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildDelta {
    /// Parent tree identity the delta starts from.
    pub bound_parent_tree: String,
    /// Child tree identity the delta ends at.
    pub bound_child_tree: String,
    /// Fingerprint over both trees and every ordered path row.
    pub fingerprint: String,
    /// Ordered path rows; canonical order is (path, status, renamed_from).
    pub paths: Vec<DeltaPath>,
}

/// Deterministic fingerprint binding a delta to both endpoint trees.
///
/// The digest input contains both tree SHAs plus one canonical line per row
/// (`status\0renamed-from\0path`), rows sorted by `(path, status,
/// renamed_from)` so observation order can never change the bytes.
#[must_use]
pub fn delta_fingerprint(parent_tree: &str, child_tree: &str, paths: &[DeltaPath]) -> String {
    let mut rows: Vec<&DeltaPath> = paths.iter().collect();
    rows.sort_by(|left, right| {
        (&left.path, &left.status, &left.renamed_from).cmp(&(
            &right.path,
            &right.status,
            &right.renamed_from,
        ))
    });
    let mut input = Vec::new();
    input.extend_from_slice(parent_tree.as_bytes());
    input.push(0);
    input.extend_from_slice(child_tree.as_bytes());
    for row in rows {
        input.push(b'\n');
        input.push(row.status.as_git_letter() as u8);
        if let Some(from) = &row.renamed_from {
            input.push(0);
            input.extend_from_slice(from.as_bytes());
        }
        input.push(0);
        input.extend_from_slice(row.path.as_bytes());
    }
    super::sha256_hex(&input)
}

/// Compute the child-only delta directly between two exact trees using
/// read-only Git. Thin adapter; all domain checks stay pure.
///
/// # Errors
/// Returns the failing Git stderr, or an error when a diff entry carries an
/// unrecognized status letter.
pub fn compute_delta_from_trees(
    repository: &Path,
    parent_tree: &str,
    child_tree: &str,
) -> Result<ChildDelta, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repository)
        .args([
            "diff-tree",
            "-r",
            "--no-commit-id",
            "--name-status",
            "-z",
            "-M",
            "-C",
            parent_tree,
            child_tree,
        ])
        .output()
        .map_err(|error| format!("failed to spawn git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git diff-tree failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let paths = parse_name_status_z(&stdout)?;
    Ok(ChildDelta {
        bound_parent_tree: parent_tree.to_string(),
        bound_child_tree: child_tree.to_string(),
        fingerprint: delta_fingerprint(parent_tree, child_tree, &paths),
        paths,
    })
}

fn parse_name_status_z(stdout: &str) -> Result<Vec<DeltaPath>, String> {
    let fields = stdout.split('\0').filter(|field| !field.is_empty());
    let mut paths = Vec::new();
    let mut fields = fields.peekable();
    while let Some(status_token) = fields.next() {
        let Some(first_letter) = status_token.chars().next() else {
            continue;
        };
        let status = DeltaStatus::parse(first_letter)
            .ok_or_else(|| format!("unrecognized diff-tree status {status_token:?}"))?;
        let first_path = fields.next().ok_or("diff-tree entry ended before its path")?;
        let (path, renamed_from) = if matches!(status, DeltaStatus::Renamed) {
            let destination =
                fields.next().ok_or("rename/copy entry ended before its destination path")?;
            // Git emits rename/copy rows as old path, new path. The domain
            // contract stores the new path in `path` and the old path in
            // `renamed_from`, so both sides can be checked by admission.
            (destination.to_string(), Some(first_path.to_string()))
        } else {
            (first_path.to_string(), None)
        };
        paths.push(DeltaPath { status, path, renamed_from });
    }
    Ok(paths)
}

/// Refuse any delta row outside the declared edge scope. Scope entries are
/// literal prefixes ending in `/` (whole subtrees) or full paths. An empty
/// scope therefore admits only an empty delta: silently contained sibling or
/// controller work can never pass as this edge's increment.
///
/// # Errors
/// Returns `("undeclared_delta_surface", <offender>)` on the first row that
/// matches no declared scope entry.
pub fn check_declared_scope(
    delta: &ChildDelta,
    edge: &super::StackEdgeDeclaration,
) -> Result<(), (String, String)> {
    for row in &delta.paths {
        let destination_allowed =
            edge.scope_paths.iter().any(|scope| super::path_matches_scope_entry(&row.path, scope));
        if !destination_allowed {
            return Err((
                "undeclared_delta_surface".to_string(),
                format!(
                    "delta destination path {:?} is outside every declared scope of the stack edge",
                    row.path
                ),
            ));
        }
        if let Some(source) = &row.renamed_from {
            let source_allowed =
                edge.scope_paths.iter().any(|scope| super::path_matches_scope_entry(source, scope));
            if !source_allowed {
                return Err((
                    "undeclared_delta_surface".to_string(),
                    format!(
                        "delta source path {:?} for destination {:?} is outside every declared scope of the stack edge",
                        source, row.path
                    ),
                ));
            }
        }
    }
    Ok(())
}
