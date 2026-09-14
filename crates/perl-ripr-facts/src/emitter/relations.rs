//! Relation inference: `relations[]` (`direct_owner_call` / `file_proximity`)
//! between `.t` test files and `.pm` source files.
//!
//! Note: `emit_relations_and_discriminators` itself carries no doc comment —
//! see the comment on `owner_fact_id` in [`super::ids`] for why (a pre-existing
//! doc-comment defect kept verbatim across the #9271 split, not fixed here).

use perl_parser_core::{Node, NodeKind, Parser};
use perl_symbol::SymbolKind;
use perl_symbol::surface::{SymbolRefKind, extract_symbol_decls, extract_symbol_refs};
use serde_json::{Value, json};

use super::discovery::{collect_pm_files, collect_t_files};
use super::ids::owner_fact_id;
use super::owners::owner_kind;

/// Rebuild the exact `owners[]` `owner_id` that [`emit_files_and_owners`] assigns
/// to the container declaration (`package`/`class`/`role`) named `package_name`
/// in `pm_path`. Returns `None` when the parse exposes no such container decl
/// (a name-mismatched or unparsed package) — the caller then omits the relation
/// with a limitation rather than emitting a dangling `owner_id`. Uses the same
/// `extract_symbol_decls` + `owner_kind` + `full_span` path as the owner emitter,
/// via the shared [`owner_fact_id`], so the reconstructed id is byte-identical.
fn resolve_package_owner_id(ast: &Node, pm_path: &str, package_name: &str) -> Option<String> {
    extract_symbol_decls(ast, Some("main")).into_iter().find_map(|decl| {
        if !matches!(decl.kind, SymbolKind::Package | SymbolKind::Class | SymbolKind::Role) {
            return None;
        }
        if decl.qualified_name != package_name {
            return None;
        }
        let kind = owner_kind(&decl.kind)?;
        let (start_byte, end_byte) = decl.full_span;
        Some(owner_fact_id(pm_path, kind, &decl.qualified_name, start_byte, end_byte))
    })
}

/// Emit relations, concrete discriminators, and observed-sink facts.
///
/// Campaign 31 Phase B PR 7 (perl-lsp-swarm#2594). The load-bearing semantic
/// slice. This conservative first-pass emitter:
///
/// - **Relations**: infers `file_proximity` relations between `.pm` source
///   files and `.t` test files that share a package name. Relation kind is
///   `file_proximity` (advisory-only per ripr's gating rules) with confidence
///   `medium`. This is the simplest relation — `direct_owner_call` /
///   `established-helper-call-chain` require AST-level call-graph analysis
///   that lands in a later enrichment.
///
/// - **Concrete discriminators**: derives from `is(...)` assertions — the
///   first argument is the observed value, the second is the expected value,
///   producing a concrete `"$got == $expected"` discriminator string. This
///   replaces the generic enum labels ripr's consumer currently falls back to.
///
/// - **Observed-sink facts**: ties each oracle to the specific value it
///   observes (the first argument of `is(got, expected, name)`). A strong
///   assertion elsewhere in the same test is NOT enough — the oracle must
///   observe the changed sink.
///
/// All three are conservative: where extraction is unsure, emit `unknown` /
/// omit the fact. ripr's strict-actionability validator fails closed on
/// unknown facts.
pub(crate) fn emit_relations_and_discriminators(
    root: &str,
    tests: &[Value],
    _oracles: &[Value],
) -> (Vec<Value>, Vec<Value>) {
    let mut relations = Vec::new();
    let mut limitations = Vec::new();

    // Collect .pm files from lib/, sorted for deterministic traversal order
    // (`std::fs::read_dir` order is filesystem/OS-dependent).
    let mut pm_files = collect_pm_files(std::path::Path::new(root));
    pm_files.sort_by(|a, b| a.0.cmp(&b.0));

    // Parse each candidate `.pm` once, hoisted above the test loop below —
    // NOT once per (test, pm) pair. `extract_package_name` stays the existing
    // string scan (out of scope); only the declared-sub proof is parser-backed.
    // The same parse also resolves the package's real `owners[]` `owner_id`
    // (#3342) so a relation can carry a resolvable cross-reference; `None` means
    // the parse exposed no matching container decl (unparsed/name-mismatched).
    type PmFact = (String, String, std::collections::BTreeMap<String, String>, Option<String>);
    let pm_facts: Vec<PmFact> = pm_files
        .iter()
        .map(|(pm_path, pm_content)| {
            let package_name = extract_package_name(pm_content);
            let (declared_sub_owner_ids, owner_id) = if package_name.is_empty() {
                (std::collections::BTreeMap::new(), None)
            } else {
                match Parser::new(pm_content).parse() {
                    Ok(ast) => (
                        declared_sub_owner_ids(&ast, pm_path, &package_name),
                        resolve_package_owner_id(&ast, pm_path, &package_name),
                    ),
                    // parse failure → cannot prove calls or resolve an owner
                    Err(_) => (std::collections::BTreeMap::new(), None),
                }
            };
            (pm_path.clone(), package_name, declared_sub_owner_ids, owner_id)
        })
        .collect();

    // Collect + parse every `.t` file once, hoisted and reused by both the
    // relation loop below and the discriminator loop further down (which
    // previously called `collect_t_files` a second time).
    let mut t_files = collect_t_files(std::path::Path::new(root));
    t_files.sort_by(|a, b| a.1.cmp(&b.1));

    let t_call_facts: std::collections::HashMap<&str, TestCallFacts> = t_files
        .iter()
        .map(|(_, relative_path, content)| {
            let facts = match Parser::new(content).parse() {
                Ok(ast) => TestCallFacts::from_ast(&ast, content),
                Err(_) => TestCallFacts::unparsed(),
            };
            (relative_path.as_str(), facts)
        })
        .collect();

    // For each test file, infer relations to .pm files by package-name match.
    for test in tests {
        let test_file_id = test["file_id"].as_str().unwrap_or("");
        let test_path = test["name"].as_str().unwrap_or("");

        for (pm_path, package_name, declared_sub_owner_ids, package_owner_id) in &pm_facts {
            if package_name.is_empty() {
                continue;
            }

            // Check if the test file references the package.
            if !test_file_id.is_empty()
                && test_references_package(
                    test_path,
                    t_call_facts.get(test_path),
                    pm_path,
                    package_name,
                )
            {
                // #3342: a relation's `owner_id` must resolve to a real
                // `owners[]` fact. Prefer the called sub/method owner only for
                // qualified call evidence; fall back to the package owner for
                // coarse file-proximity evidence.
                let relation_owners = relation_owners_for_test(
                    t_call_facts.get(test_path),
                    package_name,
                    declared_sub_owner_ids,
                    package_owner_id.as_ref(),
                );
                if relation_owners.is_empty() {
                    limitations.push(json!({
                        "limitation_id": format!("relation-owner-unresolved:{pm_path}"),
                        "kind": "unresolved_owner",
                        "message": format!(
                            "test references package `{package_name}` in `{pm_path}` but no matching owners[] fact was found (unparsed or name-mismatched package); the relation is omitted to avoid a dangling owner_id"
                        ),
                        "evidence_refs": []
                    }));
                    continue;
                }

                for (resolved_owner_id, is_direct) in relation_owners {
                    let relation_kind =
                        if is_direct { "direct_owner_call" } else { "file_proximity" };
                    let reachability = if is_direct { "reachable" } else { "weakly_reachable" };
                    let owner_suffix = relation_owner_suffix(&resolved_owner_id);

                    let relation_id = format!("relation:{test_file_id}:{pm_path}:{owner_suffix}");
                    relations.push(json!({
                        "relation_id": relation_id,
                        "change_id": "change:unresolved",
                        "owner_id": resolved_owner_id,
                        "test_id": test["test_id"],
                        "oracle_id": null,
                        "relation_kind": relation_kind,
                        "reachability_hint": reachability,
                        "confidence": "medium",
                        "provenance_refs": []
                    }));
                }
            }
        }
    }

    // Sort relations by id for deterministic output order regardless of the
    // caller's `tests` ordering.
    relations.sort_by(|a, b| a["relation_id"].as_str().cmp(&b["relation_id"].as_str()));

    // Deterministic order + dedup: the same unresolvable package referenced by
    // several test files would otherwise push a duplicate limitation per test.
    limitations.sort_by(|a, b| a["limitation_id"].as_str().cmp(&b["limitation_id"].as_str()));
    limitations.dedup_by(|a, b| a["limitation_id"] == b["limitation_id"]);

    (relations, limitations)
}

/// Extract the package name from Perl source (first `package Foo::Bar;` line).
fn extract_package_name(content: &str) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("package ")
            && let Some(name_end) = rest.find(';')
        {
            return rest[..name_end].trim().to_string();
        }
    }
    String::new()
}

fn test_references_package(
    test_path: &str,
    facts: Option<&TestCallFacts>,
    pm_path: &str,
    package_name: &str,
) -> bool {
    facts.is_some_and(|facts| {
        facts.used_modules.contains(package_name)
            || facts
                .calls
                .iter()
                .any(|call| call.package_qualifier.as_deref() == Some(package_name))
    }) || file_references_package(test_path, &[], pm_path)
}

/// Check if a test file references a .pm file (fallback heuristic: same basename).
fn file_references_package(test_path: &str, _all_pm_paths: &[&str], pm_path: &str) -> bool {
    // Simple heuristic: if the .pm basename appears in the test path.
    // E.g. t/app.t references lib/My/App.pm if "App" appears in both.
    let pm_basename = pm_path.rsplit(['/', '\\']).next().unwrap_or("").trim_end_matches(".pm");
    !pm_basename.is_empty()
        && test_path.to_ascii_lowercase().contains(&pm_basename.to_ascii_lowercase())
}

/// Per-`.t`-file facts computed once (parse + call/`use` extraction) and
/// reused across every candidate `.pm` in [`emit_relations_and_discriminators`]
/// — avoids re-parsing/re-walking `t/` once per (test, pm) pair.
struct TestCallFacts {
    /// Subroutine/method/static-method call sites from the test file.
    calls: Vec<perl_symbol::surface::SymbolRef>,
    /// Modules imported by parsed `use` statements in the test file.
    used_modules: std::collections::HashSet<String>,
    /// First two arguments from parser-backed `is(...)` assertions.
    ///
    /// Currently read only by this module's tests: #5064 moved the production
    /// consumer behind `cfg(test)` and left this field written-but-never-read
    /// in normal builds, which trips the workspace `dead_code` deny. Kept
    /// populated (rather than deleted) because the `is(...)` argument pairs are
    /// the input the discriminator work needs; gating it on `cfg(test)` instead
    /// would cascade `content` and `source_span` into dead code too.
    #[allow(dead_code, reason = "test-only reader since #5064; retained for discriminator work")]
    is_args: Vec<(String, String)>,
}

impl TestCallFacts {
    /// Conservative fallback for a test file that failed to parse: proves
    /// nothing, so [`test_calls_declared_sub`] always degrades for it.
    fn unparsed() -> Self {
        Self {
            calls: Vec::new(),
            used_modules: std::collections::HashSet::new(),
            is_args: Vec::new(),
        }
    }

    fn from_ast(ast: &Node, content: &str) -> Self {
        let mut used_modules = std::collections::HashSet::new();
        collect_used_modules(ast, &mut used_modules);
        let calls = extract_symbol_refs(ast)
            .into_iter()
            .filter(|reference| {
                matches!(
                    reference.kind,
                    SymbolRefKind::SubroutineCall
                        | SymbolRefKind::MethodCall
                        | SymbolRefKind::StaticMethodCall
                )
            })
            .collect();
        let mut is_args = Vec::new();
        collect_is_args(ast, content, &mut is_args);
        Self { calls, used_modules, is_args }
    }
}

fn collect_used_modules(node: &Node, used_modules: &mut std::collections::HashSet<String>) {
    if let NodeKind::Use { module, .. } = &node.kind {
        used_modules.insert(module.clone());
    }
    node.for_each_child(|child| collect_used_modules(child, used_modules));
}

/// Bare sub/method names a `.pm`'s AST declares *inside* `package_name`.
///
/// Filters [`extract_symbol_decls`] to callables whose `container` matches the
/// package being evaluated for this relation, so a multi-package file only
/// credits the package actually being checked.
#[cfg(test)]
fn declared_sub_names(pm_ast: &Node, package_name: &str) -> std::collections::HashSet<String> {
    extract_symbol_decls(pm_ast, Some("main"))
        .into_iter()
        .filter(|decl| matches!(decl.kind, SymbolKind::Subroutine | SymbolKind::Method))
        .filter(|decl| decl.container.as_deref() == Some(package_name))
        .map(|decl| decl.name)
        .collect()
}

fn declared_sub_owner_ids(
    pm_ast: &Node,
    pm_path: &str,
    package_name: &str,
) -> std::collections::BTreeMap<String, String> {
    extract_symbol_decls(pm_ast, Some("main"))
        .into_iter()
        .filter(|decl| matches!(decl.kind, SymbolKind::Subroutine | SymbolKind::Method))
        .filter(|decl| decl.container.as_deref() == Some(package_name))
        .filter_map(|decl| {
            let kind = owner_kind(&decl.kind)?;
            let (start_byte, end_byte) = decl.full_span;
            Some((
                decl.name,
                owner_fact_id(pm_path, kind, &decl.qualified_name, start_byte, end_byte),
            ))
        })
        .collect()
}

fn relation_owners_for_test(
    facts: Option<&TestCallFacts>,
    package_name: &str,
    declared_sub_owner_ids: &std::collections::BTreeMap<String, String>,
    package_owner_id: Option<&String>,
) -> Vec<(String, bool)> {
    let Some(facts) = facts else {
        return package_owner_id.iter().map(|owner_id| ((*owner_id).clone(), false)).collect();
    };

    let mut direct_owner_ids = Vec::new();
    for call in &facts.calls {
        if call.kind != SymbolRefKind::SubroutineCall
            || call.package_qualifier.as_deref() != Some(package_name)
        {
            continue;
        }
        if let Some(owner_id) = declared_sub_owner_ids.get(&call.name)
            && !direct_owner_ids.iter().any(|existing| existing == owner_id)
        {
            direct_owner_ids.push(owner_id.clone());
        }
    }
    if !direct_owner_ids.is_empty() {
        return direct_owner_ids.into_iter().map(|owner_id| (owner_id, true)).collect();
    }

    // Bare calls after `use Package` are export-unproven. They can keep a
    // package-level file_proximity relation, but must not point at the sub
    // owner or downstream consumers can mistake weak evidence for sub-specific
    // reachability.
    package_owner_id.iter().map(|owner_id| ((*owner_id).clone(), false)).collect()
}

fn relation_owner_suffix(owner_id: &str) -> String {
    owner_id.replace('\\', "/").replace(':', "_")
}

/// Parser-backed replacement for the former string-heuristic
/// `test_calls_package` (#3293 PR 6). Proves a `direct_owner_call` relation
/// using AST call nodes (`perl-symbol`) instead of substring scans over raw
/// file content.
///
/// A call counts as direct **only** when it is a fully-qualified
/// `SymbolRefKind::SubroutineCall` — `package_name::sub(...)` with
/// `package_qualifier == Some(package_name)` and `sub` in `declared_subs`. A
/// fully-qualified call names its owning package unambiguously and reaches the
/// declared sub regardless of exports, so this is the one form the AST can
/// *prove*.
///
/// Deliberately conservative — never a false `true`:
///  - A **bare** call (`sub(...)`) after `use package_name` is NOT proof: the
///    sub is only callable bareword if the package exports it
///    (`Exporter`/`@EXPORT`), which `perl-symbol` cannot see, and a shared
///    basename could make the same bare name belong to any of several `use`d
///    packages. Bare calls fall through to the conservative
///    `file_proximity`/`weakly_reachable` branch rather than a false
///    `reachable`. (Export-aware bare-call attribution is a later slice.)
///  - `MethodCall` / `StaticMethodCall` (`$obj->m` / `Pkg->m`) are excluded —
///    dynamic dispatch can resolve to an inherited, `AUTOLOAD`'d, or
///    role-composed method and is never provable from the call site.
///  - An empty `declared_subs` (the `.pm` failed to parse or declares nothing
///    in `package_name`) or an unparsed test file proves nothing.
///
/// This is a lexical call-graph proof, not a control-flow-verified one: a call
/// inside a dead branch or a `SKIP:` block still counts, matching this crate's
/// convention that every other "reachable"-adjacent claim here is also lexical.
#[cfg(test)]
fn test_calls_declared_sub(
    facts: &TestCallFacts,
    package_name: &str,
    declared_subs: &std::collections::HashSet<String>,
) -> bool {
    if declared_subs.is_empty() {
        return false;
    }
    facts.calls.iter().any(|call| {
        call.kind == SymbolRefKind::SubroutineCall
            && call.package_qualifier.as_deref() == Some(package_name)
            && declared_subs.contains(&call.name)
    })
}

fn source_span(content: &str, start: usize, end: usize) -> Option<&str> {
    content.get(start..end)
}

fn collect_is_args(node: &Node, content: &str, output: &mut Vec<(String, String)>) {
    if let NodeKind::FunctionCall { name, args } = &node.kind
        && name == "is"
        && args.len() >= 2
    {
        let got = source_span(content, args[0].location.start, args[0].location.end);
        let expected = source_span(content, args[1].location.start, args[1].location.end);
        if let (Some(got), Some(expected)) = (got, expected) {
            output.push((got.trim().to_string(), expected.trim().to_string()));
        }
    }
    node.for_each_child(|child| collect_is_args(child, content, output));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emitter::owners::emit_files_and_owners;
    use crate::emitter::test_facts::emit_tests_and_oracles;
    use perl_tdd_support::{must, must_some};

    fn parse_src(src: &str) -> Node {
        let mut parser = Parser::new(src);
        parser.parse().expect("test source parses")
    }

    #[test]
    fn file_references_package_accepts_windows_path_separators() {
        assert!(
            file_references_package("t/App.t", &[], "lib\\My\\App.pm"),
            "fallback basename matching must handle Windows-style .pm paths"
        );
    }

    #[test]
    fn emit_discriminators_from_indented_is_inside_subtest() {
        let root = std::env::temp_dir().join("perl-B7-indented-subtest-root");
        let t_dir = root.join("t");
        must(std::fs::create_dir_all(&t_dir));
        must(std::fs::write(
            t_dir.join("app.t"),
            "use Test::More;\nsubtest 'nested' => sub {\n    my $x = 1;\n    is($x, 1); # trailing comment\n};\n",
        ));

        let (tests, _oracles, _provenance, _limitations) =
            emit_tests_and_oracles(must_some(root.to_str()));
        let (_relations, _relation_limitations) =
            emit_relations_and_discriminators(must_some(root.to_str()), &tests, &[]);

        // Observable/sink emission was removed (#5064) — relations and
        // limitations are the only consumers. The is() discriminator
        // extraction still runs as part of relation building.

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extract_package_name_from_pm_content() {
        let content = "package My::App;\nuse strict;\n1;";
        assert_eq!(extract_package_name(content), "My::App");
    }

    #[test]
    fn extract_package_name_returns_empty_when_no_package() {
        assert_eq!(extract_package_name("use strict;\n1;"), "");
    }

    /// Build `TestCallFacts` directly from source, mirroring what
    /// `emit_relations_and_discriminators` does per `.t` file.
    fn facts_from_src(src: &str) -> TestCallFacts {
        TestCallFacts::from_ast(&parse_src(src), src)
    }

    fn one_sub(name: &str) -> std::collections::HashSet<String> {
        std::collections::HashSet::from([name.to_string()])
    }

    #[test]
    fn test_call_facts_collects_simple_is_call() {
        let calls = facts_from_src("is(discount(100), 50, 'half price');").is_args;
        assert_eq!(calls.len(), 1, "must parse exactly one is call");
        let [(got, expected)] = calls.as_slice() else {
            return;
        };
        assert_eq!(got, "discount(100)");
        assert_eq!(expected, "50");
    }

    #[test]
    fn test_call_facts_ignores_non_is_calls() {
        assert!(facts_from_src("ok(1, 'truthy');").is_args.is_empty());
    }

    #[test]
    fn test_call_facts_collects_indented_multiline_is_call() {
        let calls = facts_from_src(
            "subtest 'nested' => sub {\n    is(\n        $x,\n        1,\n    ); # trailing comment\n};\n",
        )
        .is_args;
        assert_eq!(calls, vec![("$x".to_string(), "1".to_string())]);
    }

    #[test]
    fn emit_relations_finds_pm_test_proximity() {
        let root = std::env::temp_dir().join("perl-B7-relations-root");
        let lib_dir = root.join("lib/My");
        let t_dir = root.join("t");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::create_dir_all(&t_dir));
        must(std::fs::write(lib_dir.join("App.pm"), "package My::App;\nsub discount { }\n1;"));
        must(std::fs::write(t_dir.join("App.t"), "use Test::More;\nok(1);\n"));

        // First emit tests, then relations.
        let (tests, _oracles, _provenance, _limitations) =
            emit_tests_and_oracles(must_some(root.to_str()));
        let (relations, _relation_limitations) =
            emit_relations_and_discriminators(must_some(root.to_str()), &tests, &[]);

        assert!(!relations.is_empty(), "must find at least one relation between App.pm and App.t");
        assert_eq!(
            relations[0]["relation_kind"], "file_proximity",
            "relation must be file_proximity (advisory-only)"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn emit_relations_uses_imported_package_when_test_filename_case_differs() {
        let root = std::env::temp_dir().join("perl-B7-import-relations-root");
        let lib_dir = root.join("lib");
        let t_dir = root.join("t");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::create_dir_all(&t_dir));
        must(std::fs::write(
            lib_dir.join("Pricing.pm"),
            "package Pricing;\nsub calculate_discount { }\n1;",
        ));
        must(std::fs::write(
            t_dir.join("pricing.t"),
            "use Test::More;\nuse Pricing;\nok(calculate_discount(100));\n",
        ));

        let (tests, _oracles, _provenance, _limitations) =
            emit_tests_and_oracles(must_some(root.to_str()));
        let (relations, _relation_limitations) =
            emit_relations_and_discriminators(must_some(root.to_str()), &tests, &[]);

        assert!(
            relations.iter().any(|relation| relation["owner_id"]
                .as_str()
                .is_some_and(|owner_id| owner_id.contains("Pricing"))),
            "use Pricing should relate t/pricing.t to lib/Pricing.pm despite filename casing"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn emit_discriminators_from_is_assertions() {
        let root = std::env::temp_dir().join("perl-B7-discriminators-root");
        let t_dir = root.join("t");
        must(std::fs::create_dir_all(&t_dir));
        must(std::fs::write(
            t_dir.join("app.t"),
            "use Test::More;\nis(discount(100), 50, 'half');\n",
        ));

        let (tests, _oracles, _provenance, _limitations) =
            emit_tests_and_oracles(must_some(root.to_str()));
        let (_relations, _relation_limitations) =
            emit_relations_and_discriminators(must_some(root.to_str()), &tests, &[]);

        // Observable/sink emission was removed (#5064).

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn emit_relations_upgrades_to_direct_owner_call() {
        let root = std::env::temp_dir().join("perl-P3-direct-call-root");
        let lib_dir = root.join("lib/My");
        let t_dir = root.join("t");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::create_dir_all(&t_dir));
        must(std::fs::write(lib_dir.join("App.pm"), "package My::App;\nsub discount { }\n1;\n"));
        // Test file uses the package directly.
        must(std::fs::write(
            t_dir.join("App.t"),
            "use My::App;\nis(My::App::discount(100), 50);\n",
        ));

        let (tests, _oracles, _provenance, _limitations) =
            emit_tests_and_oracles(must_some(root.to_str()));
        let (relations, _relation_limitations) =
            emit_relations_and_discriminators(must_some(root.to_str()), &tests, &[]);

        assert!(!relations.is_empty(), "must find at least one relation");
        assert!(
            relations.iter().any(|r| r["relation_kind"] == "direct_owner_call"),
            "must emit at least one direct_owner_call relation"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn emit_relations_emits_each_direct_owner_call() {
        let root = std::env::temp_dir().join("perl-P6-multiple-direct-call-root");
        let _ = std::fs::remove_dir_all(&root);
        let lib_dir = root.join("lib/My");
        let t_dir = root.join("t");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::create_dir_all(&t_dir));
        must(std::fs::write(
            lib_dir.join("App.pm"),
            "package My::App;\nsub setup { }\nsub target { }\n1;\n",
        ));
        must(std::fs::write(
            t_dir.join("App.t"),
            "use My::App;\nMy::App::setup();\nMy::App::target();\n",
        ));

        let (tests, _oracles, _provenance, _limitations) =
            emit_tests_and_oracles(must_some(root.to_str()));
        let (relations, _relation_limitations) =
            emit_relations_and_discriminators(must_some(root.to_str()), &tests, &[]);

        let direct: Vec<_> = relations
            .iter()
            .filter(|relation| relation["relation_kind"] == "direct_owner_call")
            .collect();
        assert_eq!(direct.len(), 2, "each qualified called owner gets a relation: {relations:?}");
        assert!(
            direct.iter().any(|relation| {
                relation["owner_id"]
                    .as_str()
                    .is_some_and(|owner_id| owner_id.contains(":sub:My::App::setup:"))
            }),
            "setup owner relation missing: {direct:?}"
        );
        assert!(
            direct.iter().any(|relation| {
                relation["owner_id"]
                    .as_str()
                    .is_some_and(|owner_id| owner_id.contains(":sub:My::App::target:"))
            }),
            "target owner relation missing: {direct:?}"
        );
        let relation_ids: std::collections::HashSet<_> =
            direct.iter().filter_map(|relation| relation["relation_id"].as_str()).collect();
        assert_eq!(relation_ids.len(), direct.len(), "direct relation IDs must be unique");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn declared_sub_matches_qualified_call_without_use() {
        let facts = facts_from_src("My::App::discount(100);\n");
        assert!(
            test_calls_declared_sub(&facts, "My::App", &one_sub("discount")),
            "qualified call must prove direct_owner_call even without a `use`"
        );
    }

    #[test]
    fn declared_sub_rejects_bare_call_after_use_export_unproven() {
        // A bare call after `use My::App` is NOT proof of direct_owner_call: the
        // sub is only callable bareword if My::App exports it (Exporter/@EXPORT),
        // which perl-symbol can't see. Marking it `reachable` would be a false
        // positive (the call fails at runtime if the sub isn't exported), so the
        // bare-call case degrades to the conservative file_proximity branch.
        let facts = facts_from_src("use My::App;\ndiscount(100);\n");
        assert!(
            !test_calls_declared_sub(&facts, "My::App", &one_sub("discount")),
            "a bare call after `use` is export-unproven — must not claim direct_owner_call"
        );
    }

    #[test]
    fn declared_sub_rejects_use_without_matching_call() {
        let facts = facts_from_src("use My::App;\nok(1);\n");
        assert!(
            !test_calls_declared_sub(&facts, "My::App", &one_sub("discount")),
            "`use` alone, with no call to a declared sub, must not prove direct_owner_call"
        );
    }

    #[test]
    fn declared_sub_rejects_call_name_in_comment() {
        // The key false positive `test_calls_package`'s substring scan had:
        // a comment mentioning the qualified name is not a call node.
        let facts = facts_from_src("# My::App::discount(100);\nok(1);\n");
        assert!(
            !test_calls_declared_sub(&facts, "My::App", &one_sub("discount")),
            "a call name inside a comment must never prove direct_owner_call"
        );
    }

    #[test]
    fn declared_sub_rejects_call_name_in_string_literal() {
        let facts = facts_from_src("my $x = \"My::App::discount(100)\";\nok(1);\n");
        assert!(
            !test_calls_declared_sub(&facts, "My::App", &one_sub("discount")),
            "a call name inside a string literal must never prove direct_owner_call"
        );
    }

    #[test]
    fn declared_sub_rejects_static_method_call() {
        let facts = facts_from_src("My::App->discount(100);\n");
        assert!(
            !test_calls_declared_sub(&facts, "My::App", &one_sub("discount")),
            "a static method call must degrade — dispatch can resolve to an \
             inherited/AUTOLOAD'd method the AST can't distinguish from a direct hit"
        );
    }

    #[test]
    fn declared_sub_rejects_instance_method_call() {
        let facts = facts_from_src("my $o = My::App->new;\n$o->discount(100);\n");
        assert!(
            !test_calls_declared_sub(&facts, "My::App", &one_sub("discount")),
            "an instance method call on a dynamic receiver must degrade"
        );
    }

    #[test]
    fn declared_sub_rejects_qualified_call_to_different_package() {
        let facts = facts_from_src("Other::Pkg::discount(100);\n");
        assert!(
            !test_calls_declared_sub(&facts, "My::App", &one_sub("discount")),
            "a call qualified to a different package must not prove this package's relation"
        );
    }

    #[test]
    fn declared_sub_rejects_bare_call_without_use() {
        let facts = facts_from_src("discount(100);\n");
        assert!(
            !test_calls_declared_sub(&facts, "My::App", &one_sub("discount")),
            "a bare call with no matching `use` must not prove direct_owner_call \
             (same bare name could belong to any package)"
        );
    }

    #[test]
    fn declared_sub_rejects_when_declared_subs_empty() {
        // Even a real, unambiguous qualified call must degrade when the
        // candidate package's `.pm` declared no subs (e.g. it failed to
        // parse, or only declared anonymous subs) — never a false `true`.
        let facts = facts_from_src("My::App::discount(100);\n");
        assert!(
            !test_calls_declared_sub(&facts, "My::App", &std::collections::HashSet::new()),
            "empty declared_subs must always degrade, regardless of call shape"
        );
    }

    #[test]
    fn declared_sub_rejects_use_with_empty_import_list_and_no_call() {
        // `test_calls_package`'s `content.contains("(")` used to trip on the
        // parens of `use My::App ();` alone, with zero actual calls.
        let facts = facts_from_src("use My::App ();\nok(1);\n");
        assert!(
            !test_calls_declared_sub(&facts, "My::App", &one_sub("discount")),
            "an empty-import-list `use` with no call must not prove direct_owner_call"
        );
    }

    #[test]
    fn declared_sub_names_filters_by_container() {
        // A file declaring two packages must only credit the one being asked
        // about — `declared_sub_names` filters on `container`, not just name.
        let ast = parse_src(
            "package My::App;\nsub discount { }\npackage Other::Pkg;\nsub discount { }\n",
        );
        let app_subs = declared_sub_names(&ast, "My::App");
        assert!(app_subs.contains("discount"));
        let other_subs = declared_sub_names(&ast, "Other::Pkg");
        assert!(other_subs.contains("discount"));
        // Cross-check: a package name that appears nowhere yields no subs.
        assert!(declared_sub_names(&ast, "Unrelated::Pkg").is_empty());
    }

    #[test]
    fn emit_relations_bare_call_after_use_stays_file_proximity() {
        // End-to-end: a bare call after `use` is export-unproven, so the relation
        // stays file_proximity/weakly_reachable — NOT a false direct_owner_call.
        let root = std::env::temp_dir().join("perl-P6-bare-call-root");
        let lib_dir = root.join("lib/My");
        let t_dir = root.join("t");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::create_dir_all(&t_dir));
        must(std::fs::write(lib_dir.join("App.pm"), "package My::App;\nsub discount { }\n1;\n"));
        must(std::fs::write(t_dir.join("App.t"), "use My::App;\ndiscount(100);\nok(1);\n"));

        let (tests, _oracles, _provenance, _limitations) =
            emit_tests_and_oracles(must_some(root.to_str()));
        let (relations, _relation_limitations) =
            emit_relations_and_discriminators(must_some(root.to_str()), &tests, &[]);

        assert!(!relations.is_empty(), "the file_proximity relation is still emitted");
        assert!(
            relations.iter().all(|r| r["relation_kind"] != "direct_owner_call"),
            "a bare (export-unproven) call must NOT be direct_owner_call, got: {relations:?}"
        );
        let relation_owner = relations[0]["owner_id"].as_str().expect("relation owner_id");
        assert!(
            relation_owner.contains(":package:My::App:"),
            "a bare export-unproven call must stay package-scoped, got {relation_owner}"
        );
        assert!(
            !relation_owner.contains(":sub:"),
            "a bare export-unproven call must not bind weak evidence to a sub owner, got {relation_owner}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn emit_relations_shared_basename_bare_call_marks_neither_direct() {
        // Two packages share the basename `Widget` and both declare `run`; the
        // test `use`s both and calls bare `run()`. At most one is the real owner,
        // so neither may be claimed direct_owner_call (the string version would
        // have marked both). Qualified calls would disambiguate; bare ones can't.
        let root = std::env::temp_dir().join("perl-P6-shared-basename-root");
        let t_dir = root.join("t");
        must(std::fs::create_dir_all(root.join("lib/V1")));
        must(std::fs::create_dir_all(root.join("lib/V2")));
        must(std::fs::create_dir_all(&t_dir));
        must(std::fs::write(
            root.join("lib/V1/Widget.pm"),
            "package V1::Widget;\nsub run { }\n1;\n",
        ));
        must(std::fs::write(
            root.join("lib/V2/Widget.pm"),
            "package V2::Widget;\nsub run { }\n1;\n",
        ));
        must(std::fs::write(
            t_dir.join("Widget.t"),
            "use V1::Widget;\nuse V2::Widget;\nrun();\nok(1);\n",
        ));

        let (tests, _oracles, _provenance, _limitations) =
            emit_tests_and_oracles(must_some(root.to_str()));
        let (relations, _relation_limitations) =
            emit_relations_and_discriminators(must_some(root.to_str()), &tests, &[]);

        assert!(
            relations.iter().all(|r| r["relation_kind"] != "direct_owner_call"),
            "an ambiguous shared-basename bare call must mark neither package direct, got: {relations:?}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn emit_relations_comment_mention_stays_file_proximity() {
        // The headline false positive the string heuristic had, proven fixed
        // through the full emitter, not just the predicate.
        let root = std::env::temp_dir().join("perl-P6-comment-mention-root");
        let lib_dir = root.join("lib/My");
        let t_dir = root.join("t");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::create_dir_all(&t_dir));
        must(std::fs::write(lib_dir.join("App.pm"), "package My::App;\nsub discount { }\n1;\n"));
        must(std::fs::write(
            t_dir.join("App.t"),
            "# My::App::discount(100) is deprecated\nok(1);\n",
        ));

        let (tests, _oracles, _provenance, _limitations) =
            emit_tests_and_oracles(must_some(root.to_str()));
        let (relations, _relation_limitations) =
            emit_relations_and_discriminators(must_some(root.to_str()), &tests, &[]);

        assert!(!relations.is_empty(), "must still find a file_proximity relation");
        assert!(
            relations.iter().all(|r| r["relation_kind"] == "file_proximity"),
            "a comment-only mention must never upgrade to direct_owner_call, got: {relations:?}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn emit_relations_deterministic_across_runs_and_fs_order() {
        let root = std::env::temp_dir().join("perl-P6-determinism-root");
        let lib_dir = root.join("lib/My");
        let t_dir = root.join("t");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::create_dir_all(&t_dir));
        // Create files in reverse-alphabetical order on disk.
        must(std::fs::write(lib_dir.join("Zeta.pm"), "package My::Zeta;\nsub run { }\n1;\n"));
        must(std::fs::write(lib_dir.join("Alpha.pm"), "package My::Alpha;\nsub run { }\n1;\n"));
        must(std::fs::write(t_dir.join("Zeta.t"), "use My::Zeta;\nrun();\nok(1);\n"));
        must(std::fs::write(t_dir.join("Alpha.t"), "use My::Alpha;\nrun();\nok(1);\n"));

        let (tests, _oracles, _provenance, _limitations) =
            emit_tests_and_oracles(must_some(root.to_str()));
        let (relations_1, _limitations_1) =
            emit_relations_and_discriminators(must_some(root.to_str()), &tests, &[]);
        let (relations_2, _limitations_2) =
            emit_relations_and_discriminators(must_some(root.to_str()), &tests, &[]);

        assert_eq!(
            relations_1, relations_2,
            "two runs over the same root must produce byte-identical relations JSON"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn emit_relations_sorted_by_relation_id() {
        let root = std::env::temp_dir().join("perl-P6-sorted-root");
        let lib_dir = root.join("lib/My");
        let t_dir = root.join("t");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::create_dir_all(&t_dir));
        must(std::fs::write(lib_dir.join("Zeta.pm"), "package My::Zeta;\nsub run { }\n1;\n"));
        must(std::fs::write(lib_dir.join("Mid.pm"), "package My::Mid;\nsub run { }\n1;\n"));
        must(std::fs::write(lib_dir.join("Alpha.pm"), "package My::Alpha;\nsub run { }\n1;\n"));
        must(std::fs::write(t_dir.join("Zeta.t"), "use My::Zeta;\nrun();\n"));
        must(std::fs::write(t_dir.join("Mid.t"), "use My::Mid;\nrun();\n"));
        must(std::fs::write(t_dir.join("Alpha.t"), "use My::Alpha;\nrun();\n"));

        let (tests, _oracles, _provenance, _limitations) =
            emit_tests_and_oracles(must_some(root.to_str()));
        let (relations, _relation_limitations) =
            emit_relations_and_discriminators(must_some(root.to_str()), &tests, &[]);

        assert!(relations.len() >= 3, "expected at least 3 relations, got {}", relations.len());
        let ids: Vec<&str> =
            relations.iter().map(|r| must_some(r["relation_id"].as_str())).collect();
        let mut sorted_ids = ids.clone();
        sorted_ids.sort_unstable();
        assert_eq!(ids, sorted_ids, "relations must be sorted by relation_id");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn relation_owner_id_resolves_to_owner_fact() {
        // #3342: a relation's `owner_id` must be the real `owners[]` id (kind +
        // qualified name + byte span), not the old dangling `owner:{path}:{pkg}`
        // shape — so a consumer walking `relation.owner_id → owners[]` resolves.
        let root = std::env::temp_dir().join("perl-3342-owner-id-root");
        let lib_dir = root.join("lib/My");
        let t_dir = root.join("t");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::create_dir_all(&t_dir));
        must(std::fs::write(lib_dir.join("App.pm"), "package My::App;\nsub discount { }\n1;\n"));
        must(std::fs::write(t_dir.join("App.t"), "use My::App;\nMy::App::discount(100);\n"));

        let (tests, _oracles, _provenance, _limitations) =
            emit_tests_and_oracles(must_some(root.to_str()));
        let (relations, _relation_limitations) =
            emit_relations_and_discriminators(must_some(root.to_str()), &tests, &[]);
        let (_files, owners, _file_provenance, _file_limitations) =
            emit_files_and_owners(must_some(root.to_str()));

        assert!(!relations.is_empty());
        let owner_id = must_some(relations[0]["owner_id"].as_str());
        // New shape: `owner:{path}:sub:{qualified_name}:{span}` (not the bare
        // package name), with a byte span so it is not hard-pinned here.
        assert!(
            owner_id.starts_with("owner:lib/My/App.pm:sub:My::App::discount:"),
            "relation owner_id should use the resolvable owners[] shape, got {owner_id}"
        );
        // And it must actually resolve to an emitted `owners[]` fact.
        assert!(
            owners.iter().any(|o| o["owner_id"].as_str() == Some(owner_id)),
            "relation.owner_id {owner_id} must resolve to a present owners[] fact"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
