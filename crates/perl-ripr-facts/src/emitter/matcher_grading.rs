//! Exception-matcher grading for `throws_ok` and `like(dies {...}, ...)`
//! (perl-lsp-swarm#18499).
//!
//! The base oracle table grades every exception observer `weak_broad` because a
//! callee name alone cannot say what the assertion pins. The call's arguments
//! can. This module reads the parsed argument list of two assertion shapes and
//! records, per call node, whether a literal matcher pins the error:
//!
//! - `throws_ok BLOCK MATCHER, 'name'` — the second argument is the matcher;
//! - `like(dies {...}, MATCHER, 'name')` / `like(exception {...}, MATCHER)` —
//!   the matcher trails the nested exception block.
//!
//! A matcher pins the error only when it is a non-empty regex literal or a
//! non-empty string literal that is not a bare variable (an error class name or
//! message pattern). A variable matcher, a missing matcher, and a bare
//! `dies_ok` are left to the base table: the producer does not guess what a
//! runtime value matches. The nested `dies`/`exception` block of a graded
//! `like` is the same observation, so it is reported as part of the single
//! `like` oracle and never as a second one.
//!
//! This is the producer side of the `oracle_matcher_grading` capability that
//! ripr's RIPR-SPEC-0235 rule 16 reads.

use perl_parser_core::{Node, NodeKind, RegexSurfaceForm};

/// The graded reading of one call node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatcherGrade {
    /// A literal class or pattern pins the error: `strong_exact`.
    Pinned,
    /// No literal matcher pins it: stays `weak_broad`.
    Broad,
}

/// One graded assertion call, located by the call node's byte span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GradedCall {
    /// The asserting call's name (`throws_ok` or `like`).
    pub name: &'static str,
    /// Start byte of the asserting call node.
    pub start: usize,
    /// Pinned or broad.
    pub grade: MatcherGrade,
    /// Start byte of a nested `dies`/`exception` call that is part of this
    /// assertion and must not be reported as a second oracle.
    pub nested_start: Option<usize>,
}

/// Grade every `throws_ok` and `like(dies/exception {...}, ...)` call in `ast`.
pub(crate) fn grade_matchers(ast: &Node) -> Vec<GradedCall> {
    let mut out = Vec::new();
    walk(ast, &mut out);
    // A file that defines its own `throws_ok` or `like` is not calling the
    // library assertion, so its call sites are not graded.
    let mut shadowed = Vec::new();
    find_defined_subs(ast, &mut shadowed);
    out.retain(|call| !shadowed.iter().any(|name| name == call.name));
    out
}

fn find_defined_subs(node: &Node, out: &mut Vec<String>) {
    if let NodeKind::Subroutine { name: Some(name), .. } = &node.kind {
        out.push(name.rsplit("::").next().unwrap_or(name).to_owned());
    }
    node.for_each_child(|child| find_defined_subs(child, out));
}

fn walk(node: &Node, out: &mut Vec<GradedCall>) {
    if let NodeKind::FunctionCall { name, args } = &node.kind {
        match name.as_str() {
            "throws_ok" => out.push(GradedCall {
                name: "throws_ok",
                start: node.location.start,
                grade: grade_matcher(args.get(1)),
                nested_start: None,
            }),
            "like" => {
                if let Some(inner) = args.first()
                    && let NodeKind::FunctionCall { name: inner_name, args: inner_args } =
                        &inner.kind
                    && matches!(inner_name.as_str(), "dies" | "exception")
                {
                    // The matcher either trails the nested block (the parser
                    // attaches it to the inner call) or is `like`'s own second
                    // argument.
                    let matcher = inner_args.get(1).or_else(|| args.get(1));
                    out.push(GradedCall {
                        name: "like",
                        start: node.location.start,
                        grade: grade_matcher(matcher),
                        nested_start: Some(inner.location.start),
                    });
                }
            }
            _ => {}
        }
    }
    node.for_each_child(|child| walk(child, out));
}

/// The regex source between its delimiters. The parser keeps the delimiters in
/// `pattern` (`qr/boom/` → `/boom/`, `qr{}` → `{}`), and every Perl delimiter
/// pair is one character wide.
fn regex_body(pattern: &str) -> &str {
    let mut chars = pattern.chars();
    chars.next();
    chars.next_back();
    chars.as_str()
}

/// Whether a quoted string literal names a class or message. The parser keeps
/// the quotes in `value`; only plain `'...'` and `"..."` literals are read.
/// `q()`-style quoting, an empty literal, and a bare interpolated variable are
/// not guessed at.
fn literal_pins(value: &str) -> bool {
    let quoted =
        ['\'', '"'].iter().find_map(|quote| value.strip_prefix(*quote)?.strip_suffix(*quote));
    let Some(body) = quoted else { return false };
    let body = body.trim();
    // Interpolation makes the matched text a runtime value; do not guess.
    !body.is_empty() && !body.contains('$') && !body.contains('@') && pattern_pins(body)
}

/// Whether a regex body can reject some error text. Empty, a bare variable,
/// and catch-alls (`.*`, `.`, `^`, `$`) match every message.
fn pattern_pins(body: &str) -> bool {
    let body = body.trim();
    let core = body.trim_start_matches('^').trim_end_matches('$').trim();
    let catch_all = ["", ".", ".*", ".+", ".*?", ".+?", "(?s).*", "[\\s\\S]*"];
    if catch_all.contains(&core) {
        return false;
    }
    // A pattern that is one interpolated variable (`$re`, `${re}`, `@x`).
    !(body.starts_with('$') || body.starts_with('@'))
        || body.chars().skip(1).any(|c| !(c.is_alphanumeric() || c == '_' || c == '{' || c == '}'))
}

fn grade_matcher(matcher: Option<&Node>) -> MatcherGrade {
    let Some(matcher) = matcher else { return MatcherGrade::Broad };
    let pins = match &matcher.kind {
        // Only `qr//` builds a matcher. `m//` and `/.../` match `$_` and yield
        // a boolean, which pins nothing.
        NodeKind::Regex { form: RegexSurfaceForm::Qr, pattern, replacement: None, .. } => {
            pattern_pins(regex_body(pattern))
        }
        NodeKind::String { value, .. } => literal_pins(value),
        _ => false,
    };
    if pins { MatcherGrade::Pinned } else { MatcherGrade::Broad }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use perl_parser_core::Parser;

    fn grades(src: &str) -> Vec<(&'static str, MatcherGrade, bool)> {
        let mut parser = Parser::new(src);
        let ast = parser.parse().expect("test source parses");
        grade_matchers(&ast)
            .into_iter()
            .map(|call| (call.name, call.grade, call.nested_start.is_some()))
            .collect()
    }

    #[test]
    fn throws_ok_with_class_or_regex_is_pinned() {
        for src in [
            "throws_ok { die 1 } 'My::Error', 'n';",
            "throws_ok { die 1 } 'My::Error';",
            "throws_ok { die 1 } qr/boom/, 'n';",
            "throws_ok { die 1 } qr/boom/;",
            "throws_ok(sub { die 1 }, 'My::Error', 'n');",
            "throws_ok(sub { die 1 }, qr/boom/, 'n');",
        ] {
            assert_eq!(grades(src), [("throws_ok", MatcherGrade::Pinned, false)], "src: {src}");
        }
    }

    #[test]
    fn throws_ok_without_a_literal_matcher_stays_broad() {
        for src in [
            "throws_ok { die 1 } $re, 'n';",
            "throws_ok { die 1 } $re;",
            "throws_ok { die 1 } qr//, 'n';",
            "throws_ok { die 1 } '', 'n';",
            "throws_ok { die 1 };",
        ] {
            assert_eq!(grades(src), [("throws_ok", MatcherGrade::Broad, false)], "src: {src}");
        }
    }

    #[test]
    fn like_over_an_exception_block_is_one_graded_assertion() {
        for src in [
            "like(dies { die 1 }, qr/boom/, 'n');",
            "like(exception { die 1 }, qr/boom/, 'n');",
            "like(exception { die 1 }, 'boom');",
        ] {
            assert_eq!(grades(src), [("like", MatcherGrade::Pinned, true)], "src: {src}");
        }
        assert_eq!(
            grades("like(dies { die 1 }, $re, 'n');"),
            [("like", MatcherGrade::Broad, true)]
        );
    }

    #[test]
    fn matchers_that_pin_nothing_stay_broad() {
        for src in [
            "throws_ok { die 1 } qr/.*/, 'n';",
            "throws_ok { die 1 } qr/^.*$/, 'n';",
            "throws_ok { die 1 } qr/$re/, 'n';",
            "throws_ok { die 1 } m/boom/, 'n';",
            "throws_ok { die 1 } \"x$y\", 'n';",
        ] {
            assert_eq!(grades(src), [("throws_ok", MatcherGrade::Broad, false)], "src: {src}");
        }
        assert_eq!(
            grades("like(dies { die 1 }, '.', 'n');"),
            [("like", MatcherGrade::Broad, true)]
        );
    }

    #[test]
    fn user_defined_assertion_subs_are_not_graded() {
        assert!(grades("sub throws_ok { 1 }\nthrows_ok { die 1 } 'X';").is_empty());
        assert!(grades("sub like { 1 }\nlike(dies { die 1 }, qr/boom/);").is_empty());
    }

    #[test]
    fn dies_ok_and_value_like_are_not_graded_here() {
        for src in [
            "dies_ok { die 1 } 'n';",
            "like($got, qr/x/, 'n');",
            "my $e = dies { die 1 }; like($e, qr/x/);",
            "is(exception { die 1 }, 'x');",
        ] {
            assert!(grades(src).is_empty(), "src: {src}");
        }
    }
}
