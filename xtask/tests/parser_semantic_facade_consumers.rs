//! Recurrence guard for the parser semantic-facade consumer cutover.
//!
//! Issue #11377 moved every PF02-owned residual consumer off semantic
//! authority imported through `perl-parser` compatibility paths. The facade
//! crate itself still owns those compatibility exports until #11379, so it is
//! excluded here. Any re-introduction elsewhere must register an owned,
//! conditioned exception below instead of silently returning.

use std::{fs, path::Path};

const FACADE_CRATE_PREFIX: &str = "crates/perl-parser/";

const FORBIDDEN_SEMANTIC_FACADE_TOKENS: &[&str] = &[
    "perl_parser::analysis",
    "perl_parser::semantic",
    "perl_parser::symbol",
    "perl_parser::declaration",
    "perl_parser::scope_analyzer",
    "perl_parser::type_inference",
];

struct TemporaryException {
    path: &'static str,
    token: &'static str,
    owner_issue: &'static str,
    removal_condition: &'static str,
}

const TEMPORARY_EXCEPTIONS: &[TemporaryException] = &[];

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must live beneath the repository root")
}

fn code_without_line_comments(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn contains_facade_token(code: &str, token: &str) -> bool {
    code.match_indices(token).any(|(index, _)| {
        code[index + token.len()..]
            .chars()
            .next()
            .is_none_or(|next| !(next.is_ascii_alphanumeric() || next == '_'))
    })
}

fn collect_rs_files(dir: &Path, relative: &str, found: &mut Vec<String>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|error| panic!("read {}: {error}", dir.display()));
    for entry in entries {
        let entry = entry.expect("directory entry readable");
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_relative = format!("{relative}/{name}");
        if child_relative.starts_with(FACADE_CRATE_PREFIX) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, &child_relative, found);
        } else if name.ends_with(".rs") {
            found.push(child_relative);
        }
    }
}

fn unregistered_facade_imports() -> Vec<(String, String)> {
    let root = repo_root();
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), "crates", &mut files);
    files.sort();

    let mut violations = Vec::new();
    for relative in files {
        let source = fs::read_to_string(root.join(&relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let code = code_without_line_comments(&source);
        for token in FORBIDDEN_SEMANTIC_FACADE_TOKENS {
            if contains_facade_token(&code, token)
                && !TEMPORARY_EXCEPTIONS
                    .iter()
                    .any(|exception| exception.path == relative && exception.token == *token)
            {
                violations.push((relative.clone(), (*token).to_string()));
            }
        }
    }
    violations
}

#[test]
fn no_consumer_imports_semantic_authority_through_perl_parser() {
    let violations = unregistered_facade_imports();
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

        let source = fs::read_to_string(root.join(exception.path))
            .unwrap_or_else(|error| panic!("read {}: {error}", exception.path));
        assert!(
            code_without_line_comments(&source).contains(exception.token),
            "stale exception {} / {} must be removed",
            exception.path,
            exception.token
        );
    }
}

#[test]
fn a_new_semantic_facade_import_is_rejected() {
    let source =
        "use perl_parser::semantic::SemanticAnalyzer;\nuse perl_parser::declaration::ParentMap;\n";
    let code = code_without_line_comments(source);
    for token in FORBIDDEN_SEMANTIC_FACADE_TOKENS.iter().copied() {
        assert_eq!(
            contains_facade_token(&code, token),
            matches!(token, "perl_parser::semantic" | "perl_parser::declaration"),
            "token {token} detection mismatch"
        );
    }
}

#[test]
fn boundary_check_rejects_longer_path_prefixes() {
    let source = "mod symbols;\nuse perl_parser::symbols_only_path::Thing;\n";
    let code = code_without_line_comments(source);
    assert!(!contains_facade_token(&code, "perl_parser::symbol"));
}
