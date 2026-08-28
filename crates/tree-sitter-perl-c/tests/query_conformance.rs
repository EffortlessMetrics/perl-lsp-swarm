#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
use std::{collections::BTreeMap, error::Error};

use tree_sitter::{Query, QueryCursor, StreamingIterator};
use tree_sitter_perl_c::{language, parse_perl_code};

type CaptureMap = BTreeMap<String, Vec<String>>;

fn collect_captures(query_source: &str, source: &str) -> Result<CaptureMap, Box<dyn Error>> {
    let tree = parse_perl_code(source)?;
    let query = Query::new(&language(), query_source)?;
    let mut cursor = QueryCursor::new();
    let mut captures: CaptureMap = BTreeMap::new();

    let mut query_captures = cursor.captures(&query, tree.root_node(), source.as_bytes());
    while let Some((query_match, capture_index)) = query_captures.next() {
        let capture =
            query_match.captures.get(*capture_index).ok_or("missing capture at reported index")?;

        let capture_name = query
            .capture_names()
            .get(capture.index as usize)
            .ok_or("missing capture name")?
            .to_string();

        let text = capture.node.utf8_text(source.as_bytes())?.to_string();
        captures.entry(capture_name).or_default().push(text);
    }

    Ok(captures)
}

fn assert_capture_contains(captures: &CaptureMap, capture_name: &str, needle: &str) {
    let matched = captures
        .get(capture_name)
        .is_some_and(|values| values.iter().any(|value| value.contains(needle)));

    let available = captures
        .iter()
        .map(|(name, values)| format!("{name}: {}", values.join(" | ")))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        matched,
        "expected capture `{capture_name}` to include `{needle}`\navailable captures:\n{available}"
    );
}

fn focused_query_from_upstream(upstream: &str, fragments: &[&str]) -> String {
    for fragment in fragments {
        assert!(
            upstream.contains(fragment),
            "expected upstream query to contain fragment:\n{fragment}"
        );
    }

    fragments.join("\n")
}

#[test]
fn query_conformance_highlights_covers_core_perl_constructs() -> Result<(), Box<dyn Error>> {
    let highlights_upstream = include_str!("../../../tree-sitter-perl/queries/highlights.scm");
    let highlights_query = focused_query_from_upstream(
        highlights_upstream,
        &[
            r#"((source_file . (comment) @preproc)
  (#lua-match? @preproc "^#!/"))"#,
            r#"[ "use" "no" "require" ] @include"#,
            r#"[ "sub" "method" "async" "extended" ] @keyword.function"#,
            r#"(subroutine_declaration_statement name: (bareword) @function)"#,
            r#"(scalar) @variable.scalar"#,
            r#"(comment) @comment"#,
        ],
    );

    let source = r#"#!/usr/bin/perl
use strict;
my $value = 42;
sub greet {
  return $value;
}
# trailing comment
"#;

    let captures = collect_captures(&highlights_query, source)?;

    assert_capture_contains(&captures, "preproc", "#!/usr/bin/perl");
    assert_capture_contains(&captures, "include", "use");
    assert_capture_contains(&captures, "keyword.function", "sub");
    assert_capture_contains(&captures, "function", "greet");
    assert_capture_contains(&captures, "variable.scalar", "$value");
    assert_capture_contains(&captures, "comment", "# trailing comment");
    Ok(())
}

#[test]
fn query_conformance_highlights_covers_quote_like_and_heredoc_nodes() -> Result<(), Box<dyn Error>>
{
    let highlights_upstream = include_str!("../../../tree-sitter-perl/queries/highlights.scm");
    let highlights_query = focused_query_from_upstream(
        highlights_upstream,
        &[
            r#"[
  (heredoc_token)
  (command_heredoc_token)
  (heredoc_end)
] @label"#,
            r#"[
  (string_literal)
  (interpolated_string_literal)
  (quoted_word_list)
  (command_string)
  (heredoc_content)
  (replacement)
  (transliteration_content)
] @string"#,
            r#"[
 (quoted_regexp)
 (match_regexp)
 (regexp_content)
] @string.regex"#,
        ],
    );

    let source = r#"my $rx = qr/foo.*/;
my $sql = <<'SQL';
SELECT * FROM users;
SQL
"#;

    let captures = collect_captures(&highlights_query, source)?;

    assert_capture_contains(&captures, "string.regex", "foo.*");
    assert_capture_contains(&captures, "string", "SELECT * FROM users");
    assert_capture_contains(&captures, "label", "SQL");

    Ok(())
}

#[test]
fn query_conformance_folds_covers_block_comment_pod_and_heredoc_regions()
-> Result<(), Box<dyn Error>> {
    let query = include_str!("../../../tree-sitter-perl/queries/folds.scm");
    let source = r#"# heading comment
# second comment
sub fold_me {
  my $value = 1;
}
my $sql = <<'SQL';
SELECT 1;
SQL
=pod
Fold me
=cut
"#;

    let captures = collect_captures(query, source)?;

    assert_capture_contains(&captures, "fold", "# heading comment");
    assert_capture_contains(&captures, "fold", "# second comment");
    assert_capture_contains(&captures, "fold", "sub fold_me {");
    assert_capture_contains(&captures, "fold", "SELECT 1;");
    assert_capture_contains(&captures, "fold", "Fold me");

    Ok(())
}

#[test]
fn query_conformance_injections_covers_inline_c_and_cpp_heredocs() -> Result<(), Box<dyn Error>> {
    let query = include_str!("../../../tree-sitter-perl/queries/injections.scm");

    let inline_c = r#"use Inline C => <<'END_C';
#include <math.h>
double calc(double x) { return sqrt(x); }
END_C
"#;
    let inline_captures = collect_captures(query, inline_c)?;
    assert_capture_contains(&inline_captures, "inline.package", "Inline");
    assert_capture_contains(&inline_captures, "inline.language", "C");
    assert_capture_contains(&inline_captures, "injection.content", "#include <math.h>");

    let inline_cpp = r#"use Inline CPP => <<'END_CPP';
#include <string>
class Greeter {};
END_CPP
"#;
    let inline_cpp_captures = collect_captures(query, inline_cpp)?;
    assert_capture_contains(&inline_cpp_captures, "inline.package", "Inline");
    assert_capture_contains(&inline_cpp_captures, "inline.language", "CPP");
    assert_capture_contains(&inline_cpp_captures, "injection.content", "#include <string>");

    Ok(())
}

#[test]
fn query_conformance_injections_covers_comment_and_eval_substitution() -> Result<(), Box<dyn Error>>
{
    let query = include_str!("../../../tree-sitter-perl/queries/injections.scm");
    let source = r#"# language payload
my $value = "x";
$value =~ s/x/uc($value)/e;
"#;

    let captures = collect_captures(query, source)?;

    assert_capture_contains(&captures, "injection.content", "# language payload");
    assert_capture_contains(&captures, "injection.content", "uc($value)");

    Ok(())
}

// ── Vendored-copy drift oracle (joint-refresh contract) ─────────────────
//
// bdd_workflows.rs exercises only the vendored constants while this suite
// exercises only the upstream root files, so both suites can stay green
// while the published loader silently ships stale query rules. Assert the
// UPSTREAM_SNAPSHOT.md joint-refresh contract directly: each vendored copy
// must equal its upstream source modulo the documented whitespace
// normalization (trailing whitespace per line, trailing blank lines at EOF)
// that the repository's binary-diff gate requires.

fn normalized_query_lines(source: &str) -> String {
    let mut lines: Vec<&str> = source.lines().map(str::trim_end).collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

#[test]
fn query_conformance_vendored_queries_match_upstream_modulo_documented_normalization() {
    for (query_name, upstream, vendored) in [
        (
            "injections.scm",
            include_str!("../../../tree-sitter-perl/queries/injections.scm"),
            tree_sitter_perl_c::INJECTIONS_QUERY,
        ),
        (
            "highlights.scm",
            include_str!("../../../tree-sitter-perl/queries/highlights.scm"),
            tree_sitter_perl_c::HIGHLIGHTS_QUERY,
        ),
    ] {
        assert_eq!(
            normalized_query_lines(upstream),
            normalized_query_lines(vendored),
            "vendored {query_name} drifted from the upstream snapshot; refresh both \
             copies together per UPSTREAM_SNAPSHOT.md (the published loader must not \
             ship stale query rules)"
        );
    }
}
