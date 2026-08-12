use tree_sitter_perl_c::{parse_perl_code, try_create_parser};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn bare_source(label: &str, false_terminator: &str) -> String {
    format!(
        "my $value = <<{label};\nfirst line\n{false_terminator}\nstill heredoc\n{label}\nprint $value;\n"
    )
}

fn quoted_source(label: &str, false_terminator: &str) -> String {
    format!(
        "my $value = <<\"{label}\";\nfirst line\n{false_terminator}\nstill heredoc\n{label}\nprint $value;\n"
    )
}

fn assert_clean(source: &str) -> TestResult {
    let tree = parse_perl_code(source)?;
    assert!(
        !tree.root_node().has_error(),
        "expected a clean parse, got {}",
        tree.root_node().to_sexp()
    );
    Ok(())
}

#[test]
fn same_length_delimiters_that_diverge_after_eight_bytes_do_not_alias() -> TestResult {
    assert_clean(&bare_source("ABCDEFGH1", "ABCDEFGH2"))
}

#[test]
fn same_length_delimiters_that_diverge_after_sixteen_bytes_do_not_alias() -> TestResult {
    assert_clean(&bare_source("ABCDEFGHIJKLMNOP1", "ABCDEFGHIJKLMNOP2"))
}

#[test]
fn exact_matching_holds_at_the_sixty_four_byte_envelope() -> TestResult {
    let prefix = "A".repeat(63);
    let label = format!("{prefix}1");
    let false_terminator = format!("{prefix}2");
    assert_eq!(label.len(), 64);
    assert_clean(&bare_source(&label, &false_terminator))
}

#[test]
fn unicode_delimiters_compare_their_complete_utf8_bytes() -> TestResult {
    let label = "ééééééééA";
    let false_terminator = "ééééééééB";
    assert!(label.len() > 8);
    assert_clean(&quoted_source(label, false_terminator))
}

#[test]
fn delimiters_beyond_the_exact_envelope_fail_closed() -> TestResult {
    let label = "A".repeat(65);
    let source = bare_source(&label, &label);
    let tree = parse_perl_code(&source)?;
    assert!(
        tree.root_node().has_error(),
        "an unrepresentable delimiter must not be accepted as exact: {}",
        tree.root_node().to_sexp()
    );
    Ok(())
}

#[test]
fn reused_tree_preserves_long_delimiter_identity() -> TestResult {
    let source = bare_source("ABCDEFGHIJKLMNOP1", "ABCDEFGHIJKLMNOP2");
    let mut parser = try_create_parser()?;
    let first = parser.parse(&source, None).ok_or("tree-sitter returned no initial tree")?;
    assert!(!first.root_node().has_error());

    let reused =
        parser.parse(&source, Some(&first)).ok_or("tree-sitter returned no reused tree")?;
    assert!(!reused.root_node().has_error());
    assert_eq!(first.root_node().to_sexp(), reused.root_node().to_sexp());
    Ok(())
}
