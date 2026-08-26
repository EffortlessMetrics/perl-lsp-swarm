//! Occupancy for #12732: `clippy::collapsible_if` must not be masked in perl-parser.
//!
//! Two independent occupancy surfaces:
//! 1. Crate/file/item `allow`/`expect` attributes naming `clippy::collapsible_if`.
//!    Clippy `--all-targets` is silent if the blanket returns; this scan is the
//!    discriminator that still fails.
//! 2. `cargo clippy -p perl-parser --all-targets` with `--force-warn`, which
//!    pierces allows and fails if any live site remains.
//!
//! Scanner literals and comments containing the lint name do not count as occupancy.

use std::path::{Path, PathBuf};
use std::process::Command;

const LINT: &str = "clippy::collapsible_if";

fn rest_at(source: &str, i: usize) -> &str {
    source.get(i..).unwrap_or("")
}

fn scan_char_literal(source: &str, i: usize) -> Option<usize> {
    let rest = rest_at(source, i);
    if !rest.starts_with('\'') {
        return None;
    }
    let mut chars = rest.char_indices().skip(1);
    let (_, first) = chars.next()?;
    if first == '\\' {
        let _escaped = chars.next()?;
        let (off, quote) = chars.next()?;
        if quote == '\'' {
            return Some(i + off + quote.len_utf8());
        }
        return None;
    }
    let (off, quote) = chars.next()?;
    if quote == '\'' { Some(i + off + quote.len_utf8()) } else { None }
}

fn scan_comment_or_string(source: &str, i: usize) -> Option<usize> {
    let rest = rest_at(source, i);
    if rest.starts_with("//") {
        return Some(match rest.find('\n') {
            Some(n) => i + n + 1,
            None => source.len(),
        });
    }
    if rest.starts_with("/*") {
        return Some(match rest.get(2..).and_then(|tail| tail.find("*/")) {
            Some(n) => i + 2 + n + 2,
            None => source.len(),
        });
    }
    if let Some(end) = scan_char_literal(source, i) {
        return Some(end);
    }
    if rest.starts_with('r') {
        let bytes = rest.as_bytes();
        let mut hashes = 0;
        let mut j = 1;
        while j < bytes.len() && bytes[j] == b'#' {
            hashes += 1;
            j += 1;
        }
        if j < bytes.len() && bytes[j] == b'"' {
            j += 1;
            let close = format!("\"{}", "#".repeat(hashes));
            return rest
                .get(j..)
                .and_then(|tail| tail.find(&close))
                .map(|n| i + j + n + close.len());
        }
    }
    if rest.starts_with('"') {
        let mut escaped = false;
        for (off, ch) in rest.char_indices().skip(1) {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                return Some(i + off + ch.len_utf8());
            }
        }
        return Some(source.len());
    }
    None
}

fn find_matching_paren(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < source.len() {
        if let Some(end) = scan_comment_or_string(source, i) {
            i = end;
            continue;
        }
        let Some(ch) = source[i..].chars().next() else {
            break;
        };
        if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(i);
            }
        }
        i += ch.len_utf8();
    }
    None
}

fn lint_list_contains_collapsible_if(inner: &str) -> bool {
    inner.split(',').any(|item| {
        let name =
            item.split("//").next().unwrap_or(item).split("/*").next().unwrap_or(item).trim();
        name == LINT
    })
}

fn allow_attrs_name_collapsible_if(source: &str) -> bool {
    let mut i = 0;
    while i < source.len() {
        if let Some(end) = scan_comment_or_string(source, i) {
            i = end;
            continue;
        }
        let rest = rest_at(source, i);
        let marker = if rest.starts_with("#![allow(") {
            Some("#![allow(")
        } else if rest.starts_with("#[allow(") {
            Some("#[allow(")
        } else if rest.starts_with("#![expect(") {
            Some("#![expect(")
        } else if rest.starts_with("#[expect(") {
            Some("#[expect(")
        } else {
            None
        };
        if let Some(marker) = marker {
            let open = i + marker.len() - 1;
            if let Some(close) = find_matching_paren(source, open) {
                if lint_list_contains_collapsible_if(&source[open + 1..close]) {
                    return true;
                }
                i = close + 1;
                continue;
            }
        }
        let Some(ch) = source[i..].chars().next() else {
            break;
        };
        i += ch.len_utf8();
    }
    false
}

fn visit_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            visit_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn crate_rs_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    visit_rs_files(Path::new(env!("CARGO_MANIFEST_DIR")), &mut files);
    files.sort();
    files
}

fn json_quoted_after(haystack: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":\"");
    let start = haystack.find(&pat)? + pat.len();
    let rest = haystack.get(start..)?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn clippy_collapsible_if_hits() -> Result<Vec<String>, String> {
    let output = Command::new(env!("CARGO"))
        .args([
            "clippy",
            "-p",
            "perl-parser",
            "--all-targets",
            "--locked",
            "--offline",
            "--no-deps",
            "--message-format=json",
            "--",
            "--force-warn",
            LINT,
            "-A",
            "missing_docs",
            "-A",
            "clippy::print_stdout",
            "-A",
            "clippy::print_stderr",
        ])
        .output()
        .map_err(|error| format!("failed to spawn cargo clippy: {error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut hits = Vec::new();
    for line in stdout.lines() {
        if !line.contains("compiler-message") || !line.contains(LINT) {
            continue;
        }
        if json_quoted_after(line, "reason").as_deref() != Some("compiler-message") {
            continue;
        }
        let has_lint =
            line.contains(&format!("\"{LINT}\"")) || line.contains(&format!("\"code\":\"{LINT}\""));
        if !has_lint {
            continue;
        }
        let file = json_quoted_after(line, "file_name").unwrap_or_else(|| "?".to_string());
        if !file.contains("perl-parser") {
            continue;
        }
        let line_no = line
            .split("\"line_start\":")
            .nth(1)
            .and_then(|rest| {
                rest.chars()
                    .take_while(|ch| ch.is_ascii_digit())
                    .collect::<String>()
                    .parse::<u64>()
                    .ok()
            })
            .unwrap_or(0);
        hits.push(format!("{file}:{line_no}"));
    }
    hits.sort();
    hits.dedup();
    Ok(hits)
}

#[test]
fn crate_level_allow_fixture_is_detected() {
    let with_blanket = r#"
#![allow(
    clippy::too_many_lines,
    clippy::collapsible_if,
    clippy::collapsible_match,
)]
"#;
    assert!(
        allow_attrs_name_collapsible_if(with_blanket),
        "crate-level allow list naming collapsible_if must occupy"
    );
}

#[test]
fn item_allow_fixture_is_detected() {
    let item = r#"
#[allow(clippy::collapsible_if)]
fn nested() {}
"#;
    assert!(allow_attrs_name_collapsible_if(item), "item allow must occupy");
}

#[test]
fn expect_attr_fixture_is_detected() {
    let expected = r#"
#![expect(clippy::collapsible_if, reason = "temporary")]
"#;
    assert!(allow_attrs_name_collapsible_if(expected), "expect attr must occupy");
}

#[test]
fn comment_and_string_literals_do_not_occupy() {
    let comment = r#"
// clippy::collapsible_if
#![allow(clippy::too_many_lines)]
"#;
    assert!(
        !allow_attrs_name_collapsible_if(comment),
        "comment mentioning the lint must not occupy"
    );

    let string = r#"
const MSG: &str = "clippy::collapsible_if";
"#;
    assert!(
        !allow_attrs_name_collapsible_if(string),
        "string literal mentioning the lint must not occupy"
    );
}

#[test]
fn occupancy_requires_allow_attr_not_scanner_literals() {
    let scanner_only = r#"
fn helper(source: &str) -> bool {
    source.contains("clippy::collapsible_if")
}
"#;
    assert!(
        !allow_attrs_name_collapsible_if(scanner_only),
        "scanner/fixture literals must not satisfy occupancy without an allow attr"
    );
}

#[test]
fn perl_parser_sources_do_not_allow_collapsible_if() {
    let files = crate_rs_files();
    assert!(!files.is_empty(), "crate must contain .rs files to occupy");
    let mut occupied = Vec::new();
    for path in &files {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        if allow_attrs_name_collapsible_if(&source) {
            occupied.push(path.display().to_string());
        }
    }
    assert!(
        occupied.is_empty(),
        "perl-parser must not retain allow/expect(clippy::collapsible_if); found in:\n{}",
        occupied.join("\n")
    );
}

#[test]
fn lib_rs_crate_allow_list_does_not_name_collapsible_if() {
    let lib = include_str!("../src/lib.rs");
    assert!(
        !allow_attrs_name_collapsible_if(lib),
        "crates/perl-parser/src/lib.rs crate-level allow list must not name clippy::collapsible_if"
    );
}

fn assert_no_collapsible_if_hits(result: Result<Vec<String>, String>) {
    let hits = match result {
        Ok(hits) => hits,
        Err(error) => vec![format!("instrument-failure: {error}")],
    };
    assert!(
        hits.is_empty(),
        "cargo clippy -p perl-parser --all-targets still hits collapsible_if:\n{}",
        hits.join("\n")
    );
}

#[test]
fn clippy_all_targets_has_no_collapsible_if_hits() {
    assert_no_collapsible_if_hits(clippy_collapsible_if_hits());
}
