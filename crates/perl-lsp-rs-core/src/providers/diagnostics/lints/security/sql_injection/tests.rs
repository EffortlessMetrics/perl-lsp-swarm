//! Discriminating PL607 tests for reviewed DBI SQL-text sinks (#5035 / #16864).
//!
//! These tests pin the landed conservative producer and the remaining first-
//! cohort edge cases. They are the instrument for the extract/simplify, not a
//! wrap-up. Canonical call/value facts, currentness, ORM support, and autofix
//! remain out of scope: those cases must stay silent here.

use super::{DBI_SQL_TEXT_SINKS, check_sql_injection, is_dbi_sql_text_sink};
use crate::providers::diagnostics::internal_types::Diagnostic;
use perl_diagnostics::codes::DiagnosticSeverity;
use perl_parser::Parser;
use perl_tdd_support::must;
use perl_test_must::must_some_with;

fn sql_diags(source: &str) -> Vec<Diagnostic> {
    let ast = must(Parser::new(source).parse());
    let mut diags = vec![];
    check_sql_injection(&ast, &mut diags);
    diags
}

fn dbh_connect() -> &'static str {
    r#"my $dbh = DBI->connect("dbi:Pg:dbname=x", "u", "p");"#
}

fn pl607(diags: &[Diagnostic]) -> Option<&Diagnostic> {
    diags.iter().find(|d| d.code.as_deref() == Some("PL607"))
}

fn pl607_count(diags: &[Diagnostic]) -> usize {
    diags.iter().filter(|d| d.code.as_deref() == Some("PL607")).count()
}

#[test]
fn check_security_dispatch_still_publishes_pl607() {
    let source = format!(
        "{}\nmy $user_id = 42;\nmy $sth = $dbh->prepare(\"SELECT * FROM users WHERE id = $user_id\");\n",
        dbh_connect()
    );
    let ast = must(Parser::new(&source).parse());
    let mut diags = vec![];
    super::super::check_security(&ast, &mut diags);
    assert!(
        pl607(&diags).is_some(),
        "PL607 must still reach the client through check_security: {diags:?}"
    );
}

#[test]
fn interpolated_prepare_is_flagged_with_exact_range() {
    let source = format!(
        "{}\nmy $user_id = 42;\nmy $sth = $dbh->prepare(\"SELECT * FROM users WHERE id = $user_id\");\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    let diagnostic = must_some_with(
        pl607(&diags),
        format!("interpolated prepare must be flagged as PL607: {diags:?}"),
    );
    let (start, end) = diagnostic.range;
    assert_eq!(
        &source[start..end],
        r#"$dbh->prepare("SELECT * FROM users WHERE id = $user_id")"#,
        "PL607 byte range must cover the exact prepare call"
    );
    assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
    assert!(diagnostic.critic_observation.is_none());
    assert!(diagnostic.suggestion.is_some());
}

#[test]
fn placeholder_prepare_is_silent() {
    let source = format!(
        "{}\nmy $sth = $dbh->prepare('SELECT * FROM users WHERE id = ?');\n$sth->execute(42);\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_none(),
        "placeholders with bind values are the safe control: {diags:?}"
    );
}

#[test]
fn concatenated_variable_sql_is_flagged() {
    let source = format!(
        "{}\nmy $user_input = <STDIN>;\nmy $sth = $dbh->prepare('SELECT * FROM users WHERE id = ' . $user_input);\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_some(),
        "concatenated variable SQL must be flagged as PL607: {diags:?}"
    );
}

#[test]
fn concatenated_literal_only_sql_is_silent() {
    let source =
        format!("{}\nmy $sth = $dbh->prepare('SELECT ' . '*' . ' FROM users');\n", dbh_connect());
    let diags = sql_diags(&source);
    assert!(pl607(&diags).is_none(), "literal-only concatenation carries no variable: {diags:?}");
}

#[test]
fn interpolated_do_is_flagged_and_placeholder_do_is_silent() {
    let flagged = format!(
        "{}\nmy $name = <STDIN>;\n$dbh->do(\"DELETE FROM users WHERE name = $name\");\n",
        dbh_connect()
    );
    let diags = sql_diags(&flagged);
    assert!(pl607(&diags).is_some(), "interpolated do() must be flagged as PL607: {diags:?}");

    let safe =
        format!("{}\n$dbh->do('DELETE FROM users WHERE name = ?', undef, 'bob');\n", dbh_connect());
    let diags = sql_diags(&safe);
    assert!(
        pl607(&diags).is_none(),
        "placeholder do() with bind values must stay silent: {diags:?}"
    );
}

#[test]
fn prepare_cached_is_a_statement_sink() {
    let source = format!(
        "{}\nmy $id = <STDIN>;\nmy $sth = $dbh->prepare_cached(\"SELECT * FROM t WHERE id = $id\");\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_some(),
        "prepare_cached takes SQL text exactly like prepare: {diags:?}"
    );
}

#[test]
fn execute_bind_values_are_never_a_sql_text_sink() {
    let source = format!(
        "{}\nmy $sth = $dbh->prepare('SELECT * FROM users WHERE id = ?');\nmy $id = <STDIN>;\n$sth->execute(\"$id\");\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_none(),
        "execute() arguments are bind values, never SQL text: {diags:?}"
    );
}

#[test]
fn fetchrow_methods_are_never_sql_text_sinks() {
    let source = format!(
        "{}\nmy $sth = $dbh->prepare('SELECT * FROM users WHERE id = ?');\n$sth->execute(1);\nmy $id = <STDIN>;\n$sth->fetchrow_array(\"$id\");\n$sth->fetchrow_hashref(\"$id\");\n$sth->fetchall_arrayref(\"$id\");\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(pl607(&diags).is_none(), "fetchrow/fetchall arguments are not SQL text: {diags:?}");
}

#[test]
fn non_dbi_receiver_prepare_stays_silent() {
    let source = concat!(
        "package Engine;\n",
        "sub new { return bless {}, shift }\n",
        "sub prepare { return 1 }\n",
        "package main;\n",
        "my $engine = Engine->new;\n",
        "my $name = <STDIN>;\n",
        "my $q = $engine->prepare(\"SELECT $name\");\n",
    );
    let diags = sql_diags(source);
    assert!(
        pl607(&diags).is_none(),
        "non-DBI receiver prepare must not produce a DBI warning: {diags:?}"
    );
}

#[test]
fn unassigned_receiver_prepare_stays_silent() {
    let source = "my $sth = $dbh->prepare(\"SELECT * FROM users WHERE id = $user_id\");\n";
    let diags = sql_diags(source);
    assert!(pl607(&diags).is_none(), "unproven receiver must not produce a DBI warning: {diags:?}");
}

#[test]
fn computed_sql_variable_is_a_dynamic_boundary_and_stays_silent() {
    let source =
        format!("{}\nmy $sql = build_query();\nmy $sth = $dbh->prepare($sql);\n", dbh_connect());
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_none(),
        "a computed SQL string is a typed dynamic boundary, never a guess: {diags:?}"
    );
}

#[test]
fn mixed_placeholder_and_interpolation_still_fires() {
    let source = format!(
        "{}\nmy $col = <STDIN>;\nmy $sth = $dbh->prepare(\"SELECT * FROM t WHERE a = ? AND b = $col\");\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_some(),
        "a placeholder does not sanitize an interpolated variable in the same statement: {diags:?}"
    );
}

#[test]
fn escaped_sigil_in_sql_string_stays_silent() {
    let source =
        format!("{}\nmy $sth = $dbh->prepare(\"SELECT '5\\$' WHERE x = ?\");\n", dbh_connect());
    let diags = sql_diags(&source);
    assert!(pl607(&diags).is_none(), "an escaped sigil is not an interpolation: {diags:?}");
}

#[test]
fn assignment_form_receiver_evidence_is_accepted() {
    let source = concat!(
        "my $dbh;\n",
        "$dbh = DBI->connect('dbi:SQLite:dbname=x');\n",
        "my $id = <STDIN>;\n",
        "$dbh->do(\"DELETE FROM t WHERE id = $id\");\n",
    );
    let diags = sql_diags(source);
    assert!(
        pl607(&diags).is_some(),
        "plain assignment from DBI->connect is valid receiver evidence: {diags:?}"
    );
}

#[test]
fn even_backslash_run_still_interpolates_and_is_flagged() {
    let statement = r#"my $sth = $dbh->prepare("SELECT * FROM t WHERE x = \\$id");"#;
    let source = format!("{}\nmy $id = <STDIN>;\n{}\n", dbh_connect(), statement);
    let diags = sql_diags(&source);
    assert!(pl607(&diags).is_some(), "an even backslash run does not escape the sigil: {diags:?}");
}

#[test]
fn odd_backslash_run_escapes_the_sigil_and_stays_silent() {
    let statement = r#"my $sth = $dbh->prepare("SELECT * FROM t WHERE x = \$id");"#;
    let source = format!("{}\n{}\n", dbh_connect(), statement);
    let diags = sql_diags(&source);
    assert!(pl607(&diags).is_none(), "an odd backslash run escapes the sigil: {diags:?}");
}

#[test]
fn shadowed_non_dbi_rebinding_suppresses_the_warning() {
    let source = concat!(
        "my $dbh = DBI->connect('dbi:SQLite:dbname=x');\n",
        "{\n",
        "my $dbh = Engine->new;\n",
        "my $id = <STDIN>;\n",
        "my $q = $dbh->prepare(\"SELECT * FROM t WHERE id = $id\");\n",
        "}\n",
    );
    let diags = sql_diags(source);
    assert!(
        pl607(&diags).is_none(),
        "a shadowed non-DBI rebinding must not receive PL607: {diags:?}"
    );
}

#[test]
fn connect_introduced_after_the_sink_stays_silent() {
    let source = concat!(
        "my $id = <STDIN>;\n",
        "my $sth = $dbh->prepare(\"SELECT * FROM t WHERE id = $id\");\n",
        "my $dbh = DBI->connect('dbi:SQLite:dbname=x');\n",
    );
    let diags = sql_diags(source);
    assert!(
        pl607(&diags).is_none(),
        "a connect after the sink must not retroactively classify it: {diags:?}"
    );
}

#[test]
fn interpolating_heredoc_sql_is_flagged() {
    let source = format!(
        "{}\nmy $id = <STDIN>;\nmy $sth = $dbh->prepare(<<END_SQL);\nSELECT * FROM t WHERE id = $id\nEND_SQL\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_some(),
        "an interpolating heredoc with a sigil must be flagged: {diags:?}"
    );
}

#[test]
fn literal_heredoc_sql_is_silent() {
    let source = format!(
        "{}\nmy $sth = $dbh->prepare(<<'END_SQL');\nSELECT * FROM t WHERE id = $id\nEND_SQL\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(pl607(&diags).is_none(), "a literal heredoc is static SQL: {diags:?}");
}

#[test]
fn ampersand_match_variable_sql_is_flagged_with_exact_range() {
    let source = format!(
        "{}\nmy $raw = <STDIN>;\n$raw =~ /(\\w+)/;\nmy $sth = $dbh->prepare(\"SELECT * FROM t WHERE name = $&\");\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    let diagnostic = must_some_with(
        pl607(&diags),
        format!("`$&` match text in SQL must be flagged as PL607: {diags:?}"),
    );
    let (start, end) = diagnostic.range;
    assert_eq!(
        &source[start..end],
        r#"$dbh->prepare("SELECT * FROM t WHERE name = $&")"#,
        "PL607 byte range must cover the exact prepare call"
    );
}

#[test]
fn pre_match_variable_sql_is_flagged_with_exact_range() {
    let source = format!(
        "{}\nmy $raw = <STDIN>;\n$raw =~ /(\\w+)/;\nmy $sth = $dbh->prepare(\"SELECT * FROM t WHERE name = $`\");\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    let diagnostic = must_some_with(
        pl607(&diags),
        format!("`` $` `` pre-match text in SQL must be flagged as PL607: {diags:?}"),
    );
    let (start, end) = diagnostic.range;
    assert_eq!(
        &source[start..end],
        r#"$dbh->prepare("SELECT * FROM t WHERE name = $`")"#,
        "PL607 byte range must cover the exact prepare call"
    );
}

#[test]
fn post_match_variable_sql_is_flagged_with_exact_range() {
    let source = format!(
        "{}\nmy $raw = <STDIN>;\n$raw =~ /(\\w+)/;\nmy $sth = $dbh->prepare(\"SELECT * FROM t WHERE name = $'\");\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    let diagnostic = must_some_with(
        pl607(&diags),
        format!("`$'` post-match text in SQL must be flagged as PL607: {diags:?}"),
    );
    let (start, end) = diagnostic.range;
    assert_eq!(
        &source[start..end],
        r#"$dbh->prepare("SELECT * FROM t WHERE name = $'")"#,
        "PL607 byte range must cover the exact prepare call"
    );
}

#[test]
fn highest_capture_group_variable_sql_is_flagged_with_exact_range() {
    let source = format!(
        "{}\nmy $raw = <STDIN>;\n$raw =~ /(\\w+)/;\nmy $sth = $dbh->prepare(\"SELECT * FROM t WHERE name = $+\");\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    let diagnostic = must_some_with(
        pl607(&diags),
        format!("`$+` capture text in SQL must be flagged as PL607: {diags:?}"),
    );
    let (start, end) = diagnostic.range;
    assert_eq!(
        &source[start..end],
        r#"$dbh->prepare("SELECT * FROM t WHERE name = $+")"#,
        "PL607 byte range must cover the exact prepare call"
    );
}

#[test]
fn escaped_punctuation_match_variable_stays_silent() {
    let source = format!(
        "{}\nmy $sth = $dbh->prepare(\"SELECT * FROM t WHERE x = '\\$&' AND y = ?\");\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_none(),
        "an escaped punctuation match variable is not an interpolation: {diags:?}"
    );
}

#[test]
fn receiver_evidence_index_keeps_classification_identical_across_sink_counts() {
    for sinks in [1usize, 10, 100, 500] {
        let mut proven = String::new();
        let mut rebound_silent = String::new();
        for i in 0..sinks {
            proven.push_str(&format!(
                "my $dbh{i} = DBI->connect('dbi:SQLite:dbname=x');\nmy $id{i} = <STDIN>;\n$dbh{i}->do(\"DELETE FROM t{i} WHERE id = $id{i}\");\n"
            ));
            rebound_silent.push_str(&format!(
                "my $eng{i} = Engine->new;\nmy $id{i} = <STDIN>;\n$eng{i}->do(\"DELETE FROM t{i} WHERE id = $id{i}\");\n"
            ));
        }
        assert_eq!(
            pl607_count(&sql_diags(&proven)),
            sinks,
            "every proven receiver fires once at {sinks} sinks"
        );
        assert_eq!(
            pl607_count(&sql_diags(&rebound_silent)),
            0,
            "non-connect receivers stay silent at {sinks} sinks"
        );

        let mut shared = String::new();
        for _ in 0..sinks {
            shared.push_str("$dbh = DBI->connect('dbi:SQLite:dbname=x');\n");
        }
        shared.push_str("my $id = <STDIN>;\n");
        for _ in 0..sinks {
            shared.push_str("$dbh->do(\"DELETE FROM t WHERE id = $id\");\n");
        }
        assert_eq!(
            pl607_count(&sql_diags(&shared)),
            sinks,
            "all-connect shared name keeps every sink proven at {sinks} assignments/sinks"
        );

        let mut rebound = String::new();
        rebound.push_str("my $dbh = DBI->connect('dbi:SQLite:dbname=x');\nmy $id = <STDIN>;\n");
        for _ in 0..sinks {
            rebound.push_str("$dbh->do(\"DELETE FROM a WHERE id = $id\");\n");
        }
        rebound.push_str("$dbh = Engine->new;\n");
        for _ in 0..sinks {
            rebound.push_str("$dbh->do(\"DELETE FROM b WHERE id = $id\");\n");
        }
        assert_eq!(
            pl607_count(&sql_diags(&rebound)),
            sinks,
            "only pre-rebinding sinks fire at {sinks} sinks"
        );
    }
}

#[test]
fn first_cohort_sql_text_sinks_are_exactly_the_reviewed_set() {
    assert_eq!(
        DBI_SQL_TEXT_SINKS,
        &[
            "prepare",
            "prepare_cached",
            "do",
            "selectrow_array",
            "selectrow_arrayref",
            "selectrow_hashref",
            "selectall_arrayref",
            "selectall_hashref",
            "selectcol_arrayref",
        ]
    );
    for method in ["execute", "fetchrow_array", "fetchrow_arrayref", "quote", "connect"] {
        assert!(!is_dbi_sql_text_sink(method), "{method} is not an SQL-text sink");
    }
}

#[test]
fn first_cohort_sinks_flag_interpolated_sql_at_argument_zero() {
    for &method in DBI_SQL_TEXT_SINKS {
        let call = if method == "selectall_hashref" {
            format!("$dbh->{method}(\"SELECT * FROM t WHERE id = $id\", 'id');")
        } else {
            format!("$dbh->{method}(\"SELECT * FROM t WHERE id = $id\");")
        };
        let source = format!("{}\nmy $id = <STDIN>;\n{call}\n", dbh_connect());
        let diags = sql_diags(&source);
        assert!(
            pl607(&diags).is_some(),
            "interpolated {method} must be flagged as PL607: {diags:?}"
        );
    }
}

#[test]
fn first_cohort_sinks_stay_silent_for_placeholder_sql() {
    for &method in DBI_SQL_TEXT_SINKS {
        let call = if method == "selectall_hashref" {
            format!("$dbh->{method}('SELECT * FROM t WHERE id = ?', 'id', undef, $id);")
        } else if matches!(method, "prepare" | "prepare_cached") {
            format!("$dbh->{method}('SELECT * FROM t WHERE id = ?');")
        } else {
            format!("$dbh->{method}('SELECT * FROM t WHERE id = ?', undef, $id);")
        };
        let source = format!("{}\nmy $id = <STDIN>;\n{call}\n", dbh_connect());
        let diags = sql_diags(&source);
        assert!(
            pl607(&diags).is_none(),
            "placeholder {method} with bind values must stay silent: {diags:?}"
        );
    }
}

#[test]
fn same_named_selectrow_on_unrelated_class_stays_silent() {
    let source = concat!(
        "package Engine;\n",
        "sub new { return bless {}, shift }\n",
        "sub selectrow_array { return 1 }\n",
        "package main;\n",
        "my $engine = Engine->new;\n",
        "my $id = <STDIN>;\n",
        "my $row = $engine->selectrow_array(\"SELECT $id\");\n",
    );
    let diags = sql_diags(source);
    assert!(
        pl607(&diags).is_none(),
        "same-named non-DBI selectrow_array must not inherit the sink: {diags:?}"
    );
}

#[test]
fn eval_error_interpolation_in_sql_is_flagged() {
    let source = format!(
        "{}\neval {{ die 'x' }};\nmy $sth = $dbh->prepare(\"SELECT * FROM t WHERE err = $@\");\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_some(),
        "`$@` interpolates in double-quoted SQL and must be flagged: {diags:?}"
    );
}

#[test]
fn dollar_followed_by_space_is_not_interpolation() {
    let source = format!(
        "{}\nmy $sth = $dbh->prepare(\"SELECT * FROM t WHERE label = 'cost is $ each'\");\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_none(),
        "`$` followed by whitespace is not an interpolating scalar: {diags:?}"
    );
}

#[test]
fn qq_interpolating_sql_is_flagged_and_q_literal_is_silent() {
    let flagged = format!(
        "{}\nmy $id = <STDIN>;\nmy $sth = $dbh->prepare(qq{{SELECT * FROM t WHERE id = $id}});\n",
        dbh_connect()
    );
    let diags = sql_diags(&flagged);
    assert!(pl607(&diags).is_some(), "qq{{}} interpolates like double quotes: {diags:?}");

    let safe = format!(
        "{}\nmy $sth = $dbh->prepare(q{{SELECT * FROM t WHERE id = $id}});\n",
        dbh_connect()
    );
    let diags = sql_diags(&safe);
    assert!(
        pl607(&diags).is_none(),
        "q{{}} is a non-interpolating literal even with `$id` text: {diags:?}"
    );
}

#[test]
fn comment_and_string_text_that_looks_like_prepare_stays_silent() {
    let source = format!(
        "{}\n# $dbh->prepare(\"SELECT * FROM t WHERE id = $id\");\nmy $s = '$dbh->prepare(\"SELECT $id\")';\nmy $sth = $dbh->prepare('SELECT * FROM t WHERE id = ?');\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_none(),
        "comments and string literals are not SQL-text sinks: {diags:?}"
    );
}

#[test]
fn sql_abstract_and_dbix_class_calls_stay_silent() {
    let source = concat!(
        "my $dbh = DBI->connect('dbi:SQLite:dbname=x');\n",
        "my $sql = SQL::Abstract->new;\n",
        "my ($stmt, @bind) = $sql->select('users', '*', { name => $name });\n",
        "my $sth = $dbh->prepare($stmt);\n",
        "my $rs = $schema->resultset('User')->search({ name => $name });\n",
    );
    let diags = sql_diags(source);
    assert!(
        pl607(&diags).is_none(),
        "SQL::Abstract / DBIx::Class remain a typed dynamic boundary: {diags:?}"
    );
}

#[test]
fn static_sql_stored_in_a_variable_stays_silent() {
    let source = format!(
        "{}\nmy $sql = 'SELECT * FROM t WHERE id = ?';\nmy $sth = $dbh->prepare($sql);\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_none(),
        "a SQL variable is a typed boundary without canonical value facts: {diags:?}"
    );
}

#[test]
fn concatenation_stored_in_a_variable_stays_silent_without_value_facts() {
    let source = format!(
        "{}\nmy $name = <STDIN>;\nmy $sql = 'SELECT * FROM t WHERE name = ' . $name;\nmy $sth = $dbh->prepare($sql);\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_none(),
        "composition through a local binding stays unknown without value facts: {diags:?}"
    );
}

#[test]
fn ternary_with_every_branch_composed_is_flagged() {
    let source = format!(
        "{}\nmy $id = <STDIN>;\nmy $name = <STDIN>;\nmy $sth = $dbh->prepare($ok ? \"SELECT $id\" : \"SELECT $name\");\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_some(),
        "finite ternary whose every branch interpolates must be flagged: {diags:?}"
    );
}

#[test]
fn ternary_with_one_composed_branch_is_flagged() {
    let source = format!(
        "{}\nmy $id = <STDIN>;\nmy $sth = $dbh->prepare($ok ? \"SELECT $id\" : 'SELECT 1');\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_some(),
        "a composed ternary branch is admitted composition: {diags:?}"
    );
}

#[test]
fn ternary_with_only_static_branches_is_silent() {
    let source =
        format!("{}\nmy $sth = $dbh->prepare($ok ? 'SELECT 1' : 'SELECT 2');\n", dbh_connect());
    let diags = sql_diags(&source);
    assert!(pl607(&diags).is_none(), "literal-only ternary branches are static SQL: {diags:?}");
}

#[test]
fn ternary_with_computed_branches_stays_a_dynamic_boundary() {
    let source = format!("{}\nmy $sth = $dbh->prepare($ok ? $sql_a : $sql_b);\n", dbh_connect());
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_none(),
        "computed ternary branches are a typed dynamic boundary: {diags:?}"
    );
}

#[test]
fn missing_statement_argument_stays_silent() {
    let source = format!("{}\nmy $sth = $dbh->prepare();\n", dbh_connect());
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_none(),
        "a prepare call with no SQL argument cannot be an exact finding: {diags:?}"
    );
}

#[test]
fn parenthesized_interpolated_prepare_is_flagged() {
    let source = format!(
        "{}\nmy $id = <STDIN>;\nmy $sth = $dbh->prepare((\"SELECT * FROM t WHERE id = $id\"));\n",
        dbh_connect()
    );
    let diags = sql_diags(&source);
    assert!(
        pl607(&diags).is_some(),
        "ArrayLiteral-wrapped args must still see the SQL text: {diags:?}"
    );
}
