//! HIR lowering tests for the Wave 4 string/IO ops (issue #2210).
//!
//! Pins the item-level HIR shells for `NodeKind::Heredoc`, `Readline`,
//! `Diamond`, and `Glob`. Before this slice each of those AST kinds fell to the
//! `_ => visit_children` arm in `crates/perl-parser-core/src/hir/lower.rs` and
//! left no HIR item at all, so a heredoc body, a filehandle read, and a file
//! glob were indistinguishable from empty source at the compiler-substrate
//! layer.
//!
//! The claims proved here:
//!
//! - each construct emits exactly one typed shell, anchored on its own AST kind;
//! - the facts that decide whether a value is statically knowable are recorded:
//!   heredoc interpolation/indent/command form, readline line source, and glob
//!   pattern interpolation;
//!   a command heredoc (``<<`CMD` ``) is marked rather than presented as a
//!   literal string;
//! - the new shells stay visible in the PIR-A receipt's unsupported counts
//!   instead of being silently dropped by the layer above.
//!
//! The implementation lives in `crates/perl-parser-core/src/hir/lower.rs` (the
//! `NodeKind::Heredoc`/`Readline`/`Diamond`/`Glob` arms).

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    GlobExpr, HeredocExpr, HirBody, HirExpr, HirFile, HirKind, ReadlineExpr, ReadlineSource,
};
use perl_parser_core::hir::{lower_ast, lower_body};
use perl_parser_core::pir::lower_hir;
use perl_tdd_support::must_some;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn lower_source(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

fn lower_body_source(source: &str) -> HirBody {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_body(&output.ast)
}

fn body_exprs(body: &HirBody) -> Vec<&HirExpr> {
    body.exprs.iter().collect()
}

fn heredocs(file: &HirFile) -> Vec<&HeredocExpr> {
    file.items
        .iter()
        .filter_map(|item| match &item.kind {
            HirKind::HeredocExpr(heredoc) => Some(heredoc),
            _ => None,
        })
        .collect()
}

fn readlines(file: &HirFile) -> Vec<&ReadlineExpr> {
    file.items
        .iter()
        .filter_map(|item| match &item.kind {
            HirKind::ReadlineExpr(readline) => Some(readline),
            _ => None,
        })
        .collect()
}

fn globs(file: &HirFile) -> Vec<&GlobExpr> {
    file.items
        .iter()
        .filter_map(|item| match &item.kind {
            HirKind::GlobExpr(glob) => Some(glob),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Readline: named filehandle
// ---------------------------------------------------------------------------

#[test]
fn named_filehandle_readline_lowers_to_named_handle_shell() -> TestResult {
    let file = lower_source("my $line = <STDIN>;\n");
    let readline = must_some(readlines(&file).first().copied());

    assert_eq!(readline.source, ReadlineSource::NamedHandle);
    assert_eq!(readline.filehandle.as_deref(), Some("STDIN"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Readline: scalar filehandle
// ---------------------------------------------------------------------------

#[test]
fn scalar_filehandle_readline_is_distinguished_from_a_bareword_handle() -> TestResult {
    let file = lower_source("my $line = <$fh>;\n");
    let readline = must_some(readlines(&file).first().copied());

    assert_eq!(
        readline.source,
        ReadlineSource::ScalarHandle,
        "`<$fh>` reads through a scalar holding the handle, not a bareword handle"
    );
    assert_eq!(readline.filehandle.as_deref(), Some("$fh"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Diamond: `<>` reads @ARGV, not a named handle
// ---------------------------------------------------------------------------

#[test]
fn diamond_lowers_to_an_argv_readline_shell_with_no_filehandle() -> TestResult {
    let file = lower_source("while (my $line = <>) { }\n");
    let readline = must_some(readlines(&file).first().copied());

    assert_eq!(
        readline.source,
        ReadlineSource::ArgvDiamond,
        "`<>` reads the files named in @ARGV (or STDIN), so it has no static handle"
    );
    assert_eq!(readline.filehandle, None);
    Ok(())
}

// ---------------------------------------------------------------------------
// Glob: static pattern
// ---------------------------------------------------------------------------

#[test]
fn static_glob_pattern_lowers_without_an_interpolation_fact() -> TestResult {
    let file = lower_source("my @files = <*.txt>;\n");
    let glob = must_some(globs(&file).first().copied());

    assert!(
        glob.pattern.contains("*.txt"),
        "glob pattern should be preserved, got {:?}",
        glob.pattern
    );
    assert!(!glob.interpolated, "a literal pattern has a statically knowable shape");
    Ok(())
}

// ---------------------------------------------------------------------------
// Glob: interpolating pattern is not statically knowable
// ---------------------------------------------------------------------------

#[test]
fn interpolating_glob_pattern_records_the_interpolation_fact() -> TestResult {
    let file = lower_source("my @files = <$dir/*.txt>;\n");
    let glob = must_some(globs(&file).first().copied());

    assert!(
        glob.interpolated,
        "`<$dir/*.txt>` interpolates, so the match set is a runtime property; got {:?}",
        glob.pattern
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Heredoc: interpolating body
// ---------------------------------------------------------------------------

#[test]
fn interpolating_heredoc_lowers_with_interpolation_recorded() -> TestResult {
    let file = lower_source("my $text = <<\"EOF\";\nhello $name\nEOF\n");
    let heredoc = must_some(heredocs(&file).first().copied());

    assert!(
        heredoc.delimiter.contains("EOF"),
        "delimiter should be preserved, got {:?}",
        heredoc.delimiter
    );
    assert!(heredoc.interpolated, "`<<\"EOF\"` interpolates");
    assert!(!heredoc.indented, "`<<\"EOF\"` is not the indented form");
    assert!(!heredoc.command, "`<<\"EOF\"` does not run a command");
    Ok(())
}

// ---------------------------------------------------------------------------
// Heredoc: the body is addressable by range instead of copied into HIR
// ---------------------------------------------------------------------------

#[test]
fn heredoc_body_range_addresses_the_body_text_in_the_source() -> TestResult {
    let source = "my $text = <<\"EOF\";\nhello\nEOF\n";
    let file = lower_source(source);
    let heredoc = must_some(heredocs(&file).first().copied());
    let body_range = must_some(heredoc.body_range);

    let body = must_some(source.get(body_range.start as usize..body_range.end as usize));
    assert_eq!(
        body, "hello",
        "body_range must address the heredoc body so consumers read it from source \
         instead of holding a second copy"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Heredoc: single-quoted body does not interpolate
// ---------------------------------------------------------------------------

#[test]
fn single_quoted_heredoc_is_not_interpolating() -> TestResult {
    let file = lower_source("my $text = <<'EOF';\nliteral $name\nEOF\n");
    let heredoc = must_some(heredocs(&file).first().copied());

    assert!(!heredoc.interpolated, "`<<'EOF'` is a literal body");
    Ok(())
}

// ---------------------------------------------------------------------------
// Heredoc: indented form
// ---------------------------------------------------------------------------

#[test]
fn indented_heredoc_records_the_indent_form() -> TestResult {
    let file = lower_source("my $text = <<~EOF;\n    indented\n    EOF\n");
    let heredoc = must_some(heredocs(&file).first().copied());

    assert!(heredoc.indented, "`<<~EOF` strips leading indentation");
    Ok(())
}

// ---------------------------------------------------------------------------
// Heredoc: command form is a runtime effect, not a literal
// ---------------------------------------------------------------------------

#[test]
fn command_heredoc_is_marked_rather_than_treated_as_a_literal() -> TestResult {
    let file = lower_source("my $out = <<`CMD`;\nls -l\nCMD\n");
    let heredoc = must_some(heredocs(&file).first().copied());

    assert!(
        heredoc.command,
        "``<<`CMD` `` runs its body through the shell, so consumers must not \
         treat the body as a known string value"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// One shell per construct, anchored on its own AST kind
// ---------------------------------------------------------------------------

#[test]
fn each_string_io_construct_emits_exactly_one_anchored_shell() -> TestResult {
    let file = lower_source("my $line = <STDIN>;\nmy @files = <*.txt>;\n");

    assert_eq!(readlines(&file).len(), 1, "one readline construct, one readline shell");
    assert_eq!(globs(&file).len(), 1, "one glob construct, one glob shell");

    let readline_anchor = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::ReadlineExpr(_) => Some(item.anchor.node_kind),
        _ => None,
    }));
    assert_eq!(readline_anchor, "Readline", "the readline shell keeps its own AST anchor");

    let glob_anchor = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::GlobExpr(_) => Some(item.anchor.node_kind),
        _ => None,
    }));
    assert_eq!(glob_anchor, "Glob", "the glob shell keeps its own AST anchor");
    Ok(())
}

#[test]
fn body_hir_owns_string_io_semantics_while_pir_remains_fail_closed() -> TestResult {
    let body = lower_body_source(
        "my $text = <<\"EOF\";\nhello $name\nEOF\nmy $line = <>;\nmy @files = <$dir/*.txt>;\n",
    );
    let exprs = body_exprs(&body);

    assert!(
        exprs.iter().any(|expr| matches!(
            expr,
            HirExpr::Heredoc { interpolated: true, command: false, .. }
        ))
    );
    assert!(
        exprs.iter().any(|expr| matches!(
            expr,
            HirExpr::Readline { source: ReadlineSource::ArgvDiamond, .. }
        ))
    );
    assert!(exprs.iter().any(|expr| matches!(expr, HirExpr::Glob { interpolated: true, .. })));
    Ok(())
}

#[test]
fn diamond_shell_keeps_its_own_ast_anchor() -> TestResult {
    let file = lower_source("while (my $line = <>) { }\n");

    let anchor = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::ReadlineExpr(_) => Some(item.anchor.node_kind),
        _ => None,
    }));
    assert_eq!(
        anchor, "Diamond",
        "`<>` lowers to a readline shell but stays distinguishable from `<STDIN>` by anchor"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// No silent loss at the PIR-A seam
// ---------------------------------------------------------------------------

#[test]
fn string_io_shells_stay_visible_in_the_pir_receipt() -> TestResult {
    let file = lower_source("my $line = <STDIN>;\nmy @files = <*.txt>;\n");
    let graph = lower_hir(&file);

    // PIR-A v0 does not lower these families yet. The contract is that they are
    // counted as unsupported constructs rather than disappearing between layers.
    assert_eq!(
        graph.receipt.unsupported_construct_counts.get("ReadlineExpr"),
        Some(&1),
        "ReadlineExpr must be counted, not dropped: {:?}",
        graph.receipt.unsupported_construct_counts
    );
    assert_eq!(
        graph.receipt.unsupported_construct_counts.get("GlobExpr"),
        Some(&1),
        "GlobExpr must be counted, not dropped: {:?}",
        graph.receipt.unsupported_construct_counts
    );
    Ok(())
}
