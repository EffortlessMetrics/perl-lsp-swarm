//! Test-file discovery: `tests[]` + `oracles[]` extraction (Campaign 31 Phase
//! B PR 6, perl-lsp-swarm#2593). This module extracts Test::More / Test2::V0
//! test facts + oracle facts from `.t` files in the workspace and maps them
//! into the ripr schema's `tests` + `oracles` arrays. The oracle-kind lookup
//! itself lives in [`super::oracles`]; this module owns walking `.t` files,
//! framework detection, and range/provenance assembly.

use perl_parser_core::line_index::LineIndex;
use perl_parser_core::{Node, NodeKind, Parser};
use perl_symbol::surface::{SymbolRefKind, extract_symbol_refs};
use serde_json::{Value, json};

use super::discovery::collect_t_files;
use super::oracles::oracle_for;

/// Recognized test frameworks, most-specific first. Maps a module name (as it
/// appears in a parsed `use`) to the ripr `test.framework` wire enum. Order
/// matters: the `Test2::*` bundles and the `Test::Exception`/`Test::Fatal`
/// add-ons are checked before `Test::More` so a file that pulls both a bundle
/// and an add-on reports the more specific harness. Parser-backed
/// (`NodeKind::Use.module`), never a substring match.
const TEST_FRAMEWORKS: &[(&str, &str)] = &[
    ("Test2::V1", "Test2::V1"),
    ("Test2::V0", "Test2::V0"),
    ("Test2::Suite", "Test2::Suite"),
    ("Test::Exception", "Test::Exception"),
    ("Test::Fatal", "Test::Fatal"),
    ("Test::More", "Test::More"),
];

/// Emit parser-backed `tests[]`, `oracles[]`, `provenance[]`, and
/// `limitations[]` for the `ripr-perl-facts-v1` packet (#3293 PR 4).
///
/// For each `.t` file under `root/t`, this parses the file with
/// `perl-parser-core` and:
/// - detects the framework from parsed `use` statements (`NodeKind::Use.module`,
///   never `content.contains`) and emits one `test` fact with the file's **real
///   full-file range** plus a `test_discovery` provenance carrying the `use`
///   statement's range;
/// - emits one `oracle` fact per recognized assertion **call node**
///   (`extract_symbol_refs`, filtered to `SubroutineCall`), with the call's
///   **real source range**, a kind/strength from [`ASSERTION_ORACLES`], and the
///   call's source text as `expression`; and
/// - records limitations for unparseable files, recognized-framework-but-no-
///   oracle files (wrapped/aliased/dynamic helpers), and the narrower schema
///   representation (single call range, no observed/expected sub-ranges).
///
/// No string-scan assertion counting and no placeholder `1:1` ranges. On a parse
/// failure the file still yields a `test` fact (framework `unknown`) plus a
/// limitation — never silent, never string-fallback oracles.
pub(crate) fn emit_tests_and_oracles(
    root: &str,
) -> (Vec<Value>, Vec<Value>, Vec<Value>, Vec<Value>) {
    let mut tests = Vec::new();
    let mut oracles = Vec::new();
    let mut provenance = Vec::new();
    let mut limitations = Vec::new();

    let mut t_files = collect_t_files(std::path::Path::new(root));
    // Deterministic order: sort discovered test files by repo-relative path.
    t_files.sort_by(|a, b| a.1.cmp(&b.1));

    let mut any_oracles = false;

    for (_full_path, relative_path, content) in t_files {
        let file_id = format!("file:{relative_path}");
        let test_id = format!("test:{relative_path}");
        let import_prov_id = format!("prov:test_discovery:{file_id}");

        // Parse first (borrowing `content`), so `content` can then move into
        // `LineIndex` without a per-file clone.
        let parsed = {
            let mut parser = Parser::new(&content);
            parser.parse()
        };
        let ast = match parsed {
            Ok(ast) => ast,
            Err(_) => {
                // Never silent, never string-fallback: emit the test fact with
                // an unknown framework and record why oracles are absent.
                let content_len = content.len();
                let line_index = LineIndex::new(content);
                let full_range = full_file_range(&line_index, content_len);
                tests.push(test_fact(
                    "unknown",
                    &test_id,
                    &file_id,
                    &relative_path,
                    &full_range,
                    &import_prov_id,
                ));
                provenance.push(test_discovery_provenance(
                    &import_prov_id,
                    &file_id,
                    None,
                    "medium",
                ));
                limitations.push(json!({
                    "limitation_id": format!("test-parse-failed:{file_id}"),
                    "kind": "parse_failure",
                    "message": format!(
                        "could not parse test file `{relative_path}`; emitted the test fact with unknown framework and no oracles"
                    ),
                    "evidence_refs": [file_id],
                }));
                continue;
            }
        };

        // Detect the framework and collect each oracle's span + source-text
        // `expression` while `content` is still borrowed — then `content` can
        // move into `LineIndex` (no clone) for the range conversions.
        let (framework, use_span) = framework_from_ast(&ast);
        let oracle_hits: Vec<(String, &'static str, &'static str, usize, usize, String)> =
            extract_symbol_refs(&ast)
                .into_iter()
                .filter(|reference| reference.kind == SymbolRefKind::SubroutineCall)
                .filter_map(|reference| {
                    oracle_for(&reference.name).map(|(kind, strength)| {
                        let (start_byte, end_byte) = reference.full_span;
                        let expression = call_expression(&content, start_byte, end_byte);
                        (reference.name, kind, strength, start_byte, end_byte, expression)
                    })
                })
                .collect();

        let content_len = content.len();
        let line_index = LineIndex::new(content);
        let full_range = full_file_range(&line_index, content_len);
        let import_range = use_span.map(|(s, e)| range_string(&line_index, s, e));

        tests.push(test_fact(
            framework,
            &test_id,
            &file_id,
            &relative_path,
            &full_range,
            &import_prov_id,
        ));
        provenance.push(test_discovery_provenance(
            &import_prov_id,
            &file_id,
            import_range,
            if framework == "unknown" { "medium" } else { "high" },
        ));

        // Oracles from the parsed call sites (byte spans → real ranges).
        let oracle_prov_id = format!("prov:oracle_extraction:{file_id}");
        let mut file_oracles = 0usize;
        for (name, kind, strength, start_byte, end_byte, expression) in oracle_hits {
            let ((sl, sc), (el, ec)) = line_index.range(start_byte, end_byte);
            oracles.push(json!({
                "oracle_id": format!("oracle:{relative_path}:{name}:{start_byte}-{end_byte}"),
                "test_id": test_id,
                "kind": kind,
                "strength": strength,
                "target_owner_id": null,
                "expression": expression,
                "observed_sink": null,
                "expected_expression": null,
                "range": {"start_line": sl, "start_column": sc, "end_line": el, "end_column": ec},
                "confidence": "medium",
                "provenance_refs": [oracle_prov_id.clone()],
            }));
            file_oracles += 1;
        }

        if file_oracles > 0 {
            any_oracles = true;
            provenance.push(json!({
                "provenance_id": oracle_prov_id,
                "source": "oracle_extraction",
                "file_id": file_id,
                "range": null,
                "confidence": "medium",
            }));
        } else if framework != "unknown" {
            // A recognized framework but no extractable assertion: assertions may
            // be wrapped, aliased, or dynamically generated — flag, do not guess.
            limitations.push(json!({
                "limitation_id": format!("no-oracles:{file_id}"),
                "kind": "framework_indirection",
                "message": format!(
                    "`{relative_path}` uses {framework} but no supported assertion call was found; assertions may be wrapped, aliased, or dynamically generated"
                ),
                "evidence_refs": [file_id],
            }));
        }
    }

    // Representation note: the schema carries one call `range` + string
    // expressions per oracle, not separate observed/expected sub-ranges.
    if any_oracles {
        limitations.push(json!({
            "limitation_id": "oracle-representation",
            "kind": "narrowed_representation",
            "message": "oracle facts locate the whole assertion call range; the ripr-perl-facts-v1 schema has no observed/expected expression sub-range fields, so those are conveyed as string expressions only.",
            "evidence_refs": [],
        }));
    }

    (tests, oracles, provenance, limitations)
}

/// The whole-file range `(0,0)..(end)` as a schema range value.
fn full_file_range(line_index: &LineIndex, content_len: usize) -> Value {
    let ((sl, sc), (el, ec)) = line_index.range(0, content_len);
    json!({"start_line": sl, "start_column": sc, "end_line": el, "end_column": ec})
}

/// Build a schema-valid `test` fact. The `test` schema has no `owner_id`
/// (`additionalProperties: false`), so subtest ownership, when added later, must
/// ride `test_id` naming rather than a field. Test2 harnesses run under `yath`
/// or `prove`; everything else under `prove`.
fn test_fact(
    framework: &str,
    test_id: &str,
    file_id: &str,
    name: &str,
    range: &Value,
    provenance_id: &str,
) -> Value {
    let runner_hints =
        if framework.starts_with("Test2") { json!(["yath", "prove"]) } else { json!(["prove"]) };
    json!({
        "test_id": test_id,
        "file_id": file_id,
        "framework": framework,
        "name": name,
        "range": range,
        "runner_hints": runner_hints,
        "confidence": "high",
        "provenance_refs": [provenance_id],
    })
}

/// Build a `test_discovery` provenance fact (framework/import evidence).
fn test_discovery_provenance(
    provenance_id: &str,
    file_id: &str,
    range: Option<String>,
    confidence: &str,
) -> Value {
    json!({
        "provenance_id": provenance_id,
        "source": "test_discovery",
        "file_id": file_id,
        "range": range,
        "confidence": confidence,
    })
}

/// The oracle `expression`: the call's source text, trimmed and length-bounded.
fn call_expression(content: &str, start: usize, end: usize) -> String {
    let slice = content.get(start..end).unwrap_or("").trim();
    const MAX_CHARS: usize = 120;
    if slice.chars().count() > MAX_CHARS {
        let head: String = slice.chars().take(MAX_CHARS).collect();
        format!("{head}…")
    } else {
        slice.to_owned()
    }
}

/// A compact `sl:sc-el:ec` range string for the schema's string-typed
/// `provenance.range` field.
fn range_string(line_index: &LineIndex, start: usize, end: usize) -> String {
    let ((sl, sc), (el, ec)) = line_index.range(start, end);
    format!("{sl}:{sc}-{el}:{ec}")
}

/// Detect the test framework from parsed `use` statements. Returns the ripr
/// wire name (`"unknown"` if none matched) and the byte span of the matched
/// `use` node (for provenance). When several frameworks are imported, the most
/// specific one wins (the earliest entry in [`TEST_FRAMEWORKS`]). Single-pass —
/// no intermediate allocation or module-name cloning.
fn framework_from_ast(ast: &Node) -> (&'static str, Option<(usize, usize)>) {
    // (framework index, wire name, span) of the best (lowest-index) match so far.
    let mut best: Option<(usize, &'static str, (usize, usize))> = None;
    find_framework_use(ast, &mut best);
    match best {
        Some((_, wire, span)) => (wire, Some(span)),
        None => ("unknown", None),
    }
}

/// Recursively update `best` with the most specific test-framework `use` node.
fn find_framework_use(node: &Node, best: &mut Option<(usize, &'static str, (usize, usize))>) {
    if let NodeKind::Use { module, .. } = &node.kind
        && let Some(index) = TEST_FRAMEWORKS.iter().position(|(m, _)| m == module)
        && best.is_none_or(|(best_index, _, _)| index < best_index)
    {
        let (_, wire) = TEST_FRAMEWORKS[index];
        *best = Some((index, wire, (node.location.start(), node.location.end())));
    }
    node.for_each_child(|child| find_framework_use(child, best));
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::{must, must_some};

    fn parse_src(src: &str) -> Node {
        let mut parser = Parser::new(src);
        parser.parse().expect("test source parses")
    }

    #[test]
    fn framework_from_ast_detects_test_more() {
        assert_eq!(framework_from_ast(&parse_src("use Test::More;\nok(1);\n")).0, "Test::More");
    }

    #[test]
    fn framework_from_ast_detects_test2_bundles() {
        // Contract freeze: each Test2 bundle emits its own distinct wire name
        // (early builds collapsed Suite/V1 into "Test2::V0").
        for (src, want) in [
            ("use Test2::V0;\nok(1);", "Test2::V0"),
            ("use Test2::V1;\nok(1);", "Test2::V1"),
            ("use Test2::Suite;\nok(1);", "Test2::Suite"),
        ] {
            assert_eq!(framework_from_ast(&parse_src(src)).0, want, "src: {src}");
        }
    }

    #[test]
    fn framework_from_ast_detects_exception_and_fatal() {
        assert_eq!(
            framework_from_ast(&parse_src("use Test::Exception;\nthrows_ok { die } qr/x/;")).0,
            "Test::Exception"
        );
        assert_eq!(framework_from_ast(&parse_src("use Test::Fatal;\nok(1);")).0, "Test::Fatal");
    }

    #[test]
    fn framework_from_ast_returns_unknown_for_non_test() {
        assert_eq!(framework_from_ast(&parse_src("use strict;\nprint 1;\n")).0, "unknown");
    }

    #[test]
    fn emit_tests_and_oracles_for_test_more_file() {
        let root = std::env::temp_dir().join("perl-p4-test-more-root");
        let t_dir = root.join("t");
        std::fs::create_dir_all(&t_dir).expect("mkdir");
        std::fs::write(
            t_dir.join("app.t"),
            "use Test::More;\nis(1, 1, 'one');\nok(1, 'truthy');\n",
        )
        .expect("write");

        let (tests, oracles, provenance, _limitations) =
            emit_tests_and_oracles(root.to_str().expect("utf8 root"));

        assert_eq!(tests.len(), 1, "one test fact for the .t file");
        assert_eq!(tests[0]["framework"], "Test::More");
        assert!(oracles.iter().any(|o| o["kind"] == "exact_return_assertion"), "is → exact");
        assert!(oracles.iter().any(|o| o["kind"] == "smoke_ok"), "ok → smoke");
        // Parser-backed: no placeholder 1:1 ranges.
        let placeholder =
            json!({"start_line": 1, "start_column": 1, "end_line": 1, "end_column": 1});
        assert!(oracles.iter().all(|o| o["range"] != placeholder), "no placeholder oracle ranges");
        assert!(provenance.iter().any(|p| p["source"] == "test_discovery"));
        assert!(provenance.iter().any(|p| p["source"] == "oracle_extraction"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn emit_returns_empty_when_no_t_files() {
        let root = std::env::temp_dir().join("perl-p4-empty-root");
        std::fs::create_dir_all(&root).expect("mkdir");
        let (tests, oracles, provenance, limitations) =
            emit_tests_and_oracles(root.to_str().expect("utf8 root"));
        assert!(tests.is_empty() && oracles.is_empty(), "no .t files → no tests/oracles");
        assert!(
            provenance.is_empty() && limitations.is_empty(),
            "no .t files → no prov/limitations"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn oracle_facts_carry_observed_sink_and_expected_expression_fields() {
        // Contract-freeze parity (Campaign 31 step 2): the schema declares
        // observed_sink + expected_expression on oracle (nullable). The emitter
        // must write both keys — null when not yet derived — so ripr's
        // deserializer sees them. This test runs the test/oracle emitter and
        // asserts every oracle carries both keys.
        let temp = std::env::temp_dir().join("ripr_facts_oracle_contract_test");
        let t_dir = temp.join("t");
        must(std::fs::create_dir_all(&t_dir));
        must(std::fs::write(
            t_dir.join("app.t"),
            "use Test::More;\nis(disco(100), 50, 'half');\nok(1);\n",
        ));
        let (_tests, oracles, _provenance, _limitations) =
            emit_tests_and_oracles(must_some(temp.to_str()));
        let _ = std::fs::remove_dir_all(&temp);
        assert!(!oracles.is_empty(), "expected at least one oracle fact");
        for oracle in &oracles {
            assert!(
                oracle.get("observed_sink").is_some(),
                "oracle must carry observed_sink (schema contract): {oracle}"
            );
            assert!(
                oracle.get("expected_expression").is_some(),
                "oracle must carry expected_expression (schema contract): {oracle}"
            );
        }
    }
}
