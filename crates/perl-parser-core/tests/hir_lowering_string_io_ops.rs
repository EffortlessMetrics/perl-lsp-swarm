//! HIR lowering tests for the Wave 4 string/IO ops (issue #2210).
//!
//! Pins the flat-HIR migration adapters for `NodeKind::Heredoc`, `Readline`,
//! `Diamond`, and `Glob`. Before this slice each of those AST kinds fell to the
//! `_ => visit_children` arm in `crates/perl-parser-core/src/hir/lower.rs` and
//! left no HIR item at all. The adapters preserve source-ordered migration
//! visibility, while canonical intra-body semantics live only in body HIR.
//!
//! The claims proved here:
//!
//! - each construct emits exactly one typed migration adapter, anchored on its own AST kind;
//! - the facts that decide whether a value is statically knowable are recorded:
//!   heredoc interpolation/indent/command form, readline line source, and glob
//!   pattern interpolation;
//!   a command heredoc (``<<`CMD` ``) is marked rather than presented as a
//!   literal string;
//! - the canonical body arena carries the semantic facts; the flat adapters are
//!   not presented as a second semantic authority;
//! - the new shells stay visible in the PIR-A receipt's unsupported counts on
//!   both the item and body paths, instead of being silently dropped or
//!   mistaken for exact facts by the layer above.
//!
//! The implementation lives in `crates/perl-parser-core/src/hir/lower.rs` (flat
//! migration adapters) and `crates/perl-parser-core/src/hir/body.rs` (canonical body arena), both
//! classifying through the shared rules in `hir/model.rs`
//! (`ReadlineSource::from_filehandle`, `glob_pattern_interpolates`).

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    GlobMigrationAdapter, HeredocMigrationAdapter, HirBody, HirExpr, HirFile, HirKind,
    ReadlineMigrationAdapter, ReadlineSource,
};
use perl_parser_core::hir::{lower_ast, lower_body};
use perl_parser_core::pir::{lower_hir, lower_hir_bodies};
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

fn heredocs(file: &HirFile) -> Vec<&HeredocMigrationAdapter> {
    file.items
        .iter()
        .filter_map(|item| match &item.kind {
            HirKind::HeredocMigrationAdapter(heredoc) => Some(heredoc),
            _ => None,
        })
        .collect()
}

fn readlines(file: &HirFile) -> Vec<&ReadlineMigrationAdapter> {
    file.items
        .iter()
        .filter_map(|item| match &item.kind {
            HirKind::ReadlineMigrationAdapter(readline) => Some(readline),
            _ => None,
        })
        .collect()
}

fn globs(file: &HirFile) -> Vec<&GlobMigrationAdapter> {
    file.items
        .iter()
        .filter_map(|item| match &item.kind {
            HirKind::GlobMigrationAdapter(glob) => Some(glob),
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
// Glob: a sigil only interpolates when it actually starts a variable
// ---------------------------------------------------------------------------

#[test]
fn glob_sigil_not_starting_a_variable_stays_static() -> TestResult {
    // Perl reads a trailing `@` as a literal character, so the match set of
    // these patterns *is* statically known. Calling them interpolating would
    // report a knowable pattern as unknowable.
    for source in ["my @files = <*.txt@>;\n", "my @files = <foo@>;\n"] {
        let file = lower_source(source);
        let glob = must_some(globs(&file).first().copied());

        assert!(
            !glob.interpolated,
            "a sigil that starts no variable is a literal character, so {:?} is static",
            glob.pattern
        );
    }
    Ok(())
}

#[test]
fn double_diamond_lowers_to_the_same_argv_source_as_single_diamond() -> TestResult {
    // `<<>>` (Perl 5.22+) is the "safer" diamond: it reads @ARGV but refuses
    // magic/pipe filenames. `tests/double_diamond.rs` pins that it parses as
    // `NodeKind::Diamond`; this pins what the lowerers then do with it, on both
    // paths. Without it, a future change giving `<<>>` its own `ReadlineSource`
    // variant would alter the classification with no test noticing.
    let file = lower_source("my @lines = <<>>;\n");
    let readline = must_some(readlines(&file).first().copied());
    assert_eq!(
        readline.source,
        ReadlineSource::ArgvDiamond,
        "`<<>>` reads @ARGV, so it classifies as the diamond source"
    );

    let body = lower_body_source("my @lines = <<>>;\n");
    let exprs = body_exprs(&body);
    assert!(
        exprs.iter().any(|expr| matches!(
            expr,
            HirExpr::Readline { source: ReadlineSource::ArgvDiamond, .. }
        )),
        "body HIR must classify `<<>>` as the @ARGV diamond too, got {exprs:?}"
    );
    Ok(())
}

#[test]
fn glob_unicode_array_patterns_are_recognized_as_interpolating() -> TestResult {
    // `use utf8;` admits Unicode identifiers, and perl 5.38.2 confirms `@é` is
    // an ordinary array that interpolates:
    //
    //     $ perl -e 'use utf8; my @é = ("VAL"); print "<@é>"'   # => <VAL>
    //
    // An ASCII-only name-start test reported these as statically knowable, which
    // is the unsound direction: a consumer would resolve a match set that is
    // actually a runtime property.
    for source in ["use utf8;\nmy @files = <@épattern>;\n", "use utf8;\nmy @files = <@π>;\n"] {
        let file = lower_source(source);
        let glob = must_some(globs(&file).first().copied());

        assert!(
            glob.interpolated,
            "a Unicode array name is still a variable, so {:?} is dynamic",
            glob.pattern
        );
    }
    Ok(())
}

#[test]
fn glob_digit_after_sigil_stays_static() -> TestResult {
    // The companion to the case above: widening the name-start test must not
    // widen it to digits. `@1` is not a variable, so this pattern's match set
    // really is statically known and must not be reported as interpolating.
    let file = lower_source("my @files = <*.txt@1>;\n");
    let glob = must_some(globs(&file).first().copied());

    assert!(!glob.interpolated, "`@1` is not a variable, so {:?} is static", glob.pattern);
    Ok(())
}

#[test]
fn glob_special_variable_patterns_are_recognized_as_interpolating() -> TestResult {
    // Perl's punctuation variables are variables. The parser routes these to
    // `NodeKind::Glob` rather than `Readline` (the pattern is not a bare
    // handle name), so a classifier that only accepted identifier-ish name
    // starts reported them as statically knowable when their value is a
    // runtime property. Claiming a dynamic pattern is static is the unsound
    // direction: a consumer would resolve a match set it cannot know.
    for source in [
        "my @files = <$/foo>;\n",  // input record separator
        "my @files = <$.>;\n",     // current line number
        "my @files = <$!>;\n",     // errno
        "my @files = <$@>;\n",     // eval error
        "my @files = <$$>;\n",     // pid
        "my @files = <$^V>;\n",    // perl version
        "my @files = <$/.txt>;\n", // separator followed by a literal suffix
    ] {
        let file = lower_source(source);
        let glob = must_some(globs(&file).first().copied());

        assert!(
            glob.interpolated,
            "{:?} begins with a Perl special variable, so its match set is a runtime property",
            glob.pattern
        );
    }
    Ok(())
}

#[test]
fn glob_array_and_braced_interpolation_are_recognized() -> TestResult {
    // The opposite control: these really do interpolate, so narrowing the rule
    // must not have made it blind.
    for source in ["my @files = <@list>;\n", "my @files = <${dir}/*.c>;\n"] {
        let file = lower_source(source);
        let glob = must_some(globs(&file).first().copied());

        assert!(
            glob.interpolated,
            "{:?} starts a real variable, so its match set is a runtime property",
            glob.pattern
        );
    }
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
        HirKind::ReadlineMigrationAdapter(_) => Some(item.anchor.node_kind),
        _ => None,
    }));
    assert_eq!(readline_anchor, "Readline", "the readline shell keeps its own AST anchor");

    let glob_anchor = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::GlobMigrationAdapter(_) => Some(item.anchor.node_kind),
        _ => None,
    }));
    assert_eq!(glob_anchor, "Glob", "the glob shell keeps its own AST anchor");
    Ok(())
}

#[test]
fn body_hir_owns_string_io_semantics_while_pir_remains_fail_closed() -> TestResult {
    const SOURCE: &str =
        "my $text = <<\"EOF\";\nhello $name\nEOF\nmy $line = <>;\nmy @files = <$dir/*.txt>;\n";

    // Half one — the body arena carries the same facts as the item shells, so a
    // consumer reading canonical body HIR is not told less than one reading items.
    let body = lower_body_source(SOURCE);
    let exprs = body_exprs(&body);

    assert!(
        exprs.iter().any(|expr| matches!(
            expr,
            HirExpr::Heredoc { interpolated: true, command: false, .. }
        )),
        "body HIR must carry the heredoc interpolation and command facts, got {exprs:?}"
    );
    assert!(
        exprs.iter().any(|expr| matches!(
            expr,
            HirExpr::Readline { source: ReadlineSource::ArgvDiamond, .. }
        )),
        "body HIR must classify `<>` as the @ARGV diamond, got {exprs:?}"
    );
    assert!(
        exprs.iter().any(|expr| matches!(expr, HirExpr::Glob { interpolated: true, .. })),
        "body HIR must record that `<$dir/*.txt>` interpolates, got {exprs:?}"
    );

    // Half two — the name's second claim. A typed body node must not be read as
    // PIR-A support: the body path counts these as unsupported constructs rather
    // than emitting exact operations for runtime IO.
    let graph = lower_hir_bodies(&lower_source(SOURCE));
    for construct in ["Heredoc", "Readline", "Glob"] {
        assert!(
            graph.receipt.unsupported_construct_counts.contains_key(construct),
            "PIR-A must stay fail-closed on {construct}, counting it as unsupported \
             rather than claiming an exact fact: {:?}",
            graph.receipt.unsupported_construct_counts
        );
    }
    Ok(())
}

#[test]
fn diamond_shell_keeps_its_own_ast_anchor() -> TestResult {
    let file = lower_source("while (my $line = <>) { }\n");

    let anchor = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::ReadlineMigrationAdapter(_) => Some(item.anchor.node_kind),
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
        graph.receipt.unsupported_construct_counts.get("ReadlineMigrationAdapter"),
        Some(&1),
        "ReadlineMigrationAdapter must be counted, not dropped: {:?}",
        graph.receipt.unsupported_construct_counts
    );
    assert_eq!(
        graph.receipt.unsupported_construct_counts.get("GlobMigrationAdapter"),
        Some(&1),
        "GlobMigrationAdapter must be counted, not dropped: {:?}",
        graph.receipt.unsupported_construct_counts
    );
    Ok(())
}
