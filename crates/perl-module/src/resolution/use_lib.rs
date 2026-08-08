//! Extract include paths from `use lib` and `FindBin` statements.
//!
//! Scans Perl source text for `use lib` pragmas and recognizes common
//! `FindBin` patterns to discover additional module include directories.

use std::path::Path;

mod extract;
mod resolve;
mod statements;

use extract::extract_paths_from_args;
pub use resolve::resolve_use_lib_paths;
use statements::{split_perl_statements, strip_no_lib_prefix, strip_use_lib_prefix};

/// A discovered include path from a `use lib` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseLibPath {
    /// The resolved directory path (relative or absolute).
    pub path: String,
    /// Whether this path was derived from a `FindBin` variable.
    pub from_findbin: bool,
}

/// A `use lib` / `no lib` operation extracted from source in lexical order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UseLibAction {
    /// Add paths to the effective include stack.
    Add(Vec<UseLibPath>),
    /// Remove paths from the effective include stack.
    Remove(Vec<UseLibPath>),
}

/// A `use lib` / `no lib` operation with the byte offset immediately after its
/// parsed argument text, falling back to the enclosing statement slice when
/// argument parsing cannot determine an extent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseLibOperation {
    /// Byte offset in the original source immediately after the parsed arguments.
    pub end_offset: usize,
    /// The extracted operation.
    pub action: UseLibAction,
}

/// Extract include paths from `use lib` statements in Perl source text.
///
/// Handles the following patterns:
/// - `use lib 'path';`
/// - `use lib "path";`
/// - `use lib qw(path1 path2);`
/// - `use lib qw/path1 path2/;`
/// - `use lib ("path1", "path2");`
/// - `use lib '$FindBin::Bin/path'` and `"$FindBin::Bin/path"`
/// - `use lib '$Bin/path'` and `"$RealBin/path"` (from `FindBin` exports)
///
/// Returns extracted paths in order of appearance.
pub fn extract_use_lib_paths(source: &str) -> Vec<UseLibPath> {
    let mut paths = Vec::new();

    for statement in split_perl_statements(source) {
        let trimmed = statement.trim();
        if let Some(rest) = strip_use_lib_prefix(trimmed) {
            extract_paths_from_args(rest, &mut paths);
        }
    }

    paths
}

/// Extract ordered `use lib` and `no lib` operations from source text.
#[must_use]
pub fn extract_use_lib_operations(source: &str) -> Vec<UseLibAction> {
    extract_use_lib_operations_with_offsets(source).into_iter().map(|op| op.action).collect()
}

/// Extract ordered `use lib` / `no lib` operations with argument end offsets.
///
/// Offsets match the prefix slicing semantics used by
/// [`resolve_use_lib_paths_from_source_at_offset`]: only complete statements
/// whose parsed argument text has ended at or before `use_site_offset` is active.
#[must_use]
pub fn extract_use_lib_operations_with_offsets(source: &str) -> Vec<UseLibOperation> {
    let mut ops = Vec::new();

    for statement in split_perl_statements(source) {
        let trimmed = statement.trim();
        if let Some(rest) = strip_use_lib_prefix(trimmed) {
            let mut paths = Vec::new();
            let consumed = extract_paths_from_args(rest, &mut paths);
            if !paths.is_empty() {
                let end_offset = operation_end_offset(source, statement, rest, consumed);
                ops.push(UseLibOperation { end_offset, action: UseLibAction::Add(paths) });
            }
            continue;
        }

        if let Some(rest) = strip_no_lib_prefix(trimmed) {
            let mut paths = Vec::new();
            let consumed = extract_paths_from_args(rest, &mut paths);
            if !paths.is_empty() {
                let end_offset = operation_end_offset(source, statement, rest, consumed);
                ops.push(UseLibOperation { end_offset, action: UseLibAction::Remove(paths) });
            }
        }
    }

    ops
}

fn statement_end_offset(source: &str, statement: &str) -> usize {
    let start = statement.as_ptr() as usize - source.as_ptr() as usize;
    start + statement.len()
}

fn operation_end_offset(source: &str, statement: &str, args: &str, consumed: usize) -> usize {
    if consumed == 0 {
        return statement_end_offset(source, statement);
    }

    let args_start = args.as_ptr() as usize - source.as_ptr() as usize;
    args_start + consumed
}

fn use_lib_actions_before_offset(
    ops: &[UseLibOperation],
    offset: usize,
) -> impl Iterator<Item = &UseLibAction> {
    ops.iter().filter(move |op| op.end_offset <= offset).map(|op| &op.action)
}

fn resolve_effective_paths_from_actions(
    actions: impl IntoIterator<Item = UseLibAction>,
    workspace_root: &Path,
    file_dir: Option<&Path>,
) -> Vec<String> {
    let mut resolved = Vec::new();
    for action in actions {
        match action {
            UseLibAction::Add(paths) => {
                let added = resolve_use_lib_paths(&paths, workspace_root, file_dir);
                for path in added.into_iter().rev() {
                    resolved.retain(|existing| existing != &path);
                    resolved.insert(0, path);
                }
            }
            UseLibAction::Remove(paths) => {
                for path in resolve_use_lib_paths(&paths, workspace_root, file_dir) {
                    resolved.retain(|existing| existing != &path);
                }
            }
        }
    }
    resolved
}

fn cancelled_paths_from_actions(
    actions: impl IntoIterator<Item = UseLibAction>,
    workspace_root: &Path,
    file_dir: Option<&Path>,
) -> Vec<String> {
    let mut effective = Vec::<String>::new();
    let mut cancelled = Vec::<String>::new();
    for action in actions {
        match action {
            UseLibAction::Add(paths) => {
                let added = resolve_use_lib_paths(&paths, workspace_root, file_dir);
                for path in &added {
                    cancelled.retain(|c| c != path);
                }
                for path in added.into_iter().rev() {
                    effective.retain(|e| e != &path);
                    effective.insert(0, path);
                }
            }
            UseLibAction::Remove(paths) => {
                let removed = resolve_use_lib_paths(&paths, workspace_root, file_dir);
                for path in removed {
                    effective.retain(|e| e != &path);
                    if !cancelled.contains(&path) {
                        cancelled.push(path);
                    }
                }
            }
        }
    }
    cancelled
}

/// Resolve effective include paths from lexical `use lib` / `no lib` operations.
#[must_use]
pub fn resolve_use_lib_paths_from_source(
    source: &str,
    workspace_root: &Path,
    file_dir: Option<&Path>,
) -> Vec<String> {
    resolve_use_lib_paths_from_source_at_offset(source, source.len(), workspace_root, file_dir)
}

/// Resolve effective include paths from lexical `use lib` / `no lib` operations,
/// considering only source text up to the provided byte offset.
#[must_use]
pub fn resolve_use_lib_paths_from_source_at_offset(
    source: &str,
    offset: usize,
    workspace_root: &Path,
    file_dir: Option<&Path>,
) -> Vec<String> {
    let ops = extract_use_lib_operations_with_offsets(source);
    resolve_use_lib_paths_from_operations_at_offset(&ops, offset, workspace_root, file_dir)
}

/// Resolve effective include paths from pre-extracted operations at a byte offset.
#[must_use]
pub fn resolve_use_lib_paths_from_operations_at_offset(
    ops: &[UseLibOperation],
    offset: usize,
    workspace_root: &Path,
    file_dir: Option<&Path>,
) -> Vec<String> {
    let actions = use_lib_actions_before_offset(ops, offset).cloned();
    resolve_effective_paths_from_actions(actions, workspace_root, file_dir)
}

/// Compute the set of paths that are currently excluded from `@INC` at a given
/// source offset due to `no lib` operations.
///
/// Returns the resolved path strings that have been explicitly removed by `no lib`
/// and not subsequently re-added by a later `use lib` before the given offset.
/// Callers should use this set to filter out matching entries from configured
/// include paths, so that `no lib 'lib'` cancels both lexical AND configured
/// `lib` entries that would otherwise survive the lexical scan.
///
/// # Example
///
/// For the source `use lib 'lib'; no lib 'lib'; use GoneModule;` at an offset
/// within `use GoneModule;`, this function returns `["lib"]` because `lib` was
/// added then removed before the offset.
#[must_use]
pub fn no_lib_cancelled_paths_at_offset(
    source: &str,
    offset: usize,
    workspace_root: &Path,
    file_dir: Option<&Path>,
) -> Vec<String> {
    let ops = extract_use_lib_operations_with_offsets(source);
    no_lib_cancelled_paths_from_operations_at_offset(&ops, offset, workspace_root, file_dir)
}

/// Compute cancelled paths from pre-extracted operations at a byte offset.
#[must_use]
pub fn no_lib_cancelled_paths_from_operations_at_offset(
    ops: &[UseLibOperation],
    offset: usize,
    workspace_root: &Path,
    file_dir: Option<&Path>,
) -> Vec<String> {
    let actions = use_lib_actions_before_offset(ops, offset).cloned();
    cancelled_paths_from_actions(actions, workspace_root, file_dir)
}
