//! Recurrence guard for the parser semantic-facade consumer cutover.
//!
//! Issue #11377 moved every PF02-owned residual consumer off semantic
//! authority imported through `perl-parser` compatibility paths. The facade
//! crate itself still owns those compatibility exports until #11379, so it is
//! excluded here. Any re-introduction elsewhere must register an owned,
//! conditioned exception below instead of silently returning.
//!
//! # Detection is parser-backed, not lexical
//!
//! Each source is parsed with `syn` and the resulting syntax tree is walked.
//! An earlier revision of this guard approximated Rust with a hand-rolled
//! scanner and accumulated eleven review findings against the *normalizer*
//! rather than the claim -- string, raw, byte and C-string literals; `'"'` and
//! `b'"'` character literals; crate renames; `{self as alias}`; raw
//! identifiers; `::` path heads; and a lone `:` in type position. Two of those
//! were introduced by fixes for the previous two. Parsing removes the whole
//! class: a literal is a literal, `r#type` is one identifier, and
//! `other::perl_parser` is unambiguously not the crate, because the parser
//! already decided all of it.
//!
//! Both surfaces are covered. `use` trees catch imports:
//!
//! ```ignore
//! use perl_parser::{
//!     Parser,
//!     declaration::ParentMap,
//! };
//! ```
//!
//! and every other `Path` catches references in type, expression, and generic
//! position (`fn f(x: perl_parser::semantic::SemanticAnalyzer)`).
//!
//! Every segment of a path is examined, not just the leading one, so a wrapper
//! module re-exporting semantic authority (`perl_parser::wrapper::semantic::Bar`,
//! or the brace member `wrapper::symbol::Table`) is reported too. The facade's
//! root re-export surface (`use perl_parser::SemanticAnalyzer`, brace members
//! such as `SymbolTable`) is detected as well: until #11379 removes those
//! exports, they remain semantic authority and consumers must import from
//! `perl_semantic_analyzer` instead.
//!
//! Two rules keep the walk from over-reporting. One hit per path chain, since
//! `perl_parser::semantic::SemanticAnalyzer` is a single violation of the
//! module a consumer must migrate. And [`FORBIDDEN_ROOT_REEXPORT_ITEMS`]
//! applies only at the crate root or beneath an already-forbidden module,
//! never under parser authority -- without that,
//! `perl_parser::workspace_index::{SymbolKind, VarKind}` would report the
//! workspace-index `SymbolKind`, which is not the semantic one.
//!
//! Crate renames are resolved first: `use perl_parser as pp;` and
//! `use perl_parser::{self as pp};` both make `pp` an additional path head for
//! that source, and violations are always reported under the crate's real name
//! so a rename cannot change the message.
//!
//! A source that does not parse is a scan **failure**, never a silent skip.
//!
//! Known boundary (over-reporting, not hiding): a rename applies for the whole
//! source, so a nested scope rebinding the same name to an unrelated module
//! would still read as facade-rooted. Honouring lexical shadowing needs name
//! resolution rather than parsing, which `syn` does not do; the failure is
//! loud, names its file and token, and is absorbed by an owned
//! `TEMPORARY_EXCEPTIONS` entry. Reviewed and accepted rather than overlooked
//! (#14300 owns the shared alias primitive for this guard and its TDD sibling).

use std::{
    fs,
    path::{Path, PathBuf},
};

use syn::{UseTree, visit::Visit};

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

/// `.rs` files that are not Rust source and therefore cannot consume anything.
///
/// Parse failure is otherwise a hard scan failure: a guard that cannot read a
/// file must say so rather than pass it, since a file `syn` cannot parse but
/// `rustc` can would be a hole. Each entry is an exact path with an owning
/// issue, and `unparseable_entries_are_really_unparseable` proves every one
/// still fails to parse, so this list cannot quietly exclude a healthy file.
const UNPARSEABLE_NON_SOURCE: &[TemporaryException] = &[TemporaryException {
    path: "crates/perl-lsp-rs/tests/fixtures/parser/comprehensive_syntax_fixtures.rs",
    token: "expected `;`",
    owner_issue: "#11377",
    removal_condition: "line 570 is Perl (`use Scalar::Util qw(looks_like_number);`) pasted into \
                        a Rust file. Nothing declares `mod fixtures;`, so the whole \
                        crates/perl-lsp-rs/tests/fixtures tree is never compiled and the error \
                        is invisible to rustc. Remove when that tree is either compiled or \
                        deleted.",
}];

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
/// Whether `ident` names a `perl-parser` module that re-exports semantic
/// authority.
fn is_forbidden_segment(ident: &str) -> bool {
    FORBIDDEN_FACADE_SEGMENTS.contains(&ident)
}

/// Whether `ident` is one of the facade's root re-exported semantic items.
fn is_forbidden_root_item(ident: &str) -> bool {
    FORBIDDEN_ROOT_REEXPORT_ITEMS.contains(&ident)
}

/// Record `ident` when it is forbidden in this position, returning whether it
/// was. `root_items_apply` gates the root re-export surface; see the module
/// docs for why it is not unconditional.
fn record(ident: &str, root_items_apply: bool, hits: &mut Vec<String>) -> bool {
    if is_forbidden_segment(ident) || (root_items_apply && is_forbidden_root_item(ident)) {
        hits.push(format!("{FACADE_HEAD}::{ident}"));
        return true;
    }
    false
}

/// Walk one `use` tree already known to be rooted at the facade crate.
///
/// `recorded` carries the one-hit-per-chain rule along a linear path; a brace
/// group starts each member as its own chain, so
/// `symbol::{SymbolTable, SymbolExtractor}` reports the module and both items.
fn walk_use_tree(tree: &UseTree, root_items_apply: bool, recorded: bool, hits: &mut Vec<String>) {
    match tree {
        UseTree::Path(path) => {
            let ident = path.ident.to_string();
            let hit = !recorded && record(&ident, root_items_apply, hits);
            walk_use_tree(&path.tree, is_forbidden_segment(&ident), recorded || hit, hits);
        }
        UseTree::Name(name) => {
            if !recorded {
                record(&name.ident.to_string(), root_items_apply, hits);
            }
        }
        UseTree::Rename(rename) => {
            if !recorded {
                record(&rename.ident.to_string(), root_items_apply, hits);
            }
        }
        UseTree::Group(group) => {
            for item in &group.items {
                walk_use_tree(item, root_items_apply, false, hits);
            }
        }
        UseTree::Glob(_) => {}
    }
}

/// Walk the segments of a non-`use` path whose head is the facade crate.
fn walk_path_segments(segments: &[String], hits: &mut Vec<String>) {
    let mut root_items_apply = true;
    for segment in segments {
        if record(segment, root_items_apply, hits) {
            return;
        }
        root_items_apply = is_forbidden_segment(segment);
    }
}

/// The `use` tree beneath `head`, when this tree is rooted at that head.
fn tree_under_head<'a>(tree: &'a UseTree, head: &str) -> Option<&'a UseTree> {
    match tree {
        UseTree::Path(path) if path.ident == head => Some(&path.tree),
        _ => None,
    }
}

/// Collect every additional path head bound to the facade crate in this file:
/// `use perl_parser as pp;` and `use perl_parser::{self as pp};`.
fn facade_heads(file: &syn::File) -> Vec<String> {
    struct Collector {
        heads: Vec<String>,
    }
    impl Collector {
        fn push(&mut self, alias: String) {
            if alias != "_" && !self.heads.contains(&alias) {
                self.heads.push(alias);
            }
        }
        fn scan(&mut self, tree: &UseTree) {
            match tree {
                // `use perl_parser as pp;`
                UseTree::Rename(rename) if rename.ident == FACADE_HEAD => {
                    self.push(rename.rename.to_string());
                }
                UseTree::Path(path) if path.ident == FACADE_HEAD => {
                    if let UseTree::Group(group) = &*path.tree {
                        for item in &group.items {
                            // `use perl_parser::{self as pp};`
                            if let UseTree::Rename(rename) = item
                                && rename.ident == "self"
                            {
                                self.push(rename.rename.to_string());
                            }
                        }
                    }
                }
                UseTree::Group(group) => {
                    for item in &group.items {
                        self.scan(item);
                    }
                }
                _ => {}
            }
        }
    }
    impl<'ast> Visit<'ast> for Collector {
        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            self.scan(&item.tree);
            syn::visit::visit_item_use(self, item);
        }
    }
    let mut collector = Collector { heads: vec![FACADE_HEAD.to_string()] };
    collector.visit_file(file);
    collector.heads
}

/// Every facade semantic reference in `source`, reported under the crate's
/// real name. `Err` when the source does not parse: a guard that cannot read a
/// file must say so rather than pass it.
fn try_facade_references(source: &str) -> Result<Vec<String>, String> {
    let file = syn::parse_file(source).map_err(|error| format!("parse: {error}"))?;
    let heads = facade_heads(&file);

    struct Scanner<'a> {
        heads: &'a [String],
        hits: Vec<String>,
    }
    impl<'a, 'ast> Visit<'ast> for Scanner<'a> {
        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            for head in self.heads {
                if let Some(tree) = tree_under_head(&item.tree, head) {
                    walk_use_tree(tree, true, false, &mut self.hits);
                }
            }
            syn::visit::visit_item_use(self, item);
        }

        fn visit_path(&mut self, path: &'ast syn::Path) {
            let segments: Vec<String> =
                path.segments.iter().map(|segment| segment.ident.to_string()).collect();
            if let Some((head, rest)) = segments.split_first()
                && self.heads.iter().any(|known| known == head)
            {
                walk_path_segments(rest, &mut self.hits);
            }
            syn::visit::visit_path(self, path);
        }
    }

    let mut scanner = Scanner { heads: &heads, hits: Vec::new() };
    scanner.visit_file(&file);
    let mut hits = scanner.hits;
    hits.sort();
    hits.dedup();
    Ok(hits)
}

/// Test-facing wrapper over [`try_facade_references`] for fixture sources,
/// which are always valid Rust.
fn facade_references(source: &str) -> Vec<String> {
    match try_facade_references(source) {
        Ok(hits) => hits,
        Err(error) => {
            // A fixture that does not parse is a broken test, not a finding.
            assert!(error.is_empty(), "fixture source must parse: {error}");
            Vec::new()
        }
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
        match try_facade_references(&source) {
            Ok(hits) => {
                for hit in hits {
                    if !TEMPORARY_EXCEPTIONS
                        .iter()
                        .any(|exception| exception.path == relative && exception.token == hit)
                    {
                        violations.push((relative.clone(), hit));
                    }
                }
            }
            Err(error) => {
                if !UNPARSEABLE_NON_SOURCE.iter().any(|known| known.path == relative) {
                    failures.push(format!("{relative}: {error}"));
                }
            }
        }
    }
    (violations, failures)
}

#[test]
fn crate_renames_are_resolved_before_matching() {
    let aliased = "use perl_parser as pp;\nuse pp::semantic::SemanticAnalyzer;\n";
    assert_eq!(facade_references(aliased), vec!["perl_parser::semantic".to_string()]);

    // A rename combined with a wrapper chain and a brace group.
    let aliased_chain = "use perl_parser as pp;\nuse pp::wrapper::{declaration, symbol};\n";
    assert_eq!(
        facade_references(aliased_chain),
        vec!["perl_parser::declaration".to_string(), "perl_parser::symbol".to_string(),]
    );

    // The rename only exists where it is declared: an unrelated crate keeping
    // the same short name must not be reported.
    let unrelated = "use perl_semantic_analyzer as pp;\nuse pp::semantic::SemanticAnalyzer;\n";
    assert!(facade_references(unrelated).is_empty());

    // Parser authority still passes under a rename.
    let allowed = "use perl_parser as pp;\nuse pp::Parser;\nuse pp::ast::Node;\n";
    assert!(facade_references(allowed).is_empty());
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

        // An exception is only honest while the reference it excuses is still
        // there. Ask the detector itself rather than matching text, so an
        // exception cannot be kept alive by the token appearing in a comment
        // or a string literal.
        assert!(
            fs::read_to_string(root.join(exception.path)).is_ok_and(|source| {
                try_facade_references(&source)
                    .is_ok_and(|hits| hits.iter().any(|hit| hit == exception.token))
            }),
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
    let hits = facade_references(source);
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
    let hits = facade_references(source);
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
    let hits = facade_references(source);
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
fn whitespace_between_path_segments_is_irrelevant() {
    let source = "use perl_parser :: {\n    semantic :: SemanticAnalyzer ,\n};\n\
                  fn f() {\n    let _x =\n        perl_parser\n            ::\n            \
                  scope_analyzer :: ScopeAnalyzer ;\n}\n";
    assert_eq!(
        facade_references(source),
        vec!["perl_parser::scope_analyzer".to_string(), "perl_parser::semantic".to_string()]
    );
}

#[test]
fn comments_cannot_hide_or_create_violations() {
    let hidden = "// use perl_parser::semantic::SemanticAnalyzer;\n\
                  /* use perl_parser::{\n       declaration::ParentMap,\n   }; */\n\
                  fn f() {\n    let ok = 1;\n}\n";
    assert!(facade_references(hidden).is_empty());

    let allowed = "use perl_parser::Parser;\n/* perl_parser::semantic */\n";
    assert!(facade_references(allowed).is_empty());
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
    assert!(facade_references(source).is_empty());
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
    let hits = facade_references(source);
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
    let hits = facade_references(fixture_bearing_guard);
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
    let hits = facade_references(source);
    assert_eq!(
        hits,
        vec!["perl_parser::type_inference".to_string()],
        "raw/byte literals must be elided whole, and must not swallow the code after them"
    );
}

/// Review finding r3887038668: the flat path arm read only the first segment,
/// so a wrapper module re-exporting semantic authority was never checked.
#[test]
fn a_forbidden_segment_behind_a_wrapper_module_is_reported() {
    let source = "use perl_parser::wrapper::semantic::Bar;\n";
    let hits = facade_references(source);
    assert_eq!(hits, vec!["perl_parser::semantic".to_string()]);
}

/// Review finding r3887042840: the brace path descended only on `{`, so a
/// member's own `::` chain was walked past unchecked.
#[test]
fn a_forbidden_segment_behind_a_wrapper_inside_a_brace_member_is_reported() {
    let source = "use perl_parser::{Parser, wrapper::symbol::Table};\n";
    let hits = facade_references(source);
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
        facade_references(source).is_empty(),
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
    let hits = facade_references(source);
    assert_eq!(hits, vec!["perl_parser::semantic".to_string()]);
}

/// A bare crate reference is not a path into the facade.
#[test]
fn a_bare_crate_reference_without_a_path_is_not_a_violation() {
    let source = "extern crate perl_parser;\nfn f() {\n    let name = perl_parser;\n}\n";
    assert!(facade_references(source).is_empty());
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
    let hits = facade_references(source);
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
    let hits = facade_references(source);
    assert_eq!(hits, vec!["perl_parser::semantic".to_string()]);
}

/// A rename declaration is itself whitespace-insensitive. Devin finding
/// r3941731269 named the multiline form specifically; `35d1f4c3` already handles
/// it, but nothing pinned it, and an unpinned behaviour in a recurrence guard
/// is exactly what silently regresses.
#[test]
fn a_crate_rename_split_across_lines_is_still_resolved() {
    let split = "use perl_parser\n    as\n    pp;\nuse pp::semantic::SemanticAnalyzer;\n";
    assert_eq!(facade_references(split), vec!["perl_parser::semantic".to_string()]);

    // The negative control travels with it: an unrelated crate renamed across
    // lines to the same short name must still not be reported.
    let unrelated = "use perl_semantic_analyzer\n    as\n    pp;\nuse pp::semantic::X;\n";
    assert!(facade_references(unrelated).is_empty());
}

/// Devin finding r3941643602: `b'"'` is a byte character literal, but its
/// quote opened a phantom string that elided every import until the next `"`.
/// The fixture deliberately contains no further double quote, so the import
/// below it is only visible if the literal was consumed whole.
/// A literal cannot hide code from a parser, but the assertion is kept: it is
/// the invariant the old lexical normalizer repeatedly broke, and it costs one
/// parse to hold.
#[test]
fn a_character_literal_cannot_hide_a_later_import() {
    let source = "use perl_parser::declaration::ParentMap;\n\
                  fn f() {\n    let quote = b\'\"\';\n    let plain = \'\"\';\n}\n";
    assert_eq!(facade_references(source), vec!["perl_parser::declaration".to_string()]);

    // A lifetime is not a literal and must not consume what follows.
    let lifetime = "fn take<\'a>(input: &\'a str) -> &\'a str { input }\n\
                    use perl_parser::scope_analyzer::ScopeAnalyzer;\n";
    assert_eq!(facade_references(lifetime), vec!["perl_parser::scope_analyzer".to_string()]);
}

/// Devin finding r3941638226: `use perl_parser::{self as pp};` binds the crate
/// root under an alias exactly as `as` does, so paths through it are governed.
#[test]
fn a_braced_self_alias_registers_the_crate_head() {
    let braced = "use perl_parser::{self as pp};\nuse pp::semantic::SemanticAnalyzer;\n";
    assert_eq!(facade_references(braced), vec!["perl_parser::semantic".to_string()]);

    // Mixed with ordinary members, and split across lines.
    let mixed =
        "use perl_parser::{\n    self as pp,\n    Parser,\n};\nuse pp::type_inference::X;\n";
    assert_eq!(facade_references(mixed), vec!["perl_parser::type_inference".to_string()]);

    // Negative control: `self as pp` under an unrelated crate binds nothing here.
    let unrelated = "use perl_semantic_analyzer::{self as pp};\nuse pp::semantic::X;\n";
    assert!(facade_references(unrelated).is_empty());

    // Parser authority through the aliased head still passes.
    let allowed = "use perl_parser::{self as pp};\nuse pp::ast::Node;\n";
    assert!(facade_references(allowed).is_empty());
}

/// Devin finding r3941... : `other::perl_parser` is an unrelated module that
/// happens to share the crate's name. Registering an alias from it, or
/// reporting a path through it, blocks an innocent consumer.
#[test]
fn a_same_named_module_under_another_path_is_not_the_facade_crate() {
    for source in [
        "use other::perl_parser::{self as pp};\nuse pp::semantic::X;\n",
        "use other::perl_parser as pp;\nuse pp::semantic::X;\n",
        "use other::perl_parser::semantic::X;\n",
        "use crate::vendor::perl_parser::symbol::Table;\n",
    ] {
        assert!(
            facade_references(source).is_empty(),
            "a later path segment sharing the crate name is not the facade: {source:?}"
        );
    }

    // The real crate-root forms must still be detected, so the boundary check
    // cannot become a blanket escape.
    for (source, expected) in [
        ("use perl_parser::{self as pp};\nuse pp::semantic::X;\n", "perl_parser::semantic"),
        ("use perl_parser as pp;\nuse pp::symbol::Table;\n", "perl_parser::symbol"),
        ("use perl_parser::declaration::ParentMap;\n", "perl_parser::declaration"),
    ] {
        assert_eq!(
            facade_references(source),
            vec![expected.to_string()],
            "crate-root form must still be reported: {source:?}"
        );
    }
}

/// Devin finding r3941...: `r#type` is one raw identifier. Reading only `r`
/// records an alias that never matches its own use sites, so the facade path
/// behind it stays invisible — the unsafe direction.
#[test]
fn a_raw_identifier_alias_is_read_whole() {
    let braced = "use perl_parser::{self as r#type};\nuse r#type::semantic::SemanticAnalyzer;\n";
    assert_eq!(facade_references(braced), vec!["perl_parser::semantic".to_string()]);

    let renamed = "use perl_parser as r#match;\nuse r#match::symbol::Table;\n";
    assert_eq!(facade_references(renamed), vec!["perl_parser::symbol".to_string()]);

    // Negative control: an unrelated crate under a raw alias binds nothing.
    let unrelated = "use perl_semantic_analyzer as r#type;\nuse r#type::semantic::X;\n";
    assert!(facade_references(unrelated).is_empty());

    // A raw string must still be read as a literal, not as an identifier.
    let raw_string =
        "const A: &str = r#\"use perl_parser::semantic::X;\"#;\nuse perl_parser::symbol::T;\n";
    assert_eq!(facade_references(raw_string), vec!["perl_parser::symbol".to_string()]);
}

/// Devin finding r3941691...: the first path-head guard tested for a single
/// `:`, so a type annotation suppressed the scan entirely — the unsafe
/// direction, and a defect introduced by the fix for the `other::perl_parser`
/// false positive. Only a full `::` separates path segments.
#[test]
fn a_single_colon_does_not_suppress_the_scan() {
    for (source, expected) in [
        ("fn f(x: perl_parser::semantic::SemanticAnalyzer) {}\n", "perl_parser::semantic"),
        (
            "fn f() {\n    let v: Vec<perl_parser::symbol::Symbol> = vec![];\n}\n",
            "perl_parser::symbol",
        ),
        (
            "struct S {\n    field: perl_parser::type_inference::TypeEnvironment,\n}\n",
            "perl_parser::type_inference",
        ),
        // An absolute path still names the crate, not someone else's segment.
        ("use ::perl_parser::semantic::SemanticAnalyzer;\n", "perl_parser::semantic"),
    ] {
        assert_eq!(
            facade_references(source),
            vec![expected.to_string()],
            "a lone colon or leading `::` must not hide a facade reference: {source:?}"
        );
    }

    // The continuation cases the guard exists to exclude must still be excluded,
    // so this fix cannot undo the one before it.
    for source in [
        "use other::perl_parser::semantic::X;\n",
        "use crate::perl_parser::semantic::X;\n",
        "use other::perl_parser as pp;\nuse pp::semantic::X;\n",
    ] {
        assert!(
            facade_references(source).is_empty(),
            "a real path segment before `::` still means this is not the crate: {source:?}"
        );
    }
}

/// The unparseable list is an escape hatch, so it must not be able to hide a
/// file that actually parses — that would be a silent hole in the scan.
#[test]
fn unparseable_entries_are_really_unparseable() {
    let root = repo_root();
    for entry in UNPARSEABLE_NON_SOURCE {
        assert!(entry.owner_issue.starts_with('#'), "entry needs an owning issue");
        assert!(!entry.removal_condition.trim().is_empty(), "entry needs a removal condition");
        let source = fs::read_to_string(root.join(entry.path)).unwrap_or_default();
        assert!(!source.is_empty(), "{} must exist and be readable", entry.path);
        assert!(
            try_facade_references(&source).is_err(),
            "{} parses now and must leave UNPARSEABLE_NON_SOURCE",
            entry.path
        );
    }
}
