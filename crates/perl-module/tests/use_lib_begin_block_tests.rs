use std::path::Path;

use perl_module::{
    UseLibAction, UseLibPath, extract_use_lib_operations, extract_use_lib_operations_with_offsets,
    no_lib_cancelled_paths_at_offset, resolve_use_lib_paths_from_source_at_offset,
};

#[test]
fn begin_block_leading_use_lib_is_active_for_following_use() {
    let source = "BEGIN {\n    use lib 'local/lib';\n    use Local::Thing;\n}\n";
    let offset = source.find("use Local::Thing;").unwrap_or(source.len());

    let paths =
        resolve_use_lib_paths_from_source_at_offset(source, offset, Path::new("/workspace"), None);

    assert_eq!(paths, vec!["local/lib".to_string()]);
}

#[test]
fn begin_block_comments_before_pragma_preserve_ordered_qw_paths() {
    let source = "BEGIN # compile-time include setup\n{\n    # Keep local modules first.\n    use lib qw(local/lib vendor/lib);\n    use Local::Thing;\n}\n";

    let operations = extract_use_lib_operations(source);

    assert_eq!(
        operations,
        vec![UseLibAction::Add(vec![
            UseLibPath { path: "local/lib".to_string(), from_findbin: false },
            UseLibPath { path: "vendor/lib".to_string(), from_findbin: false },
        ])]
    );
}

#[test]
fn begin_block_leading_no_lib_cancels_path_for_following_use() {
    let source = "BEGIN {\n    no lib 'local/lib';\n    use Local::Thing;\n}\n";
    let offset = source.find("use Local::Thing;").unwrap_or(source.len());

    let cancelled = no_lib_cancelled_paths_at_offset(source, offset, Path::new("/workspace"), None);

    assert_eq!(cancelled, vec!["local/lib".to_string()]);
}

#[test]
fn begin_block_unterminated_use_lib_remains_active_while_editing() {
    let source = "BEGIN {\n    use lib 'local/lib'\n    use Local::Thing;\n}\n";
    let offset = source.find("use Local::Thing;").unwrap_or(source.len());

    let paths =
        resolve_use_lib_paths_from_source_at_offset(source, offset, Path::new("/workspace"), None);

    assert_eq!(paths, vec!["local/lib".to_string()]);
}

#[test]
fn begin_block_use_lib_preserves_inner_statement_end_offset() {
    let source = "BEGIN { use lib 'x'; }\n";
    let operations = extract_use_lib_operations_with_offsets(source);

    assert_eq!(operations.len(), 1);
    let inner_statement_end = source.find(';').map_or(0, |offset| offset + 1);
    assert_eq!(operations[0].end_offset, inner_statement_end);
}

#[test]
fn begin_nested_block_does_not_unwrap_inner_pragma() {
    let source = "BEGIN { BEGIN { use lib 'x'; } }\n";

    assert!(extract_use_lib_operations(source).is_empty());
}

#[test]
fn heredoc_bodies_do_not_create_lib_operations() {
    let sources = [
        "my $s = <<'EOF';\nBEGIN { use lib 'phantom'; }\nEOF\n",
        "my $s = <<\"EOF\";\nBEGIN { use lib 'phantom'; }\nEOF\n",
        "my $out = <<`CMD`;\nBEGIN { use lib 'phantom'; }\nCMD\n",
        "my $s = <<~'EOF';\n    BEGIN { use lib 'phantom'; }\n    EOF\n",
        "my $s = <<EOF;\nBEGIN { use lib 'phantom'; }\nEOF\n",
        "my $a = <<A; my $b = <<B;\nBEGIN { use lib 'phantom'; }\nA\nBEGIN { use lib 'phantom'; }\nB\n",
        "my $s = <<'EOF';\nuse lib 'x';\nEOF\n",
    ];

    for source in sources {
        assert!(
            extract_use_lib_operations(source).is_empty(),
            "heredoc body created an operation: {source:?}"
        );
    }
}

#[test]
fn pod_cut_prefix_does_not_terminate_pod() {
    let source = "\
use strict;\n\
=pod\n\
=cutlery\n\
BEGIN { use lib 'phantom_pod'; }\n\
=cut\n\
use Local::Thing;\n";

    assert!(extract_use_lib_operations(source).is_empty());
}

#[test]
fn unterminated_bareword_heredocs_do_not_hide_later_pragmas() {
    let sources = [
        "my $r = qr/<<MISSING/;\nuse lib 'later';\n",
        "my $x = 1 << MISSING;\nuse lib 'later';\n",
        "my $x = <<~MISSING;\nuse lib 'later';\n",
    ];

    for source in sources {
        assert_eq!(
            extract_use_lib_operations(source),
            vec![UseLibAction::Add(vec![UseLibPath {
                path: "later".to_string(),
                from_findbin: false,
            }])],
            "unterminated bareword heredoc hid a later pragma: {source:?}"
        );
    }
}

#[test]
fn pod_bodies_do_not_create_lib_operations() {
    let source = "\
use strict;\n\
\n\
=pod\n\
\n\
Example: use strict;\n\
\n\
BEGIN { use lib 'phantom_pod'; }\n\
\n\
=cut\n\
\n\
use Local::Thing;\n";

    assert!(extract_use_lib_operations(source).is_empty());
}

#[test]
fn code_after_closed_heredoc_and_pod_remains_scannable() {
    let heredoc_source = "\
my $s = <<'EOF';\n\
body\n\
EOF\n\
use lib 'real';\n";
    assert_eq!(
        extract_use_lib_operations(heredoc_source),
        vec![UseLibAction::Add(vec![UseLibPath { path: "real".to_string(), from_findbin: false }])]
    );

    let surrounding_source = "\
BEGIN { use lib 'before'; }\n\
my $s = <<'EOF';\n\
BEGIN { use lib 'phantom'; }\n\
EOF\n\
BEGIN { use lib 'after'; };\n\
=pod\n\
BEGIN { use lib 'phantom_pod'; }\n\
=cut\n\
BEGIN { use lib 'after_pod'; }\n";
    assert_eq!(
        extract_use_lib_operations(surrounding_source),
        vec![
            UseLibAction::Add(vec![UseLibPath { path: "before".to_string(), from_findbin: false }]),
            UseLibAction::Add(vec![UseLibPath { path: "after".to_string(), from_findbin: false }]),
            UseLibAction::Add(vec![UseLibPath {
                path: "after_pod".to_string(),
                from_findbin: false,
            }]),
        ]
    );
}

#[test]
fn begin_lookalikes_do_not_create_lib_operations() {
    let identifier = "BEGINNER { use lib 'fake'; }\n";
    let quoted = "BEGIN { \"use lib 'also_fake';\"; }\n";

    assert!(extract_use_lib_operations(identifier).is_empty());
    assert!(extract_use_lib_operations(quoted).is_empty());
}

/// A top-level pragma that follows a block must stay visible and stay *after*
/// the block-scoped one.
///
/// The slice following a block opens with that block's closing brace, which is
/// not part of the statement. Before the brace trim the scanner reported only
/// the `BEGIN`-scoped root; `lexical_paths` then front-inserted that single
/// root and moved the lexically-later `second` ahead of it in the effective
/// `@INC`, which is the ordering defect this pins.
#[test]
fn top_level_pragma_after_a_block_keeps_lexical_order() {
    let source = "BEGIN {\n    use lib 'first';\n}\nuse lib 'second';\nuse Recovered;\n";

    assert_eq!(
        extract_use_lib_operations(source),
        vec![
            UseLibAction::Add(vec![UseLibPath { path: "first".to_string(), from_findbin: false }]),
            UseLibAction::Add(vec![UseLibPath { path: "second".to_string(), from_findbin: false }]),
        ]
    );
}

/// The brace trim is not special to `BEGIN`: a top-level pragma after any block
/// is real code.
///
/// The pragma inside the conditional block stays out here only because it shares
/// its slice with the `if (1) {` opener — not because the rail excludes
/// conditional pragmas in general. It does not; see
/// `conditional_pragma_below_its_block_opener_is_an_ordinary_candidate`.
#[test]
fn top_level_pragma_after_a_conditional_block_is_scanned() {
    let source = "if (1) {\n    use lib 'conditional';\n}\nuse lib 'top_level';\n";

    assert_eq!(
        extract_use_lib_operations(source),
        vec![UseLibAction::Add(vec![UseLibPath {
            path: "top_level".to_string(),
            from_findbin: false,
        }])]
    );
}

/// Consecutive blocks stack closing braces in front of the next slice.
#[test]
fn stacked_block_closers_do_not_hide_later_pragmas() {
    let source = "BEGIN {\n    use lib 'a';\n}\nBEGIN {\n    use lib 'b';\n}\nuse lib 'c';\n";

    let paths: Vec<String> = extract_use_lib_operations(source)
        .into_iter()
        .map(|action| match action {
            UseLibAction::Add(paths) => paths[0].path.clone(),
            UseLibAction::Remove(paths) => paths[0].path.clone(),
        })
        .collect();

    assert_eq!(paths, vec!["a", "b", "c"]);
}

/// Perl allows whitespace between `<<`/`<<~` and a *quoted* delimiter, so those
/// bodies are prose and must not reach the rail.
#[test]
fn spaced_quoted_heredoc_bodies_do_not_create_lib_operations() {
    let ordinary =
        "my $s = << 'EOF';\nBEGIN { use lib 'phantom_spaced'; }\nEOF\nuse Local::Thing;\n";
    let indented =
        "my $s = <<~ 'EOF';\n  BEGIN { use lib 'phantom_indent'; }\n  EOF\nuse Local::Thing;\n";
    let double = "my $s = << \"EOF\";\nuse lib 'phantom_double';\nEOF\nuse Local::Thing;\n";

    assert!(extract_use_lib_operations(ordinary).is_empty());
    assert!(extract_use_lib_operations(indented).is_empty());
    assert!(extract_use_lib_operations(double).is_empty());
}

/// A spaced quoted opener is ambiguous with a shift by a string literal, so it
/// only counts as a heredoc when a terminator line confirms it. Without that
/// confirmation the following pragma must stay visible.
#[test]
fn spaced_shift_expression_does_not_hide_later_pragmas() {
    let source = "my $n = 1 << 'EOF';\nuse lib 'real';\n";

    assert_eq!(
        extract_use_lib_operations(source),
        vec![UseLibAction::Add(vec![UseLibPath { path: "real".to_string(), from_findbin: false }])]
    );
}

/// `<< EOF` with a bareword delimiter is the left-shift operator, never a
/// heredoc, so nothing after it may be swallowed.
#[test]
fn spaced_bareword_is_not_a_heredoc_opener() {
    let source = "my $n = $bits << SHIFT;\nuse lib 'still_scanned';\nSHIFT\n";

    assert_eq!(
        extract_use_lib_operations(source),
        vec![UseLibAction::Add(vec![UseLibPath {
            path: "still_scanned".to_string(),
            from_findbin: false,
        }])]
    );
}

/// A trailing comment consumes the newline that drains pending heredoc bodies,
/// so the drain has to happen at the comment's line boundary instead.
#[test]
fn commented_heredoc_openers_do_not_expose_body_pragmas() {
    let quoted = "my $s = <<'EOF'; # note\nuse lib 'phantom_comment';\nEOF\nuse lib 'real';\n";
    let bareword = "my $s = <<EOF; # note\nuse lib 'phantom_bareword';\nEOF\nuse lib 'real';\n";
    let stacked = "my @s = (<<'A', <<'B'); # two\nuse lib 'phantom_a';\nA\nuse lib 'phantom_b';\nB\nuse lib 'real';\n";

    for source in [quoted, bareword, stacked] {
        assert_eq!(
            extract_use_lib_operations(source),
            vec![UseLibAction::Add(vec![UseLibPath {
                path: "real".to_string(),
                from_findbin: false,
            }])],
            "commented heredoc leaked a body pragma: {source:?}"
        );
    }
}

/// `<<` after a complete term is the left-shift operator, not a heredoc.
///
/// Perl decides this by lexer position and it is observable: `perl -c` accepts
/// `my $x = 1 <<'EOF';` with no `EOF` line anywhere. Treating that as an
/// unconfirmed heredoc swallowed every later pragma.
#[test]
fn shift_after_a_complete_term_does_not_hide_later_pragmas() {
    let number = "my $x = 1 <<'EOF';\nuse lib 'real';\n";
    let variable = "my $x = $bits <<'EOF';\nuse lib 'real';\n";
    let paren = "my $x = ($bits) <<'EOF';\nuse lib 'real';\n";

    for source in [number, variable, paren] {
        assert_eq!(
            extract_use_lib_operations(source),
            vec![UseLibAction::Add(vec![UseLibPath {
                path: "real".to_string(),
                from_findbin: false,
            }])],
            "shift expression swallowed a later pragma: {source:?}"
        );
    }
}

/// A bareword before `<<` is a function call, which leaves the opener in term
/// position — so `print <<'EOF'` stays a heredoc and its body stays out.
#[test]
fn heredoc_after_a_bareword_call_still_hides_its_body() {
    let source = "print <<'EOF';\nuse lib 'phantom_print';\nEOF\nuse lib 'real';\n";

    assert_eq!(
        extract_use_lib_operations(source),
        vec![UseLibAction::Add(vec![UseLibPath { path: "real".to_string(), from_findbin: false }])]
    );
}

/// Perl accepts an empty heredoc delimiter (`<<''`), terminated by a blank
/// line. Rejecting the opener scanned its body as code.
#[test]
fn empty_heredoc_delimiter_bodies_do_not_create_lib_operations() {
    let source = "my $s = <<'';\nuse lib 'phantom_empty';\n\nuse lib 'real';\n";

    assert_eq!(
        extract_use_lib_operations(source),
        vec![UseLibAction::Add(vec![UseLibPath { path: "real".to_string(), from_findbin: false }])]
    );
}

/// A term-position heredoc still being typed has no terminator yet. It must
/// suppress the text below it rather than invent an `@INC` root from it.
#[test]
fn unterminated_term_position_heredoc_suppresses_rather_than_invents() {
    let source = "my $s = <<'EOF';\nuse lib 'phantom_typing';\n";

    assert!(extract_use_lib_operations(source).is_empty());
}

/// `<<\EOF` is a heredoc with single-quote semantics — `perl -c` accepts it and
/// prints the body — so its body is prose, not code.
#[test]
fn backslash_heredoc_bodies_do_not_create_lib_operations() {
    let source = "my $s = <<\\EOF;\nuse lib 'phantom_backslash';\nEOF\nuse lib 'real';\n";

    assert_eq!(
        extract_use_lib_operations(source),
        vec![UseLibAction::Add(vec![UseLibPath { path: "real".to_string(), from_findbin: false }])]
    );
}

/// The rail does not exclude conditional pragmas, and never did.
///
/// A pragma that shares its slice with the block opener is invisible because the
/// slice starts with `if`, not because of a conditional rule. One that does not
/// share that slice is an ordinary approximate candidate — the same on `main` as
/// here, with or without the brace trim. This pins that boundary so the
/// neighbouring `..._after_a_conditional_block_...` test is not read as a
/// promise the rail does not make.
#[test]
fn conditional_pragma_below_its_block_opener_is_an_ordinary_candidate() {
    let source = "if ($x) {\n    foo();\n    use lib 'conditional';\n}\n";

    assert_eq!(
        extract_use_lib_operations(source),
        vec![UseLibAction::Add(vec![UseLibPath {
            path: "conditional".to_string(),
            from_findbin: false,
        }])]
    );
}
