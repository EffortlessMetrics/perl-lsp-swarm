//! Distribution-metadata facts.
//!
//! Reads a Perl distribution's metadata files — `META.json` (CPAN Meta Spec
//! v2, via `serde_json`), `cpanfile` (a Perl DSL, via a dependency-light
//! statement scan) — into typed facts: name, version, abstract, licenses, and
//! prerequisites. The extraction mirrors the proven, std+serde_json approach in
//! `perl-lsp-rs-core::config::metadata_dependencies` (which the substrate may
//! not depend on, being above the leaf line), ported here so dist facts sit in
//! the substrate for Kwalitee and other consumers (PLSP-ADR-0006 PR 7).

use serde::{Deserialize, Serialize};

use crate::id::FileId;

/// Which metadata file a dist fact came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistMetadataSource {
    /// `META.json` (CPAN Meta Spec v2).
    MetaJson,
    /// A `cpanfile` (Perl DSL).
    Cpanfile,
}

/// One declared prerequisite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prereq {
    /// The required module.
    pub module: String,
    /// The version requirement, if any (`"0"` means "any").
    pub version: Option<String>,
    /// Phase: `configure` / `build` / `test` / `runtime` / `develop`.
    pub phase: String,
    /// Relation: `requires` / `recommends` / `suggests` / `conflicts`.
    pub relation: String,
}

/// Distribution-metadata facts extracted from one metadata file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistMetadataFacts {
    /// The metadata file these facts came from.
    pub file_id: FileId,
    /// Which metadata format.
    pub source: DistMetadataSource,
    /// Distribution name (e.g. `Foo-Bar`), if declared.
    pub name: Option<String>,
    /// Distribution version, if declared.
    pub version: Option<String>,
    /// The `abstract` one-line description, if declared.
    pub summary: Option<String>,
    /// Declared licenses (SPDX-ish tokens like `perl_5`).
    pub licenses: Vec<String>,
    /// Declared prerequisites.
    pub prereqs: Vec<Prereq>,
}

/// The prereq relations recognized in cpanfile / META.json.
const RELATIONS: &[&str] = &["requires", "recommends", "suggests", "conflicts"];
/// Canonical prerequisite phases recognized in cpanfile `on` blocks.
const CPANFILE_PHASES: &[&str] = &["configure", "build", "test", "runtime", "develop"];
/// META 1.x phase-specific top-level prerequisite keys → canonical phase.
const META_V1_PHASED_REQUIRES: &[(&str, &str)] =
    &[("configure_requires", "configure"), ("build_requires", "build")];
/// cpanfile statement keywords → (relation, phase).
const CPANFILE_KEYWORDS: &[(&str, &str, &str)] = &[
    ("configure_requires", "requires", "configure"),
    ("build_requires", "requires", "build"),
    ("test_requires", "requires", "test"),
    ("author_requires", "requires", "develop"),
    ("requires", "requires", "runtime"),
    ("recommends", "recommends", "runtime"),
    ("suggests", "suggests", "runtime"),
    ("conflicts", "conflicts", "runtime"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum CpanfileBlock {
    Phase(String),
    Unsupported,
}

/// Parse a `META.json` (CPAN Meta Spec v2, with a v1.4 flat fallback).
///
/// Returns `None` when the content is not valid JSON.
#[must_use]
pub fn parse_meta_json(file_id: FileId, content: &str) -> Option<DistMetadataFacts> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;

    let name = value.get("name").and_then(json_string);
    let version = value.get("version").and_then(json_scalar_string);
    let summary = value.get("abstract").and_then(json_string);
    let licenses = match value.get("license") {
        // v2: an array of license strings.
        Some(serde_json::Value::Array(items)) => {
            items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
        }
        // v1.4 / META.yml: a single string.
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    };

    let mut prereqs = Vec::new();
    let mut recovered_v2_entries = false;
    // v2: prereqs[phase][relation] = { module: version }.
    if let Some(serde_json::Value::Object(phases)) = value.get("prereqs") {
        for (phase, relations) in phases {
            let serde_json::Value::Object(relations) = relations else { continue };
            for (relation, modules) in relations {
                if !RELATIONS.contains(&relation.as_str()) {
                    continue;
                }
                recovered_v2_entries |= collect_modules(modules, phase, relation, &mut prereqs);
            }
        }
    }
    // v1.4 flat fallback: phase-specific *_requires plus runtime relations.
    if !recovered_v2_entries {
        for &(key, phase) in META_V1_PHASED_REQUIRES {
            if let Some(modules) = value.get(key) {
                let _ = collect_modules(modules, phase, "requires", &mut prereqs);
            }
        }
        for relation in RELATIONS {
            if let Some(modules) = value.get(relation) {
                let _ = collect_modules(modules, "runtime", relation, &mut prereqs);
            }
        }
    }
    prereqs.sort_by(|a, b| {
        (&a.phase, &a.relation, &a.module).cmp(&(&b.phase, &b.relation, &b.module))
    });

    Some(DistMetadataFacts {
        file_id,
        source: DistMetadataSource::MetaJson,
        name,
        version,
        summary,
        licenses,
        prereqs,
    })
}

/// Parse a `cpanfile` for its unconditional prerequisites (heuristic statement
/// scan — no Perl parser). Name/version/abstract are not declared in a cpanfile.
///
/// Handles both the flat form (`requires`, `test_requires`, …) and recognized
/// block forms (`on 'test' => sub { requires '...' }`). Other blocks are
/// deliberately ignored because this fact type cannot retain their predicates.
#[must_use]
pub fn parse_cpanfile(file_id: FileId, content: &str) -> DistMetadataFacts {
    let cleaned = strip_comments(content);
    let mut prereqs = Vec::new();
    let mut block_stack: Vec<CpanfileBlock> = Vec::new();
    let mut buf = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in cleaned.chars() {
        if let Some(delimiter) = quote {
            buf.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
            continue;
        }

        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                buf.push(ch);
            }
            ';' => {
                match active_cpanfile_scope(&block_stack) {
                    CpanfileScope::TopLevel => {
                        handle_cpanfile_statement(&buf, None, &mut prereqs);
                    }
                    CpanfileScope::Phase(phase) => {
                        handle_cpanfile_statement(&buf, Some(phase), &mut prereqs);
                    }
                    CpanfileScope::Unsupported => {}
                }
                buf.clear();
            }
            '{' => {
                let block = if matches!(block_stack.last(), Some(CpanfileBlock::Unsupported)) {
                    CpanfileBlock::Unsupported
                } else if let Some(phase) = parse_on_phase(&buf) {
                    CpanfileBlock::Phase(phase)
                } else {
                    CpanfileBlock::Unsupported
                };
                block_stack.push(block);
                buf.clear();
            }
            '}' => {
                match active_cpanfile_scope(&block_stack) {
                    CpanfileScope::TopLevel => {
                        handle_cpanfile_statement(&buf, None, &mut prereqs);
                    }
                    CpanfileScope::Phase(phase) => {
                        handle_cpanfile_statement(&buf, Some(phase), &mut prereqs);
                    }
                    CpanfileScope::Unsupported => {}
                }
                buf.clear();
                block_stack.pop();
            }
            _ => buf.push(ch),
        }
    }
    match active_cpanfile_scope(&block_stack) {
        CpanfileScope::TopLevel => handle_cpanfile_statement(&buf, None, &mut prereqs),
        CpanfileScope::Phase(phase) => handle_cpanfile_statement(&buf, Some(phase), &mut prereqs),
        CpanfileScope::Unsupported => {}
    }

    prereqs.sort_by(|a, b| {
        (&a.phase, &a.relation, &a.module).cmp(&(&b.phase, &b.relation, &b.module))
    });
    DistMetadataFacts {
        file_id,
        source: DistMetadataSource::Cpanfile,
        name: None,
        version: None,
        summary: None,
        licenses: Vec::new(),
        prereqs,
    }
}

enum CpanfileScope<'a> {
    TopLevel,
    Phase(&'a str),
    Unsupported,
}

fn active_cpanfile_scope(block_stack: &[CpanfileBlock]) -> CpanfileScope<'_> {
    match block_stack.last() {
        None => CpanfileScope::TopLevel,
        Some(CpanfileBlock::Phase(phase)) => CpanfileScope::Phase(phase.as_str()),
        Some(CpanfileBlock::Unsupported) => CpanfileScope::Unsupported,
    }
}

/// Recognize a prereq statement and push it, resolving its phase.
///
/// A prefixed keyword (`configure_requires`/`build_requires`/`test_requires`)
/// carries its own phase; a plain `requires`/`recommends`/`suggests` uses the
/// enclosing `on 'phase'` block's phase, defaulting to `runtime`.
fn handle_cpanfile_statement(buf: &str, block_phase: Option<&str>, out: &mut Vec<Prereq>) {
    let statement = buf.trim();
    // The keyword boundary check prevents prefix collisions.
    let Some((_kw, relation, kw_phase)) =
        CPANFILE_KEYWORDS.iter().find(|(kw, _, _)| starts_with_cpanfile_keyword(statement, kw))
    else {
        return;
    };
    let phase = if *kw_phase == "runtime" { block_phase.unwrap_or("runtime") } else { kw_phase };
    let quoted = quoted_strings(statement);
    if let Some(module) = quoted.first() {
        out.push(Prereq {
            module: module.clone(),
            version: quoted.get(1).cloned(),
            phase: phase.to_string(),
            relation: (*relation).to_string(),
        });
    }
}

fn starts_with_cpanfile_keyword(statement: &str, keyword: &str) -> bool {
    let Some(rest) = statement.strip_prefix(keyword) else {
        return false;
    };
    rest.chars().next().is_none_or(|ch| ch.is_whitespace() || matches!(ch, '(' | '\'' | '"'))
}

/// Extract a canonical phase from an `on 'phase' => sub` block header.
fn parse_on_phase(buf: &str) -> Option<String> {
    let rest = buf.trim().strip_prefix("on")?;
    // `on` must be followed by whitespace or a quote, not be part of a longer word.
    if !rest.starts_with(|c: char| c.is_whitespace() || matches!(c, '(' | '\'' | '"')) {
        return None;
    }
    // Prefer a quoted phase (`on 'test'`); fall back to a bareword (`on test`).
    // A quoted candidate takes precedence, so a non-canonical first quoted string does not consult the bareword fallback.
    let phase = quoted_strings(buf)
        .into_iter()
        .next()
        .or_else(|| rest.split_whitespace().next().map(str::to_string))?;
    CPANFILE_PHASES.contains(&phase.as_str()).then_some(phase)
}

/// Collect `{ module: version }` object entries into prereqs.
fn collect_modules(
    modules: &serde_json::Value,
    phase: &str,
    relation: &str,
    out: &mut Vec<Prereq>,
) -> bool {
    let serde_json::Value::Object(map) = modules else { return false };
    let mut recovered = false;
    for (module, version) in map {
        let Some(version) = json_scalar_string(version) else { continue };
        out.push(Prereq {
            module: module.clone(),
            version: Some(version),
            phase: phase.to_string(),
            relation: relation.to_string(),
        });
        recovered = true;
    }
    recovered
}

/// A JSON value as a string, only if it *is* a string.
fn json_string(value: &serde_json::Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

/// A JSON scalar (string or number) coerced to a string.
///
/// Note: a bare JSON *number* version like `1.20` round-trips through `f64` and
/// serializes back as `1.2` (trailing zeros lost). CPAN Meta Spec recommends
/// versions be strings for exactly this reason; string versions are preserved
/// verbatim.
fn json_scalar_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Strip `#` comments from cpanfile source, preserving `#` inside quotes.
fn strip_comments(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    for line in content.lines() {
        let mut in_single = false;
        let mut in_double = false;
        for ch in line.chars() {
            match ch {
                '\'' if !in_double => in_single = !in_single,
                '"' if !in_single => in_double = !in_double,
                '#' if !in_single && !in_double => break,
                _ => {}
            }
            out.push(ch);
        }
        out.push('\n');
    }
    out
}

/// Extract single- or double-quoted string literals from a statement.
fn quoted_strings(statement: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = statement.chars();
    while let Some(ch) = chars.next() {
        if ch == '\'' || ch == '"' {
            let quote = ch;
            let mut literal = String::new();
            for c in chars.by_ref() {
                if c == quote {
                    break;
                }
                literal.push(c);
            }
            out.push(literal);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]
    use super::*;
    use crate::id::Digest;

    fn fid() -> FileId {
        FileId::new("META.json", &Digest::of("x"))
    }

    #[test]
    fn parses_meta_json_v2() {
        let content = r#"{
            "name": "Foo-Bar",
            "version": "1.23",
            "abstract": "does foo to bar",
            "license": ["perl_5"],
            "prereqs": {
                "runtime": { "requires": { "strict": "0", "Moo": "2.0" } },
                "test":    { "requires": { "Test::More": "0.98" } }
            }
        }"#;
        let facts = parse_meta_json(fid(), content).unwrap();
        assert_eq!(facts.name.as_deref(), Some("Foo-Bar"));
        assert_eq!(facts.version.as_deref(), Some("1.23"));
        assert_eq!(facts.summary.as_deref(), Some("does foo to bar"));
        assert_eq!(facts.licenses, vec!["perl_5"]);
        assert!(facts.prereqs.iter().any(|p| p.module == "Moo" && p.phase == "runtime"));
        assert!(facts.prereqs.iter().any(|p| p.module == "Test::More" && p.phase == "test"));
    }

    #[test]
    fn coerces_numeric_version() {
        let facts = parse_meta_json(fid(), r#"{"name":"X","version":1.5}"#).unwrap();
        assert_eq!(facts.version.as_deref(), Some("1.5"), "numeric version coerced to string");
    }

    #[test]
    fn v1_4_flat_prereqs_and_string_license() {
        let content = r#"{"name":"X","license":"perl","requires":{"Carp":"0"}}"#;
        let facts = parse_meta_json(fid(), content).unwrap();
        assert_eq!(facts.licenses, vec!["perl"], "v1.4 single-string license");
        assert!(
            facts.prereqs.iter().any(|p| p.module == "Carp" && p.relation == "requires"),
            "flat top-level requires read as fallback"
        );
    }

    #[test]
    fn v1_4_phase_specific_prereqs_are_retained() {
        let content = r#"{
            "configure_requires": {"ExtUtils::MakeMaker": "6.64"},
            "build_requires": {"Test::More": "0.88"},
            "requires": {"Carp": "0"}
        }"#;
        let facts = parse_meta_json(fid(), content).unwrap();
        let mapped = facts
            .prereqs
            .iter()
            .map(|p| {
                (p.module.as_str(), p.version.as_deref(), p.phase.as_str(), p.relation.as_str())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            mapped,
            vec![
                ("Test::More", Some("0.88"), "build", "requires"),
                ("ExtUtils::MakeMaker", Some("6.64"), "configure", "requires"),
                ("Carp", Some("0"), "runtime", "requires"),
            ],
            "META 1.x prerequisite keys retain phase, relation, version, and deterministic order"
        );
    }

    #[test]
    fn v2_prereqs_take_precedence_over_flat_v1_fields() {
        let facts = parse_meta_json(
            fid(),
            r#"{
                "prereqs": {"runtime": {"requires": {"V2::Only": "1"}}},
                "configure_requires": {"V1::Only": "1"},
                "requires": {"V1::Runtime": "1"}
            }"#,
        )
        .unwrap();

        assert_eq!(
            facts.prereqs,
            vec![Prereq {
                module: "V2::Only".to_string(),
                version: Some("1".to_string()),
                phase: "runtime".to_string(),
                relation: "requires".to_string(),
            }]
        );
    }

    #[test]
    fn empty_or_malformed_v2_prereqs_fall_back_to_flat_v1_fields() {
        for v2 in [r#"{}"#, r#"{"runtime": []}"#, r#"{"runtime": "bad"}"#] {
            let content =
                format!(r#"{{"prereqs": {v2}, "configure_requires": {{"V1::Only": "1"}}}}"#);
            let facts = parse_meta_json(fid(), &content).unwrap();
            assert_eq!(facts.prereqs.len(), 1, "v2={v2}");
            assert_eq!(facts.prereqs[0].module, "V1::Only", "v2={v2}");
            assert_eq!(facts.prereqs[0].phase, "configure", "v2={v2}");
        }
    }

    #[test]
    fn malformed_phase_maps_do_not_fabricate_prereqs_or_panic() {
        let facts = parse_meta_json(
            fid(),
            r#"{
                "prereqs": {
                    "runtime": [],
                    "test": "not a relation map",
                    "develop": null,
                    "build": {"requires": ["not", "a", "module map"]}
                },
                "configure_requires": [],
                "build_requires": "not a module map",
                "requires": null
            }"#,
        )
        .unwrap();

        assert!(facts.prereqs.is_empty());
    }

    #[test]
    fn malformed_v2_relations_are_ignored_without_fabricated_facts() {
        let facts = parse_meta_json(
            fid(),
            r#"{
                "prereqs": {
                    "runtime": {
                        "unknown_relation": {"Fabricated::Fact": "1"},
                        "requires": null,
                        "recommends": {"Real::Fact": "2"}
                    }
                }
            }"#,
        )
        .unwrap();

        assert_eq!(facts.prereqs.len(), 1);
        assert_eq!(facts.prereqs[0].module, "Real::Fact");
        assert!(!facts.prereqs.iter().any(|p| p.module == "Fabricated::Fact"));
    }

    #[test]
    fn invalid_json_returns_none() {
        assert!(parse_meta_json(fid(), "{not json").is_none());
    }

    #[test]
    fn parses_cpanfile() {
        let content = "requires 'Moo', '2.0';\n# a comment with 'quotes'\ntest_requires 'Test::More';\nrequires 'Path::Tiny';\n";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);
        assert!(facts.prereqs.iter().any(|p| p.module == "Moo"
            && p.version.as_deref() == Some("2.0")
            && p.phase == "runtime"));
        assert!(
            facts.prereqs.iter().any(|p| p.module == "Test::More" && p.phase == "test"),
            "test_requires → test phase; prereqs={:?}",
            facts.prereqs
        );
        assert!(facts.prereqs.iter().any(|p| p.module == "Path::Tiny"));
        // The comment's quoted text must not leak in as a module.
        assert!(!facts.prereqs.iter().any(|p| p.module == "quotes"));
    }

    #[test]
    fn cpanfile_block_form_phase_deps() {
        // Module::CPANfile block syntax: `on 'phase' => sub { requires ... }`.
        let content = "requires 'Moo';\non 'test' => sub {\n    requires 'Test::More', '0.88';\n};\non 'develop' => sub {\n    requires 'Perl::Critic';\n};\n";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);
        assert!(
            facts.prereqs.iter().any(|p| p.module == "Moo" && p.phase == "runtime"),
            "flat requires stays runtime"
        );
        assert!(
            facts.prereqs.iter().any(|p| p.module == "Test::More" && p.phase == "test"),
            "block-form requires picks up the on-phase; prereqs={:?}",
            facts.prereqs
        );
        assert!(
            facts.prereqs.iter().any(|p| p.module == "Perl::Critic" && p.phase == "develop"),
            "develop block phase"
        );
    }

    #[test]
    fn cpanfile_quoted_delimiters_are_statement_text() {
        let content = r#"
            my $open = '{';
            my $close = "}";
            my $separator = ";";
            requires 'Path::Tiny';
        "#;
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert!(
            facts.prereqs.iter().any(|p| p.module == "Path::Tiny" && p.phase == "runtime"),
            "quoted braces and semicolons must remain statement text: {:?}",
            facts.prereqs
        );
    }

    #[test]
    fn cpanfile_parenthesized_on_phase_is_recognized() {
        let content = "on('test') => sub { requires 'Test::More'; };";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert!(
            facts.prereqs.iter().any(|p| p.module == "Test::More" && p.phase == "test"),
            "parenthesized on blocks retain their canonical phase: {:?}",
            facts.prereqs
        );
    }

    #[test]
    fn cpanfile_nested_on_blocks_use_innermost_phase() {
        let content = "on 'test' => sub { on 'build' => sub { requires 'Nested::Build'; }; };";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        let nested_build: Vec<_> =
            facts.prereqs.iter().filter(|p| p.module == "Nested::Build").collect();
        assert_eq!(nested_build.len(), 1);
        assert_eq!(nested_build[0].phase, "build");
    }

    #[test]
    fn cpanfile_canonical_on_phases_emit_declared_phase() {
        let content = "on 'runtime' => sub { requires 'Runtime::Dep'; }; on 'configure' => sub { requires 'Configure::Dep'; };";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert!(facts.prereqs.iter().any(|p| p.module == "Runtime::Dep" && p.phase == "runtime"));
        assert!(
            facts.prereqs.iter().any(|p| p.module == "Configure::Dep" && p.phase == "configure")
        );
    }

    #[test]
    fn cpanfile_unsupported_blocks_do_not_become_unconditional_facts() {
        let content = r#"
            requires 'Top::Level';
            feature 'SQLite' => sub {
                requires 'DBD::SQLite';
                on 'test' => sub { requires 'Feature::Test'; };
            };
            if ($^O eq 'MSWin32') {
                build_requires 'Win32::Build';
            }
            on 'test' => sub {
                requires 'Test::More';
                if ($ENV{AUTHOR_TESTING}) { requires 'Test::Warnings'; }
                recommends 'Test::Deep';
            };
        "#;
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert!(facts.prereqs.iter().any(|p| p.module == "Top::Level" && p.phase == "runtime"));
        assert!(facts.prereqs.iter().any(|p| p.module == "Test::More" && p.phase == "test"));
        assert!(facts.prereqs.iter().any(|p| p.module == "Test::Deep" && p.phase == "test"));
        for conditional in ["DBD::SQLite", "Feature::Test", "Win32::Build", "Test::Warnings"] {
            assert!(
                !facts.prereqs.iter().any(|p| p.module == conditional),
                "conditional prerequisite {conditional} must not become unconditional: {:?}",
                facts.prereqs
            );
        }
    }

    #[test]
    fn cpanfile_unknown_phases_and_keyword_prefixes_are_rejected() {
        let content = r#"
            on 'deploy' => sub { requires 'Deploy::Only'; };
            requires_extra 'Prefix::Collision';
            oncall 'test' => sub { requires 'Not::An::On::Block'; };
            on 'build' => sub { requires 'Build::Known'; };
        "#;
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert_eq!(
            facts.prereqs,
            vec![Prereq {
                module: "Build::Known".to_string(),
                version: None,
                phase: "build".to_string(),
                relation: "requires".to_string(),
            }]
        );
    }

    #[test]
    fn cpanfile_longest_keyword_wins() {
        // `configure_requires` must not be captured by the `requires` prefix.
        let facts = parse_cpanfile(
            FileId::new("cpanfile", &Digest::of("x")),
            "configure_requires 'ExtUtils::MakeMaker';\n",
        );
        let p = facts.prereqs.iter().find(|p| p.module == "ExtUtils::MakeMaker").unwrap();
        assert_eq!(p.phase, "configure");
    }

    #[test]
    fn cpanfile_conflicts_is_recognized() {
        // Regression: `conflicts` is a documented cpanfile/META relation
        // (RELATIONS includes it), but CPANFILE_KEYWORDS previously had no
        // entry for it, so `conflicts 'Foo';` was silently dropped.
        let facts = parse_cpanfile(
            FileId::new("cpanfile", &Digest::of("x")),
            "conflicts 'Some::Broken::Module';\n",
        );
        assert!(
            facts.prereqs.iter().any(|p| p.module == "Some::Broken::Module"),
            "conflicts statement must produce a prereq entry; prereqs={:?}",
            facts.prereqs
        );
        let p = facts.prereqs.iter().find(|p| p.module == "Some::Broken::Module").unwrap();
        assert_eq!(p.relation, "conflicts");
        assert_eq!(p.phase, "runtime");
    }
}
