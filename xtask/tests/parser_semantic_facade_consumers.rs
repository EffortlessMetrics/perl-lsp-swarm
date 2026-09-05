//! Recurrence guard for the parser semantic-facade consumer cutover.
//!
//! Issue #11377 moved every PF02-owned residual consumer off semantic
//! authority imported through `perl-parser` compatibility paths. The facade
//! crate itself still owns those compatibility exports until #11379, so it is
//! excluded here. Any re-introduction elsewhere must register an owned,
//! conditioned exception below instead of silently returning.
//!
//! Detection normalizes each source before matching so formatting cannot hide
//! a violation: line and block comments are stripped, then every
//! `perl_parser ::` path head and brace-group membership is scanned with
//! whitespace-insensitive boundaries. Multi-line forms like
//!
//! ```ignore
//! use perl_parser::{
//!     Parser,
//!     declaration::ParentMap,
//! };
//! ```
//!
//! are rejected exactly like their single-line equivalents. The facade's root
//! re-export surface (`use perl_parser::SemanticAnalyzer`, brace members such
//! as `SymbolTable`) is detected as well: until #11379 removes those exports,
//! they remain semantic authority and consumers must import from
//! `perl_semantic_analyzer` instead.
//!
//! Known limitation: comment stripping is lexical and does not tokenize
//! string literals, so a `//` or unbalanced `/*` inside a string can hide the
//! remainder of that line from the scan. This mirrors the precision of the
//! sibling TDD facade guard and stays on the safe side for import statements.

use std::{
    fs,
    path::{Path, PathBuf},
};

const FACADE_CRATE_PREFIX: &str = "crates/perl-parser/";
const FACADE_HEAD: &str = "perl_parser";

/// Scan roots for governed consumer sources: whole workspace members, not
/// hand-picked subdirectories. Naming `xtask` and `fuzz` at member granularity
/// keeps `xtask/tests`, `xtask/examples`, and any directory a member grows
/// later inside the guard automatically (#14300 wave 2); the sibling TDD guard
/// still carries the narrow literal until the shared discovery primitive lands
/// under that issue.
const SCAN_ROOTS: &[&str] = &["crates", "xtask", "fuzz"];

/// Directory names never scanned: build output is generated, not governed
/// source, and can contain vendored copies of facade consumers.
const SKIPPED_DIR_NAMES: &[&str] = &["target"];

/// Facade-guard sources excluded from their own scan. These files carry the
/// forbidden tokens deliberately, as string fixtures proving the detector
/// rejects them; because comment stripping is lexical and does not tokenize
/// string literals, scanning them would report the detector's own evidence as
/// a violation. The list is exact paths, never a prefix, so a real consumer
/// cannot hide behind it.
const GUARD_SELF_PATHS: &[&str] = &[
    "xtask/tests/parser_semantic_facade_consumers.rs",
    "xtask/tests/parser_tdd_facade_consumers.rs",
];

/// Leading path segments of `perl-parser` modules that re-export semantic
/// authority.
const FORBIDDEN_FACADE_SEGMENTS: &[&str] =
    &["analysis", "declaration", "scope_analyzer", "semantic", "symbol", "type_inference"];

/// Root re-export items of those same semantic modules. These only match in a
/// `perl_parser ::` path-head or brace-group membership context, never bare.
const FORBIDDEN_ROOT_REEXPORT_ITEMS: &[&str] = &[
    "HoverInfo",
    "IssueKind",
    "PerlType",
    "ScalarType",
    "ScopeAnalyzer",
    "ScopeIssue",
    "SemanticAnalyzer",
    "SemanticModel",
    "SemanticToken",
    "SemanticTokenModifier",
    "SemanticTokenType",
    "Symbol",
    "SymbolExtractor",
    "SymbolKind",
    "SymbolReference",
    "SymbolTable",
    "TypeBasedCompletion",
    "TypeConstraint",
    "TypeEnvironment",
    "TypeInferenceEngine",
    "TypeLocation",
];

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

/// Strip line comments and (nested) block comments while preserving all other
/// structure, including newlines and brace groups.
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

fn is_forbidden_ident(ident: &str) -> bool {
    FORBIDDEN_FACADE_SEGMENTS.contains(&ident) || FORBIDDEN_ROOT_REEXPORT_ITEMS.contains(&ident)
}

fn record_forbidden_ident(ident: &str, hits: &mut Vec<String>) {
    if is_forbidden_ident(ident) {
        hits.push(format!("{FACADE_HEAD}::{ident}"));
    }
}

/// Record the leading identifier of one brace-group member span, descending
/// into nested groups (for example `semantic::{HoverInfo}`).
fn record_member(chars: &[char], start: usize, end: usize, hits: &mut Vec<String>) {
    let member_start = skip_whitespace(chars, start);
    if member_start >= end {
        return;
    }
    let ident = read_identifier(chars, member_start, end);
    record_forbidden_ident(&ident, hits);
    let mut cursor = member_start + ident.len();
    while cursor < end {
        if chars[cursor] == '{' {
            cursor = scan_brace_group(chars, cursor + 1, hits);
        } else {
            cursor += 1;
        }
    }
}

/// Walk one brace group starting just past `{`. Split top-level members on
/// commas and return the index just past the matching `}`.
fn scan_brace_group(chars: &[char], start: usize, hits: &mut Vec<String>) -> usize {
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
                    record_member(chars, member_start, index, hits);
                    return index + 1;
                }
                index += 1;
            }
            ',' if depth == 1 => {
                record_member(chars, member_start, index, hits);
                member_start = index + 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    index
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
        let after_head = index + head.len();
        let mut cursor = skip_whitespace(&chars, after_head);
        if chars.get(cursor) != Some(&':') {
            index = after_head;
            continue;
        }
        cursor = skip_whitespace(&chars, cursor + 1);
        if chars.get(cursor) != Some(&':') {
            index = after_head;
            continue;
        }
        cursor = skip_whitespace(&chars, cursor + 1);
        match chars.get(cursor) {
            Some('{') => {
                index = scan_brace_group(&chars, cursor + 1, &mut hits);
            }
            Some(_) => {
                let ident_end = skip_to_identifier_end(&chars, cursor);
                let ident = read_identifier(&chars, cursor, ident_end);
                record_forbidden_ident(&ident, &mut hits);
                index = ident_end.max(after_head);
            }
            None => break,
        }
    }
    hits.sort();
    hits.dedup();
    hits
}

fn skip_to_identifier_end(chars: &[char], start: usize) -> usize {
    let mut index = start;
    while index < chars.len() && (chars[index].is_ascii_alphanumeric() || chars[index] == '_') {
        index += 1;
    }
    index
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
        if child_relative.starts_with(FACADE_CRATE_PREFIX)
            || child_relative == "crates/perl-parser"
            || SKIPPED_DIR_NAMES.contains(&name.as_str())
            || GUARD_SELF_PATHS.contains(&child_relative.as_str())
        {
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
fn no_consumer_imports_semantic_authority_through_perl_parser() {
    let (violations, failures) = unregistered_facade_imports();
    assert!(
        failures.is_empty(),
        "governed scan must reach every governed file (issue #11377): {failures:?}"
    );
    assert!(
        violations.is_empty(),
        "consumers must not import semantic authority through perl-parser \
         facade paths (issue #11377); migrate to perl_semantic_analyzer or \
         register an owned exception: {violations:?}"
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
fn single_line_direct_path_imports_are_rejected() {
    let source =
        "use perl_parser::semantic::SemanticAnalyzer;\nuse perl_parser::declaration::ParentMap;\n";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(
        hits,
        vec!["perl_parser::declaration".to_string(), "perl_parser::semantic".to_string()]
    );
}

#[test]
fn multi_line_brace_pre_image_is_rejected() {
    let source = "\
use perl_parser::{
    Parser,
    declaration::ParentMap,
};
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(hits, vec!["perl_parser::declaration".to_string()]);
}

#[test]
fn multi_line_brace_groups_with_nested_members_and_root_items_are_rejected() {
    let source = "\
use perl_parser::{
    Parser,
    SemanticAnalyzer,
    symbol::{
        SymbolTable,
        SymbolExtractor,
    },
    type_inference::TypeInferenceEngine,
};
use perl_parser::{SemanticModel, Symbol};
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(
        hits,
        vec![
            "perl_parser::SemanticAnalyzer".to_string(),
            "perl_parser::SemanticModel".to_string(),
            "perl_parser::Symbol".to_string(),
            "perl_parser::SymbolExtractor".to_string(),
            "perl_parser::SymbolTable".to_string(),
            "perl_parser::symbol".to_string(),
            "perl_parser::type_inference".to_string(),
        ]
    );
}

#[test]
fn whitespace_between_path_segments_is_normalized_before_matching() {
    let source = "use perl_parser :: {\n    semantic :: SemanticAnalyzer ,\n};\nlet x =\n    perl_parser\n        ::\n        scope_analyzer ;\n";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(
        hits,
        vec!["perl_parser::scope_analyzer".to_string(), "perl_parser::semantic".to_string()]
    );
}

#[test]
fn comments_cannot_hide_or_create_violations() {
    let hidden = "// use perl_parser::semantic::SemanticAnalyzer;\n\
                  /* use perl_parser::{\n       declaration::ParentMap,\n   }; */\n\
                  let ok = 1;\n";
    assert!(forbidden_facade_references(&code_without_comments(hidden)).is_empty());

    let allowed = "use perl_parser::Parser;\n/* perl_parser::semantic */\n";
    assert!(forbidden_facade_references(&code_without_comments(allowed)).is_empty());
}

#[test]
fn parser_authority_members_remain_allowed() {
    let source = "\
use perl_parser::{Node, NodeKind, Parser, SourceLocation};
use perl_parser::{
    ast::{Node as AstNode, NodeKind},
    error, parser, position,
};
use perl_parser::{TokenKind, TokenStream, ParseError, RecoverySalvageClass};
";
    assert!(forbidden_facade_references(&code_without_comments(source)).is_empty());
}

#[test]
fn boundary_check_rejects_longer_path_prefixes_and_other_heads() {
    let source = "\
mod symbols;
use perl_parser::symbols_only_path::Thing;
use perl_parser::semantics_local::Other;
use my_perl_parser::semantic::Wrong;
use crate::perl_parser_wrapper::analysis::Also;
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert!(hits.is_empty(), "unexpected boundary hits: {hits:?}");
}

/// #14300 wave 2: the guard's coverage hole was `SCAN_ROOTS` naming
/// hand-picked subdirectories, so a facade consumer landing in
/// `xtask/tests/**` or `xtask/examples/**` merged unseen. Roots are now whole
/// workspace members. This proves the collector actually reaches those
/// directories rather than trusting the constant's shape.
#[test]
fn scan_reaches_every_directory_of_each_governed_member() {
    let root = repo_root();
    let mut files = Vec::new();
    let mut failures = Vec::new();
    for scan_root in SCAN_ROOTS {
        collect_rs_files(&root.join(scan_root), scan_root, &mut files, &mut failures);
    }
    assert!(failures.is_empty(), "scan roots must all be readable: {failures:?}");

    for required in ["crates/", "xtask/src/", "xtask/tests/", "fuzz/fuzz_targets/"] {
        assert!(
            files.iter().any(|file| file.starts_with(required)),
            "no source collected under {required}; SCAN_ROOTS no longer covers it"
        );
    }

    assert!(
        !files.iter().any(|file| file.starts_with("crates/perl-parser/")),
        "the facade crate owns its own compatibility exports until #11379 and must stay excluded"
    );
    assert!(
        !files.iter().any(|file| file.split('/').any(|segment| segment == "target")),
        "build output must never be scanned as governed source"
    );
    for guard in GUARD_SELF_PATHS {
        assert!(
            !files.iter().any(|file| file == guard),
            "{guard} carries forbidden tokens as detector fixtures and must stay excluded"
        );
    }
}
