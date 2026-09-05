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
//! are rejected exactly like their single-line equivalents. Every segment of a
//! `::` chain is walked, not just the leading one, so a wrapper module
//! re-exporting semantic authority (`perl_parser::wrapper::semantic::Bar`,
//! or the brace member `wrapper::symbol::Table`) is reported too. The facade's root
//! re-export surface (`use perl_parser::SemanticAnalyzer`, brace members such
//! as `SymbolTable`) is detected as well: until #11379 removes those exports,
//! they remain semantic authority and consumers must import from
//! `perl_semantic_analyzer` instead.
//!
//! Normalization elides string literals as well as comments, so the guards'
//! own detector fixtures do not read as violations and no source needs a
//! whole-file exclusion: a real facade import in a fixture-bearing file is
//! executable code and is still reported. Character literals are tracked only
//! far enough to keep `'"'` from opening a phantom string; a forbidden token
//! cannot fit in one, and a `'` that does not close after a single character
//! is a lifetime and is left alone.
//!
//! Crate renames are resolved first: `use perl_parser as pp;` makes `pp` an
//! additional path head for that source, and violations are always reported
//! under the crate's real name so a rename cannot change the message.

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
/// Normalize a source before matching: elide comments *and* string literals,
/// leaving executable code.
///
/// Eliding string literals is what lets the facade guards be scanned like
/// every other governed source. They carry the forbidden tokens deliberately,
/// as fixtures proving the detector rejects them, and those fixtures live in
/// string literals; a real `use perl_parser::semantic::…` in the same file is
/// executable code and still reported. Excluding such files wholesale would
/// leave governed Rust consumers outside the contract — the exact hole this
/// guard exists to close.
///
/// Literal forms handled: ordinary `"…"` with backslash escapes, raw strings
/// `r"…"` / `r#"…"#` at any hash depth, and the `b` byte-string and `c`
/// C-string prefixes, alone or combined with `r` (`br"…"`, `cr#"…"#`).
/// Character literals are deliberately not tracked: a forbidden token cannot
/// fit in one, and `'` is ambiguous with lifetimes, so treating it as a
/// literal opener would swallow real code.
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
        } else if block_depth == 0
            && let Some(next) = char_literal_end(&chars, index)
        {
            index = next;
        } else if block_depth == 0
            && let Some(next) = string_literal_end(&chars, index)
        {
            index = next;
        } else {
            if block_depth == 0 {
                out.push(chars[index]);
            }
            index += 1;
        }
    }
    out
}

/// If a character literal closes after exactly one character at `index`,
/// return the index just past it. A `'` that does not close that way is a
/// lifetime and is left alone.
///
/// This exists for one case: `'"'`. Without it that quote opens a phantom
/// string literal and elides every import until the next `"` in the file --
/// the unsafe direction for a recurrence guard. No forbidden token fits in a
/// character literal, so nothing else needs tracking here.
fn char_literal_end(chars: &[char], index: usize) -> Option<usize> {
    if chars[index] != '\'' {
        return None;
    }
    // A `'` immediately after an identifier character is a label or a lifetime
    // bound, never a literal opener.
    if index > 0 && (chars[index - 1].is_ascii_alphanumeric() || chars[index - 1] == '_') {
        return None;
    }
    let body_end = match chars.get(index + 1) {
        Some('\\') => index + 3,
        Some(_) => index + 2,
        None => return None,
    };
    (chars.get(body_end) == Some(&'\'')).then_some(body_end + 1)
}

/// If a string literal opens at `index`, return the index just past its close.
/// An unterminated literal consumes the remainder, which keeps the scan on the
/// safe side of a malformed source rather than resuming mid-literal.
fn string_literal_end(chars: &[char], index: usize) -> Option<usize> {
    let mut cursor = index;
    if chars[cursor] == 'b' || chars[cursor] == 'c' {
        cursor += 1;
    }
    let raw = chars.get(cursor) == Some(&'r');
    if raw {
        cursor += 1;
    }
    let mut hashes = 0usize;
    if raw {
        while chars.get(cursor) == Some(&'#') {
            hashes += 1;
            cursor += 1;
        }
    }
    if chars.get(cursor) != Some(&'"') {
        return None;
    }
    // A prefix only opens a literal when it is not the tail of an identifier,
    // so `over"` or `my_r"` are not misread as literal openers.
    if index > 0 && (chars[index - 1].is_ascii_alphanumeric() || chars[index - 1] == '_') {
        return None;
    }
    cursor += 1;
    while cursor < chars.len() {
        if !raw && chars[cursor] == '\\' {
            cursor += 2;
            continue;
        }
        if chars[cursor] == '"' {
            let close = cursor + 1;
            if chars[close..].iter().take(hashes).filter(|c| **c == '#').count() == hashes {
                return Some(close + hashes);
            }
        }
        cursor += 1;
    }
    Some(chars.len())
}

fn skip_whitespace(chars: &[char], mut index: usize) -> usize {
    while index < chars.len() && chars[index].is_whitespace() {
        index += 1;
    }
    index
}

/// Read the identifier starting at `start`, returning its text and the index
/// just past it. One helper for both callers: the flat path bounds `end` at
/// `chars.len()`, the brace path at the member span.
fn read_identifier(chars: &[char], start: usize, end: usize) -> (String, usize) {
    let mut index = start;
    while index < end && (chars[index].is_ascii_alphanumeric() || chars[index] == '_') {
        index += 1;
    }
    (chars[start..index].iter().collect(), index)
}

/// If a `::` separator starts at or after `cursor`, return the index just past
/// it. Whitespace-insensitive, so `a :: b` reads like `a::b`.
fn skip_path_separator(chars: &[char], cursor: usize) -> Option<usize> {
    let first = skip_whitespace(chars, cursor);
    if chars.get(first) != Some(&':') {
        return None;
    }
    let second = skip_whitespace(chars, first + 1);
    if chars.get(second) != Some(&':') {
        return None;
    }
    Some(second + 1)
}

/// Walk one `::`-separated path chain inside `[start, end)`, recording the
/// forbidden module it reaches and descending into brace groups at any depth.
///
/// Checking only the leading identifier was the shared root cause of two
/// review findings: `perl_parser::wrapper::semantic::Bar` and the brace member
/// `wrapper::symbol::Table` both hide a facade segment behind a non-forbidden
/// one. A future wrapper module re-exporting semantic authority is exactly the
/// recurrence this guard exists to reject, so the whole chain is walked.
///
/// Two rules keep the widened walk from over-reporting:
///
/// - One hit per chain. `perl_parser::semantic::SemanticAnalyzer` is a single
///   violation of `semantic`; the item behind it is incidental, and the module
///   is the thing a consumer must migrate.
/// - `root_items_apply` gates [`FORBIDDEN_ROOT_REEXPORT_ITEMS`]. Those names
///   are semantic authority only at the crate root or beneath a forbidden
///   module — never under parser authority. Without this,
///   `perl_parser::workspace_index::{SymbolKind, VarKind}` would report the
///   workspace-index `SymbolKind`, which is not the semantic one.
fn record_path_chain(
    chars: &[char],
    start: usize,
    end: usize,
    hits: &mut Vec<String>,
    mut root_items_apply: bool,
) -> usize {
    let mut cursor = start;
    let mut recorded = false;
    loop {
        cursor = skip_whitespace(chars, cursor);
        if cursor >= end {
            return cursor;
        }
        if chars[cursor] == '{' {
            cursor = scan_brace_group(chars, cursor + 1, hits, root_items_apply);
        } else {
            let (ident, ident_end) = read_identifier(chars, cursor, end);
            if ident.is_empty() {
                return cursor;
            }
            let is_segment = FORBIDDEN_FACADE_SEGMENTS.contains(&ident.as_str());
            let is_root_item =
                root_items_apply && FORBIDDEN_ROOT_REEXPORT_ITEMS.contains(&ident.as_str());
            if !recorded && (is_segment || is_root_item) {
                hits.push(format!("{FACADE_HEAD}::{ident}"));
                recorded = true;
            }
            root_items_apply = is_segment;
            cursor = ident_end;
        }
        match skip_path_separator(chars, cursor) {
            Some(next) if next <= end => cursor = next,
            _ => return cursor,
        }
    }
}

/// Walk one brace group starting just past `{`. Split top-level members on
/// commas and return the index just past the matching `}`. Each member is a
/// path chain in its own right.
fn scan_brace_group(
    chars: &[char],
    start: usize,
    hits: &mut Vec<String>,
    root_items_apply: bool,
) -> usize {
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
                    record_path_chain(chars, member_start, index, hits, root_items_apply);
                    return index + 1;
                }
                index += 1;
            }
            ',' if depth == 1 => {
                record_path_chain(chars, member_start, index, hits, root_items_apply);
                member_start = index + 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    index
}

fn forbidden_facade_references(code: &str) -> Vec<String> {
    let mut hits: Vec<String> = Vec::new();
    for head in facade_heads(code) {
        collect_references_for_head(code, &head, &mut hits);
    }
    hits.sort();
    hits.dedup();
    hits
}

/// Every path head that resolves to `perl_parser` in this source: the crate
/// name itself, plus any `use perl_parser as alias;` rename. Without the
/// aliases, `use perl_parser as pp;` followed by `pp::semantic::_` reaches
/// semantic authority with the guard silent.
fn facade_heads(code: &str) -> Vec<String> {
    let chars: Vec<char> = code.chars().collect();
    let head: Vec<char> = FACADE_HEAD.chars().collect();
    let mut heads = vec![FACADE_HEAD.to_string()];
    let mut index = 0;
    while index + head.len() <= chars.len() {
        if chars[index..index + head.len()] != head[..]
            || index > 0 && (chars[index - 1].is_ascii_alphanumeric() || chars[index - 1] == '_')
        {
            index += 1;
            continue;
        }
        let after_head = index + head.len();
        index = after_head;
        let as_start = skip_whitespace(&chars, after_head);
        let (keyword, keyword_end) = read_identifier(&chars, as_start, chars.len());
        if keyword != "as" {
            continue;
        }
        let alias_start = skip_whitespace(&chars, keyword_end);
        let (alias, _) = read_identifier(&chars, alias_start, chars.len());
        if !alias.is_empty() && alias != "_" && !heads.contains(&alias) {
            heads.push(alias);
        }
    }
    heads
}

/// Scan one source for chains rooted at `head_name`. Hits are always recorded
/// under the crate's real name, so a rename cannot change what the failure
/// message says was violated.
fn collect_references_for_head(code: &str, head_name: &str, hits: &mut Vec<String>) {
    let chars: Vec<char> = code.chars().collect();
    let head: Vec<char> = head_name.chars().collect();
    let mut index = 0;
    while index + head.len() <= chars.len() {
        if chars[index..index + head.len()] != head[..]
            || index > 0 && (chars[index - 1].is_ascii_alphanumeric() || chars[index - 1] == '_')
        {
            index += 1;
            continue;
        }
        let after_head = index + head.len();
        // A bare crate reference with no `::` is the crate name, not a path
        // into it, and is never a violation.
        let Some(chain_start) = skip_path_separator(&chars, after_head) else {
            index = after_head;
            continue;
        };
        index = record_path_chain(&chars, chain_start, chars.len(), hits, true).max(after_head);
    }
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
fn crate_renames_are_resolved_before_matching() {
    let aliased = "use perl_parser as pp;\nuse pp::semantic::SemanticAnalyzer;\n";
    assert_eq!(
        forbidden_facade_references(&code_without_comments(aliased)),
        vec!["perl_parser::semantic".to_string()]
    );

    // A rename combined with a wrapper chain and a brace group.
    let aliased_chain = "use perl_parser as pp;\nuse pp::wrapper::{declaration, symbol};\n";
    assert_eq!(
        forbidden_facade_references(&code_without_comments(aliased_chain)),
        vec!["perl_parser::declaration".to_string(), "perl_parser::symbol".to_string(),]
    );

    // The rename only exists where it is declared: an unrelated crate keeping
    // the same short name must not be reported.
    let unrelated = "use perl_semantic_analyzer as pp;\nuse pp::semantic::SemanticAnalyzer;\n";
    assert!(forbidden_facade_references(&code_without_comments(unrelated)).is_empty());

    // Parser authority still passes under a rename.
    let allowed = "use perl_parser as pp;\nuse pp::Parser;\nuse pp::ast::Node;\n";
    assert!(forbidden_facade_references(&code_without_comments(allowed)).is_empty());
}

#[test]
fn a_quote_character_literal_does_not_open_a_phantom_string() {
    // `'"'` is a character literal, not a string opener. Treating its quote as
    // one elides every import until the next `"` in the file.
    let with_quote_char = "let quote = '\"';\nuse perl_parser::declaration::ParentMap;\n";
    assert_eq!(
        forbidden_facade_references(&code_without_comments(with_quote_char)),
        vec!["perl_parser::declaration".to_string()]
    );

    let escaped = "let quote = '\\\"';\nuse perl_parser::symbol::SymbolTable;\n";
    assert_eq!(
        forbidden_facade_references(&code_without_comments(escaped)),
        vec!["perl_parser::symbol".to_string()]
    );

    // A lifetime is not a literal and must not consume what follows.
    let lifetime = "fn take<'a>(input: &'a str) -> &'a str { input }\n\
                    use perl_parser::scope_analyzer::ScopeAnalyzer;\n";
    assert_eq!(
        forbidden_facade_references(&code_without_comments(lifetime)),
        vec!["perl_parser::scope_analyzer".to_string()]
    );
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
    for guard in [
        "xtask/tests/parser_semantic_facade_consumers.rs",
        "xtask/tests/parser_tdd_facade_consumers.rs",
    ] {
        assert!(
            files.iter().any(|file| file == guard),
            "{guard} must be scanned like any other governed source; no file is excluded wholesale"
        );
    }
}

/// The guards carry forbidden tokens as fixtures, so they must be readable by
/// the scan without reporting themselves — and a genuine facade import in the
/// same file must still be reported. Whole-file exclusion would satisfy the
/// first and silently break the second.
#[test]
fn fixture_bearing_guard_files_are_scanned_without_reporting_their_own_fixtures() {
    let (violations, failures) = unregistered_facade_imports();
    assert!(failures.is_empty(), "scan must not fail: {failures:?}");
    assert!(violations.is_empty(), "detector fixtures must not read as violations: {violations:?}");
}

#[test]
fn a_real_import_beside_detector_fixtures_is_still_reported() {
    let fixture_bearing_guard = "\
const FIXTURE: &str = \"use perl_parser::semantic::SemanticAnalyzer;\";
const RAW_FIXTURE: &str = r#\"use perl_parser::{SymbolTable, Symbol};\"#;
// use perl_parser::type_inference::TypeEnvironment;
use perl_parser::declaration::ParentMap;
";
    let hits = forbidden_facade_references(&code_without_comments(fixture_bearing_guard));
    assert_eq!(
        hits,
        vec!["perl_parser::declaration".to_string()],
        "only the executable import may be reported; string and comment fixtures must not be"
    );
}

#[test]
fn raw_and_byte_string_literals_do_not_leak_or_swallow_code() {
    let source = "\
const A: &str = r\"use perl_parser::semantic::X;\";
const B: &[u8] = b\"use perl_parser::symbol::Y;\";
const C: &str = r##\"a \"# not a close\" use perl_parser::scope_analyzer::Z;\"##;
use perl_parser::type_inference::TypeEnvironment;
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(
        hits,
        vec!["perl_parser::type_inference".to_string()],
        "raw/byte literals must be elided whole, and must not swallow the code after them"
    );
}

#[test]
fn an_identifier_ending_in_a_literal_prefix_does_not_open_a_literal() {
    // `over` ends in `r`; treating that as a raw-string prefix would elide the
    // rest of the file and hide every import after it.
    let source = "\
let over\"x\" = 1;
use perl_parser::semantic::SemanticAnalyzer;
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(hits, vec!["perl_parser::semantic".to_string()]);
}

/// Review finding r3887038668: the flat path arm read only the first segment,
/// so a wrapper module re-exporting semantic authority was never checked.
#[test]
fn a_forbidden_segment_behind_a_wrapper_module_is_reported() {
    let source = "use perl_parser::wrapper::semantic::Bar;\n";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(hits, vec!["perl_parser::semantic".to_string()]);
}

/// Review finding r3887042840: the brace path descended only on `{`, so a
/// member's own `::` chain was walked past unchecked.
#[test]
fn a_forbidden_segment_behind_a_wrapper_inside_a_brace_member_is_reported() {
    let source = "use perl_parser::{Parser, wrapper::symbol::Table};\n";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(hits, vec!["perl_parser::symbol".to_string()]);
}

/// The widened chain walk must not turn parser authority into a violation.
/// `workspace_index` stays on `perl-parser` (#11377 leaves it there), and its
/// `SymbolKind` is not the semantic `SymbolKind` — reporting it would be a
/// false accept of the migration's own boundary.
#[test]
fn root_reexport_items_under_parser_authority_are_not_violations() {
    let source = "\
use perl_parser::workspace_index::{LspWorkspaceSymbol, SymbolKind, VarKind};
use perl_parser::ast::{Node, NodeKind};
";
    assert!(
        forbidden_facade_references(&code_without_comments(source)).is_empty(),
        "root re-export items apply at the crate root or beneath a forbidden module, not under \
         parser authority"
    );
}

/// One violation per chain: the module is what a consumer must migrate, and
/// the item behind it is incidental. Reporting both would double-count a
/// single import.
#[test]
fn a_chain_reports_its_module_once_not_every_semantic_name_in_it() {
    let source = "use perl_parser::semantic::SemanticAnalyzer;\n";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(hits, vec!["perl_parser::semantic".to_string()]);
}

/// A bare crate reference is not a path into the facade.
#[test]
fn a_bare_crate_reference_without_a_path_is_not_a_violation() {
    let source = "let name = perl_parser;\nextern crate perl_parser;\n";
    assert!(forbidden_facade_references(&code_without_comments(source)).is_empty());
}

/// Devin finding r3941604610: `c"…"` and `cr#"…"#` are Rust string literals
/// too. Leaving their contents executable would both report fixture text as a
/// violation and let a `//` inside one hide a later facade import.
#[test]
fn c_string_literals_are_elided_like_every_other_literal() {
    let source = "\
const A: &CStr = c\"use perl_parser::semantic::X;\";
const B: &CStr = cr#\"use perl_parser::symbol::Y;\"#;
const C: &[u8] = br\"use perl_parser::scope_analyzer::Z;\";
use perl_parser::type_inference::TypeEnvironment;
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(hits, vec!["perl_parser::type_inference".to_string()]);
}

/// The hiding direction for the same prefixes: a `//` inside a C string must
/// not comment out the rest of the file.
#[test]
fn a_comment_marker_inside_a_c_string_cannot_hide_a_later_import() {
    let source = "\
const A: &CStr = c\"// not a comment\";
use perl_parser::semantic::SemanticAnalyzer;
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(hits, vec!["perl_parser::semantic".to_string()]);
}
