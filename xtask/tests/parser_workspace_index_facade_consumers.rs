//! Recurrence guard for the residual workspace/index facade consumer cutover.
//!
//! Issue #11389 moved every remaining consumer off workspace/document/index
//! authority imported through `perl-parser` compatibility paths (`workspace`,
//! `workspace_index`, `document_store`, and their `compat` escape hatches).
//! The facade crate itself still owns those compatibility exports until
//! #11391 removes the public surface, so `crates/perl-parser/` is excluded
//! here. Any re-introduction elsewhere must register an owned, conditioned
//! exception below instead of silently returning.
//!
//! Detection normalizes each source before matching so formatting cannot hide
//! a violation: line and block comments are stripped, then every
//! `perl_parser ::` path head and brace-group membership is scanned with
//! whitespace-insensitive boundaries. Multi-line forms like
//!
//! ```ignore
//! use perl_parser::{
//!     Parser,
//!     workspace_index::WorkspaceIndex,
//! };
//! ```
//!
//! are rejected exactly like their single-line equivalents, matching the
//! pre-image shapes this cutover removed from `crates/perl-lsp-rs`.
//!
//! The facade's `compat` escape hatch is covered as well: while #11391 has
//! not removed `perl_parser::compat::{document_store, workspace_index}`, a
//! consumer such as `use perl_parser::compat::workspace_index::WorkspaceIndex;`
//! reaches the same authority and is rejected with a
//! `perl_parser::compat::...` token. Importing the bare `compat` module
//! without a governed segment stays allowed.
//!
//! Governed scan roots are the complete Rust source trees for the root-level
//! workspace members `crates`, `xtask`, and `fuzz`; this includes each
//! member's src, tests, examples, benches, and fuzz targets. The
//! `crates/perl-parser/` tree itself is excluded because it is the facade
//! owner until #11391.
//! `perl_workspace::workspace_index` and similar canonical-owner heads never
//! match because every hit must anchor on the exact `perl_parser` path head.
//!
//! Out-of-scope facade rows this guard deliberately does not flag:
//! `perl_parser::workspace_refactor` / `perl_parser::workspace_rename`
//! belong to the legacy refactor rows owned by #5231/#8281, and
//! `perl_parser::index` re-exports semantic-analyzer authority governed by
//! the #11377 semantic-facade row (#12483).
//!
//! String and character literals are blanked as well as comments, preserving
//! newlines so a literal cannot create a hit or hide a later import on the
//! same line.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const FACADE_CRATE_PREFIX: &str = "crates/perl-parser/";
const FACADE_HEAD: &str = "perl_parser";

/// Complete Rust source trees for the root-level workspace members. Keeping
/// the member roots (rather than selected target subdirectories) includes
/// dev/test/example/bench targets and makes the guard itself part of the
/// governed population.
const SCAN_ROOTS: &[&str] = &["crates", "xtask", "fuzz"];

/// Leading path segments of `perl-parser` modules that re-export workspace,
/// document-store, and cross-file index authority. The root-level re-exports
/// `perl_parser::{document_store, workspace_index}` are these same module
/// names, so no separate root-item list exists for this facade row.
const FORBIDDEN_FACADE_SEGMENTS: &[&str] = &["document_store", "workspace", "workspace_index"];

struct TemporaryException {
    path: &'static str,
    token: &'static str,
    owner_issue: &'static str,
    removal_condition: &'static str,
}

const TEMPORARY_EXCEPTIONS: &[TemporaryException] = &[];

fn repo_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

/// Strip comments and Rust literals while preserving newlines and all other
/// structure, including brace groups.
fn code_without_comments(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut index = 0;
    let mut block_depth = 0usize;
    while index < chars.len() {
        if block_depth == 0 && chars[index] == '/' && chars.get(index + 1) == Some(&'/') {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
        } else if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
            block_depth += 1;
            index += 2;
        } else if block_depth > 0 && chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
            block_depth -= 1;
            index += 2;
        } else if block_depth == 0 && chars[index] == 'r' {
            let mut quote = index + 1;
            while chars.get(quote) == Some(&'#') {
                quote += 1;
            }
            if chars.get(quote) == Some(&'"') {
                let hashes = quote - index - 1;
                index = quote + 1;
                while index < chars.len() {
                    if chars[index] == '"'
                        && (0..hashes).all(|offset| chars.get(index + 1 + offset) == Some(&'#'))
                    {
                        index += hashes + 1;
                        break;
                    }
                    if chars[index] == '\n' {
                        out.push('\n');
                    }
                    index += 1;
                }
            } else {
                out.push(chars[index]);
                index += 1;
            }
        } else if block_depth == 0 && matches!(chars[index], '"' | '\'') {
            let delimiter = chars[index];
            index += 1;
            while index < chars.len() {
                let escaped = chars[index] == '\\' && index + 1 < chars.len();
                if chars[index] == '\n' {
                    out.push('\n');
                }
                if chars[index] == delimiter && !escaped {
                    index += 1;
                    break;
                }
                index += if escaped { 2 } else { 1 };
            }
        } else {
            if block_depth == 0 {
                out.push(chars[index]);
            }
            index += 1;
        }
    }
    out
}

fn skip_whitespace(chars: &[char], mut index: usize) -> usize {
    while index < chars.len() && chars[index].is_whitespace() {
        index += 1;
    }
    index
}

fn read_identifier(chars: &[char], start: usize, end: usize) -> String {
    let mut ident = String::new();
    let mut index = start;
    while index < end && (chars[index].is_ascii_alphanumeric() || chars[index] == '_') {
        ident.push(chars[index]);
        index += 1;
    }
    ident
}

fn skip_to_identifier_end(chars: &[char], start: usize) -> usize {
    let mut index = start;
    while index < chars.len() && (chars[index].is_ascii_alphanumeric() || chars[index] == '_') {
        index += 1;
    }
    index
}

fn is_forbidden_ident(ident: &str) -> bool {
    FORBIDDEN_FACADE_SEGMENTS.contains(&ident)
}

/// Record a forbidden identifier under its governing path-head token name,
/// threading the `compat` escape-hatch infix through the hit label.
fn record_forbidden_ident(ident: &str, compat: bool, hits: &mut Vec<String>) {
    if is_forbidden_ident(ident) {
        if compat {
            hits.push(format!("{FACADE_HEAD}::compat::{ident}"));
        } else {
            hits.push(format!("{FACADE_HEAD}::{ident}"));
        }
    }
}

/// Record the leading identifier of one brace-group member span, threading
/// its `::` continuations (so a compat escape-hatch infix inside the group,
/// e.g. `{compat::workspace_index::WorkspaceIndex}`, stays detected) and
/// descending into nested groups (for example
/// `workspace::{workspace_index::Location}`).
fn record_member(chars: &[char], start: usize, end: usize, compat: bool, hits: &mut Vec<String>) {
    let member_start = skip_whitespace(chars, start);
    if member_start >= end {
        return;
    }
    let ident = read_identifier(chars, member_start, end);
    let mut thread_compat = compat;
    if ident == "compat" && !thread_compat {
        thread_compat = true;
    }
    record_forbidden_ident(&ident, thread_compat, hits);
    let mut cursor = member_start + ident.len();
    loop {
        cursor = skip_whitespace(chars, cursor);
        if !(cursor + 1 < end && chars[cursor] == ':' && chars[cursor + 1] == ':') {
            break;
        }
        cursor = skip_whitespace(chars, cursor + 2);
        let segment_end = skip_to_identifier_end(chars, cursor).min(end);
        let segment = read_identifier(chars, cursor, segment_end);
        if segment.is_empty() {
            break;
        }
        if segment == "compat" && !thread_compat {
            thread_compat = true;
        } else {
            record_forbidden_ident(&segment, thread_compat, hits);
        }
        cursor = segment_end;
    }
    while cursor < end {
        if chars[cursor] == '{' {
            cursor = scan_brace_group(chars, cursor + 1, thread_compat, hits);
        } else {
            cursor += 1;
        }
    }
}

/// Walk one brace group starting just past `{`. Split top-level members on
/// commas and return the index just past the matching `}`.
fn scan_brace_group(chars: &[char], start: usize, compat: bool, hits: &mut Vec<String>) -> usize {
    let mut depth = 1usize;
    let mut member_start = start;
    let mut index = start;
    while index < chars.len() {
        match chars[index] {
            '{' => {
                depth += 1;
                index += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    record_member(chars, member_start, index, compat, hits);
                    return index + 1;
                }
                index += 1;
            }
            ',' if depth == 1 => {
                record_member(chars, member_start, index, compat, hits);
                member_start = index + 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    index
}

/// Consume one `::`-continuation of a `perl_parser` path head. A leading
/// `compat` segment is transparent: scanning resumes after it with the
/// `compat` infix enabled so escape-hatch consumers stay detected. Returns
/// the index the outer scan should resume from.
fn scan_facade_path(chars: &[char], start: usize, compat: bool, hits: &mut Vec<String>) -> usize {
    let mut cursor = skip_whitespace(chars, start);
    if chars.get(cursor) != Some(&':') {
        return start;
    }
    cursor = skip_whitespace(chars, cursor + 1);
    if chars.get(cursor) != Some(&':') {
        return start;
    }
    cursor = skip_whitespace(chars, cursor + 1);
    match chars.get(cursor) {
        Some('{') => scan_brace_group(chars, cursor + 1, compat, hits),
        Some(_) => {
            let ident_end = skip_to_identifier_end(chars, cursor);
            let ident = read_identifier(chars, cursor, ident_end);
            if ident == "compat" && !compat {
                return scan_facade_path(chars, ident_end, true, hits).max(start);
            }
            record_forbidden_ident(&ident, compat, hits);
            ident_end.max(start)
        }
        None => start,
    }
}

fn forbidden_facade_references(code: &str) -> Vec<String> {
    let chars: Vec<char> = code.chars().collect();
    let head: Vec<char> = FACADE_HEAD.chars().collect();
    let mut hits: Vec<String> = Vec::new();
    let mut index = 0;
    while index + head.len() <= chars.len() {
        if chars[index..index + head.len()] != head[..]
            || index > 0 && (chars[index - 1].is_ascii_alphanumeric() || chars[index - 1] == '_')
        {
            index += 1;
            continue;
        }
        index = scan_facade_path(&chars, index + head.len(), false, &mut hits);
    }
    hits.sort();
    hits.dedup();
    let aliases = facade_aliases(code);
    for (alias, target) in aliases {
        let alias_chars: Vec<char> = alias.chars().collect();
        for cursor in 0..=chars.len().saturating_sub(alias_chars.len()) {
            if chars[cursor..cursor + alias_chars.len()] == alias_chars[..]
                && (cursor == 0
                    || (!chars[cursor - 1].is_ascii_alphanumeric() && chars[cursor - 1] != '_'))
                && chars.get(cursor + alias_chars.len()) == Some(&':')
                && chars.get(cursor + alias_chars.len() + 1) == Some(&':')
            {
                hits.push(target.clone());
            }
        }
    }
    hits.sort();
    hits.dedup();
    hits
}

/// Resolve aliases introduced by `use` statements so renamed facade roots and
/// brace members remain inside the recurrence guard's governed population.
fn facade_aliases(code: &str) -> BTreeMap<String, String> {
    let compact = code.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut aliases = BTreeMap::new();
    for statement in compact.split(';') {
        let Some(mut path) = statement.trim().strip_prefix("use ") else {
            continue;
        };
        if let Some((root, alias)) = path.split_once(" as ") {
            if root == FACADE_HEAD {
                aliases.insert(alias.trim().to_string(), FACADE_HEAD.to_string());
            } else if let Some(target) = resolve_facade_path(root, &aliases) {
                aliases.insert(alias.trim().to_string(), target);
            }
            continue;
        }
        if !path.starts_with(FACADE_HEAD) {
            continue;
        }
        if let (Some(open), Some(close)) = (path.find('{'), path.rfind('}')) {
            path = &path[open + 1..close];
            for member in path.split(',') {
                let Some((member_path, alias)) = member.trim().split_once(" as ") else {
                    continue;
                };
                if let Some(target) = resolve_facade_path(member_path.trim(), &aliases) {
                    aliases.insert(alias.trim().to_string(), target);
                }
            }
        }
    }
    aliases
}

fn resolve_facade_path(path: &str, aliases: &BTreeMap<String, String>) -> Option<String> {
    let mut segments = path.split("::");
    let first = segments.next()?.trim();
    let mut target = if first == FACADE_HEAD {
        FACADE_HEAD.to_string()
    } else if first == "compat" || is_forbidden_ident(first) {
        format!("{FACADE_HEAD}::{first}")
    } else {
        aliases.get(first)?.clone()
    };
    if is_forbidden_ident(first) {
        return Some(target);
    }
    for segment in segments {
        let segment = segment.trim();
        if segment == "compat" || is_forbidden_ident(segment) {
            target = format!("{target}::{segment}");
            if is_forbidden_ident(segment) {
                return Some(target);
            }
        } else if segment == "*" {
            return None;
        }
    }
    None
}

fn collect_rs_files(
    dir: &Path,
    relative: &str,
    found: &mut Vec<String>,
    failures: &mut Vec<String>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            failures.push(format!("read {}: {error}", dir.display()));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                failures.push(format!("entry under {}: {error}", dir.display()));
                continue;
            }
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_relative = format!("{relative}/{name}");
        if child_relative.starts_with(FACADE_CRATE_PREFIX) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, &child_relative, found, failures);
        } else if name.ends_with(".rs") {
            found.push(child_relative);
        }
    }
}

fn unregistered_facade_imports() -> (Vec<(String, String)>, Vec<String>) {
    let root = repo_root();
    let mut files = Vec::new();
    let mut failures = Vec::new();
    for scan_root in SCAN_ROOTS {
        collect_rs_files(&root.join(scan_root), scan_root, &mut files, &mut failures);
    }
    files.sort();

    let mut violations = Vec::new();
    for relative in files {
        let source = match fs::read_to_string(root.join(&relative)) {
            Ok(source) => source,
            Err(error) => {
                failures.push(format!("read {relative}: {error}"));
                continue;
            }
        };
        for hit in forbidden_facade_references(&code_without_comments(&source)) {
            if !TEMPORARY_EXCEPTIONS
                .iter()
                .any(|exception| exception.path == relative && exception.token == hit)
            {
                violations.push((relative.clone(), hit));
            }
        }
    }
    (violations, failures)
}

#[test]
fn no_consumer_imports_workspace_or_index_authority_through_perl_parser() {
    let (violations, failures) = unregistered_facade_imports();
    assert!(
        failures.is_empty(),
        "governed scan must reach every governed file (issue #11389): {failures:?}"
    );
    assert!(
        violations.is_empty(),
        "consumers must not import workspace/index authority through \
         perl-parser facade paths (issue #11389); migrate to perl_workspace \
         or register an owned exception: {violations:?}"
    );
}

#[test]
fn temporary_exceptions_are_unique_owned_and_still_consumed() {
    let root = repo_root();
    let mut unique = std::collections::BTreeSet::new();
    for exception in TEMPORARY_EXCEPTIONS {
        assert!(
            unique.insert((exception.path, exception.token)),
            "duplicate exception for {} / {}",
            exception.path,
            exception.token
        );
        assert!(exception.owner_issue.starts_with('#'), "exception needs an owning issue");
        assert!(
            !exception.removal_condition.trim().is_empty(),
            "exception needs a removal condition"
        );

        assert!(
            fs::read_to_string(root.join(exception.path))
                .is_ok_and(|source| code_without_comments(&source).contains(exception.token)),
            "stale exception {} / {} must be removed",
            exception.path,
            exception.token
        );
    }
}

#[test]
fn scan_roots_cover_all_root_level_workspace_members_and_target_kinds() {
    assert_eq!(SCAN_ROOTS, &["crates", "xtask", "fuzz"]);
    for relative in ["xtask/src", "xtask/tests", "xtask/examples", "fuzz/fuzz_targets"] {
        assert!(
            SCAN_ROOTS.iter().any(|root| relative == *root || relative.starts_with(&format!("{root}/"))),
            "governed target root is outside the scan denominator: {relative}"
        );
    }
}

#[test]
fn single_line_direct_path_imports_are_rejected() {
    let source = "\
use perl_parser::workspace_index::WorkspaceIndex;
use perl_parser::workspace_index::{SymKind, SymbolKey};
use perl_parser::workspace::document_store::DocumentStore;
use perl_parser::workspace::DocumentStore;
use perl_parser::document_store::DocumentStore;
use perl_parser::workspace_index;
use perl_parser::document_store;
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(
        hits,
        vec![
            "perl_parser::document_store".to_string(),
            "perl_parser::workspace".to_string(),
            "perl_parser::workspace_index".to_string(),
        ]
    );
}

#[test]
fn multi_line_brace_pre_image_is_rejected() {
    let source = "\
use perl_parser::{
    Parser,
    workspace_index::{DegradationReason, IndexState},
};
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(hits, vec!["perl_parser::workspace_index".to_string()]);
}

#[test]
fn single_line_brace_group_is_rejected() {
    let source = "use perl_parser::{Parser, workspace_index::WorkspaceIndex, document_store::DocumentStore};\n";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(
        hits,
        vec!["perl_parser::document_store".to_string(), "perl_parser::workspace_index".to_string(),]
    );
}

#[test]
fn compat_escape_hatch_paths_are_rejected() {
    let direct = "use perl_parser::compat::workspace_index::WorkspaceIndex;\n";
    let hits = forbidden_facade_references(&code_without_comments(direct));
    assert_eq!(hits, vec!["perl_parser::compat::workspace_index".to_string()]);

    let braced = "use perl_parser::compat::{document_store, workspace_index};\n";
    let hits = forbidden_facade_references(&code_without_comments(braced));
    assert_eq!(
        hits,
        vec![
            "perl_parser::compat::document_store".to_string(),
            "perl_parser::compat::workspace_index".to_string(),
        ]
    );

    let grouped_at_head =
        "use perl_parser::{compat::workspace_index::WorkspaceIndex};\n";
    let grouped_hits = forbidden_facade_references(&code_without_comments(grouped_at_head));
    assert_eq!(
        grouped_hits,
        vec!["perl_parser::compat::workspace_index".to_string()],
        "a compat escape hatch grouped at the crate head must stay detected"
    );

    let spaced = "use perl_parser :: compat :: workspace_index ;\n";
    let hits = forbidden_facade_references(&code_without_comments(spaced));
    assert_eq!(hits, vec!["perl_parser::compat::workspace_index".to_string()]);
}

#[test]
fn bare_compat_module_import_remains_allowed() {
    let source = "use perl_parser::compat;\nuse perl_parser::prelude::*;\n";
    assert!(forbidden_facade_references(&code_without_comments(source)).is_empty());
}

#[test]
fn whitespace_between_path_segments_is_normalized_before_matching() {
    let source = "use perl_parser :: {\n    workspace_index :: WorkspaceIndex ,\n};\nlet x =\n    perl_parser\n        ::\n        document_store ;\n";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(
        hits,
        vec!["perl_parser::document_store".to_string(), "perl_parser::workspace_index".to_string(),]
    );
}

#[test]
fn comments_cannot_hide_or_create_violations() {
    let hidden = "// use perl_parser::workspace_index::WorkspaceIndex;\n\
                  /* use perl_parser::{\n       workspace_index::WorkspaceIndex,\n   }; */\n\
                  let ok = 1;\n";
    assert!(forbidden_facade_references(&code_without_comments(hidden)).is_empty());

    let allowed = "use perl_parser::Parser;\
                  \n/* perl_parser::workspace_index */\
                  \n// perl_parser::document_store\n";
    assert!(forbidden_facade_references(&code_without_comments(allowed)).is_empty());
}

#[test]
fn literals_cannot_hide_or_create_violations() {
    let source = r#"
let ordinary = "perl_parser::workspace_index::WorkspaceIndex";
let raw = r###"perl_parser::document_store::DocumentStore"###;
let character = ':';
let marker = "//"; use perl_parser::workspace_index::WorkspaceIndex;
"#;
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(hits, vec!["perl_parser::workspace_index".to_string()]);
}

#[test]
fn renamed_facade_imports_cannot_bypass_the_guard() {
    let source = "use perl_parser as parser;\nuse parser::workspace_index::WorkspaceIndex;\n\
use perl_parser::{workspace_index as index, document_store as docs};\n\
use index::WorkspaceIndex; use docs::DocumentStore;\n";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(
        hits,
        vec![
            "perl_parser::document_store".to_string(),
            "perl_parser::workspace_index".to_string(),
        ]
    );
}

#[test]
fn parser_authority_and_canonical_owner_members_remain_allowed() {
    let source = "\
use perl_parser::{Node, NodeKind, Parser, SourceLocation};
use perl_parser::{
    ast::{Node as AstNode, NodeKind},
    error, parser, position,
};
use perl_workspace::{
    document_store::DocumentStore,
    workspace_index::{SymKind, WorkspaceIndex},
};
use perl_workspace::workspace::workspace_rename;
use perl_parser::workspace_refactor::WorkspaceRefactor;
use perl_parser::index::SymbolKey;
";
    assert!(forbidden_facade_references(&code_without_comments(source)).is_empty());
}

#[test]
fn boundary_check_rejects_longer_path_prefixes_and_other_heads() {
    let source = "\
mod workspace_index_helpers;
use perl_parser::workspace_indexers_local::Thing;
use perl_parser::workspaces_local::Other;
use my_perl_parser::workspace_index::Wrong;
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert!(hits.is_empty(), "unexpected boundary hits: {hits:?}");
}
