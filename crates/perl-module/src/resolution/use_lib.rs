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

/// A `use lib` / `no lib` operation with the byte offset at which it becomes
/// active.
///
/// For a well-formed pragma this is the end of its enclosing statement slice,
/// which is what Perl's own ordering implies: the import runs only once the
/// whole argument list has been evaluated.
///
/// The exception is a pragma that is *not* terminated. An editor buffer
/// frequently contains a pragma whose semicolon has not been typed yet
/// (`use lib 'lib'\nuse My::Test;`), and the statement splitter then hands back
/// one slice spanning both lines. Keying activation on that slice's end would
/// hide `lib` from the later use-site and emit a spurious PL701 (#6208), so an
/// unterminated pragma activates at the end of its argument text instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseLibOperation {
    /// Byte offset in the original source at which this operation takes effect.
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

/// Extract ordered `use lib` / `no lib` operations with activation offsets.
///
/// An operation is active at a use-site offset when its `end_offset <= offset`.
#[must_use]
pub fn extract_use_lib_operations_with_offsets(source: &str) -> Vec<UseLibOperation> {
    let mut ops = Vec::new();

    for statement in split_perl_statements(source) {
        let trimmed = statement.trim();

        if let Some(rest) = strip_use_lib_prefix(trimmed) {
            let mut paths = Vec::new();
            let consumed = extract_paths_from_args(rest, &mut paths);
            if !paths.is_empty() {
                let end_offset = activation_offset(source, statement, rest, consumed);
                ops.push(UseLibOperation { end_offset, action: UseLibAction::Add(paths) });
            }
            continue;
        }

        if let Some(rest) = strip_no_lib_prefix(trimmed) {
            let mut paths = Vec::new();
            let consumed = extract_paths_from_args(rest, &mut paths);
            if !paths.is_empty() {
                let end_offset = activation_offset(source, statement, rest, consumed);
                ops.push(UseLibOperation { end_offset, action: UseLibAction::Remove(paths) });
            }
        }
    }

    ops
}

/// Byte offset at which a pragma's paths become visible.
///
/// Defaults to the end of the enclosing statement slice, matching Perl: the
/// import runs after the whole argument list is evaluated, so a compile-time
/// `use` nested inside that list (`use lib 'lib', do { use Nested; 1 };`) runs
/// *before* `lib` joins `@INC` and must not see it.
///
/// The statement end is only wrong when the pragma has no terminator of its
/// own, because `split_perl_statements` then returns a slice that runs on
/// through unrelated later code. That is detected by what follows the consumed
/// arguments:
///
/// - nothing, or `;` — a complete, terminated pragma; use the statement end.
/// - `,` — the argument list continues with an expression this extractor does
///   not parse; the true end is unknown, so conservatively use the statement
///   end rather than activating early.
/// - anything else — the pragma was never terminated and the splitter swallowed
///   a following statement; activate at the end of the argument text (#6208).
fn activation_offset(source: &str, statement: &str, rest: &str, consumed: usize) -> usize {
    let statement_end = byte_offset_within(source, statement) + statement.len();
    let Some(tail) = rest.get(consumed..) else {
        return statement_end;
    };

    let tail = tail.trim_start();
    match tail.chars().next() {
        None | Some(';') | Some(',') => statement_end,
        Some(_) => byte_offset_within(source, rest) + consumed,
    }
}

/// Byte offset of the subslice `inner` within the string it was sliced from.
///
/// Both arguments must come from the same allocation; every caller here derives
/// `inner` from `outer` by slicing or trimming.
fn byte_offset_within(outer: &str, inner: &str) -> usize {
    (inner.as_ptr() as usize).saturating_sub(outer.as_ptr() as usize)
}

fn use_lib_actions_before_offset(
    ops: &[UseLibOperation],
    offset: usize,
) -> impl Iterator<Item = &UseLibAction> {
    ops.iter().filter(move |op| op.end_offset <= offset).map(|op| &op.action)
}

fn resolve_effective_paths_from_actions<'a>(
    actions: impl IntoIterator<Item = &'a UseLibAction>,
    workspace_root: &Path,
    file_dir: Option<&Path>,
) -> Vec<String> {
    let mut resolved = Vec::new();
    for action in actions {
        match action {
            UseLibAction::Add(paths) => {
                let added = resolve_use_lib_paths(paths, workspace_root, file_dir);
                for path in added.into_iter().rev() {
                    resolved.retain(|existing| existing != &path);
                    resolved.insert(0, path);
                }
            }
            UseLibAction::Remove(paths) => {
                for path in resolve_use_lib_paths(paths, workspace_root, file_dir) {
                    resolved.retain(|existing| existing != &path);
                }
            }
        }
    }
    resolved
}

fn cancelled_paths_from_actions<'a>(
    actions: impl IntoIterator<Item = &'a UseLibAction>,
    workspace_root: &Path,
    file_dir: Option<&Path>,
) -> Vec<String> {
    let mut effective = Vec::<String>::new();
    let mut cancelled = Vec::<String>::new();
    for action in actions {
        match action {
            UseLibAction::Add(paths) => {
                let added = resolve_use_lib_paths(paths, workspace_root, file_dir);
                for path in &added {
                    cancelled.retain(|c| c != path);
                }
                for path in added.into_iter().rev() {
                    effective.retain(|e| e != &path);
                    effective.insert(0, path);
                }
            }
            UseLibAction::Remove(paths) => {
                let removed = resolve_use_lib_paths(paths, workspace_root, file_dir);
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
    let actions = use_lib_actions_before_offset(ops, offset);
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
    let actions = use_lib_actions_before_offset(ops, offset);
    cancelled_paths_from_actions(actions, workspace_root, file_dir)
}
