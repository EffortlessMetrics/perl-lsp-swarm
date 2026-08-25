//! Recurrence guard for the residual TDD facade consumer cutover.
//!
//! Issue #11382 moved every remaining consumer off TDD/test-generation
//! authority imported through `perl-parser` compatibility paths (`tdd`,
//! `tdd_basic`, `tdd_workflow`, `test_generator`, `test_runner`, and their
//! root type re-exports). The facade crate itself still owns those
//! compatibility exports until #11385, so it is excluded here. Any
//! re-introduction elsewhere must register an owned, conditioned exception
//! below instead of silently returning.

use std::{fs, path::Path, path::PathBuf};

const FACADE_CRATE_PREFIX: &str = "crates/perl-parser/";

const FORBIDDEN_TDD_FACADE_TOKENS: &[&str] = &[
    "perl_parser::tdd_basic",
    "perl_parser::tdd_workflow",
    "perl_parser::tdd",
    "perl_parser::test_generator",
    "perl_parser::test_runner",
    "perl_parser::TestGenerator",
    "perl_parser::TestFramework",
    "perl_parser::TestRunner",
    "perl_parser::TddWorkflow",
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
    collect_rs_files(&root.join("crates"), "crates", &mut files, &mut failures);
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
        let code = code_without_line_comments(&source);
        for token in FORBIDDEN_TDD_FACADE_TOKENS {
            if contains_facade_token(&code, token)
                && !TEMPORARY_EXCEPTIONS
                    .iter()
                    .any(|exception| exception.path == relative && exception.token == *token)
            {
                violations.push((relative.clone(), (*token).to_string()));
            }
        }
    }
    (violations, failures)
}

#[test]
fn no_consumer_imports_tdd_authority_through_perl_parser() {
    let (violations, failures) = unregistered_facade_imports();
    assert!(
        failures.is_empty(),
        "governed scan must reach every crate file (issue #11382): {failures:?}"
    );
    assert!(
        violations.is_empty(),
        "consumers must not import TDD/test-generation authority through \
         perl-parser facade paths (issue #11382); migrate to perl_tdd_support \
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
                .is_ok_and(|source| code_without_line_comments(&source).contains(exception.token)),
            "stale exception {} / {} must be removed",
            exception.path,
            exception.token
        );
    }
}

#[test]
fn a_new_tdd_facade_import_is_rejected() {
    let source = "\
use perl_parser::tdd_basic::TestGenerator;
use perl_parser::tdd_workflow::TddWorkflow;
use perl_parser::tdd::WorkflowState;
use perl_parser::test_generator::{TestGenerator, TestFramework};
use perl_parser::test_runner::{TestKind, TestRunner};
use perl_parser::TestGenerator;
use perl_parser::TestFramework;
use perl_parser::TestRunner;
use perl_parser::TddWorkflow;
";
    let code = code_without_line_comments(source);
    for token in FORBIDDEN_TDD_FACADE_TOKENS.iter().copied() {
        assert!(contains_facade_token(&code, token), "token {token} detection mismatch");
    }
}

#[test]
fn boundary_check_rejects_longer_path_prefixes() {
    let code = code_without_line_comments(
        "use perl_parser::tddx::Other;\nuse perl_parser::test_runners_local::Thing;\n",
    );
    assert!(!contains_facade_token(&code, "perl_parser::tdd"));
    assert!(!contains_facade_token(&code, "perl_parser::test_runner"));
}
