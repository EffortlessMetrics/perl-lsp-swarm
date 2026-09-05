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

/// Perl's indirect filehandle syntax leaves `<<` in term position, so
/// `print $fh <<'EOF'` is a heredoc even though a sigiled variable precedes it.
///
/// `perl -c` proves it: with no terminator the file fails with "Can't find
/// string terminator". Classifying the filehandle as a shift operand meant the
/// body was scanned as code while the heredoc was still being typed.
#[test]
fn filehandle_heredoc_bodies_do_not_create_lib_operations() {
    let terminated = "print $fh <<'EOF';\nuse lib 'phantom_fh';\nEOF\nuse lib 'real';\n";
    let still_being_typed = "print $fh <<'EOF';\nuse lib 'phantom_typing';\n";

    assert_eq!(
        extract_use_lib_operations(terminated),
        vec![UseLibAction::Add(vec![UseLibPath { path: "real".to_string(), from_findbin: false }])]
    );
    assert!(extract_use_lib_operations(still_being_typed).is_empty());
}

/// `<<` after a complete term is the shift operator, and Perl never revisits
/// that choice — so a terminator appearing later must not reclassify it.
///
/// `perl -c` accepts `my $x = 1 <<\EOF;` with no terminator anywhere, and
/// running it prints `x=[0]` while parsing the following lines as code. Both
/// delimiter forms must therefore stay visible-through, terminator or not.
#[test]
fn shift_is_not_reclassified_by_a_later_terminator_line() {
    let backslash = "my $x = 1 <<\\EOF;\nuse lib 'real';\nEOF\n";
    let quoted = "my $x = 1 <<'EOF';\nuse lib 'real';\nEOF\n";

    for source in [backslash, quoted] {
        assert_eq!(
            extract_use_lib_operations(source),
            vec![UseLibAction::Add(vec![UseLibPath {
                path: "real".to_string(),
                from_findbin: false,
            }])],
            "a later terminator line reclassified a shift as a heredoc: {source:?}"
        );
    }
}

/// A bareword before a sigiled variable does not by itself put `<<` in term
/// position — only the builtins that actually take an indirect filehandle do.
///
/// `perl -c` separates the groups: `say $fh <<'EOF'` demands the terminator,
/// while `return $x <<'EOF'`, `defined $y <<'EOF'`, and `scalar $y <<'EOF'`
/// are accepted without one, so those are shifts and must not hide what
/// follows.
#[test]
fn keyword_led_shifts_do_not_hide_later_pragmas() {
    let sources = [
        "sub f { my $z = return $x <<'EOF'; }\nuse lib 'real';\n",
        "my $z = defined $y <<'EOF';\nuse lib 'real';\n",
        "my $z = scalar $y <<'EOF';\nuse lib 'real';\n",
        // A bareword that merely ends in a builtin's name is not that builtin.
        "my $z = sprint $y <<'EOF';\nuse lib 'real';\n",
    ];

    for source in sources {
        assert_eq!(
            extract_use_lib_operations(source),
            vec![UseLibAction::Add(vec![UseLibPath {
                path: "real".to_string(),
                from_findbin: false,
            }])],
            "a keyword-led shift swallowed a later pragma: {source:?}"
        );
    }
}

/// The output builtins keep the indirect-filehandle heredoc working.
#[test]
fn filehandle_builtins_still_open_heredocs() {
    for builtin in ["print", "printf", "say"] {
        let source = format!("{builtin} $fh <<'EOF';\nuse lib 'phantom';\nEOF\nuse lib 'real';\n");

        assert_eq!(
            extract_use_lib_operations(&source),
            vec![UseLibAction::Add(vec![UseLibPath {
                path: "real".to_string(),
                from_findbin: false,
            }])],
            "{builtin} lost its indirect-filehandle heredoc"
        );
    }
}

/// A column-0 `__END__` or `__DATA__` ends the code region. Text below it is
/// data Perl never compiles, so a semicolon down there must not start a slice
/// the rail treats as a pragma.
#[test]
fn data_section_markers_end_the_scanned_region() {
    let sources = [
        "use lib 'real';\n__END__\nprose; use lib 'phantom_end';\n",
        "use lib 'real';\n__DATA__\nrow; use lib 'phantom_data';\n",
        // The boundary is the end of the identifier, not whitespace: `perl -c`
        // accepts `__END__;` and `__END__ trailing words` alike and treats the
        // rest of the file as data.
        "use lib 'real';\n__END__;\nprose; use lib 'phantom_punctuated';\n",
        "use lib 'real';\n__END__ trailing words\nprose; use lib 'phantom_trailing';\n",
    ];

    for source in sources {
        assert_eq!(
            extract_use_lib_operations(source),
            vec![UseLibAction::Add(vec![UseLibPath {
                path: "real".to_string(),
                from_findbin: false,
            }])],
            "text below a data-section marker reached the rail: {source:?}"
        );
    }
}

/// A colon after the marker makes it a *label*, so the code region continues.
///
/// Measured by running each form: `__END__;` and `__END__ trailing words` print
/// only the line above them, but `__END__:` and `__DATA__:` print the line
/// below as well — the colon turns the token into `LABEL:` rather than a
/// data-section marker. An earlier version of this suite listed `__DATA__:`
/// among the markers, asserting the opposite of the interpreter.
///
/// The residual boundary is shared with every label, not specific to these two:
/// a pragma that shares a statement slice with its label is absorbed, because
/// `split_perl_statements` cuts on semicolons and a label carries none. That is
/// the same class as an empty block opener, and it behaves identically for an
/// ordinary `MYLOOP:` on `main`. What matters here is that the scan is no
/// longer *truncated* — everything below the colon form stays reachable.
#[test]
fn colon_suffixed_markers_are_labels_and_do_not_truncate_the_scan() {
    for marker in ["__END__", "__DATA__", "MYLOOP"] {
        let source = format!("use lib 'real';\n{marker}:\n1;\nuse lib 'still_code';\n");

        assert_eq!(
            extract_use_lib_operations(&source),
            vec![
                UseLibAction::Add(vec![UseLibPath {
                    path: "real".to_string(),
                    from_findbin: false
                }]),
                UseLibAction::Add(vec![UseLibPath {
                    path: "still_code".to_string(),
                    from_findbin: false,
                }]),
            ],
            "{marker}: truncated the scan instead of acting as a label"
        );

        // The shared boundary: same slice as the label, so absorbed.
        let same_slice = format!("use lib 'real';\n{marker}:\nuse lib 'absorbed';\n");
        assert_eq!(
            extract_use_lib_operations(&same_slice),
            vec![UseLibAction::Add(vec![UseLibPath {
                path: "real".to_string(),
                from_findbin: false
            }])],
            "{marker}: unexpectedly delimited a same-slice pragma"
        );
    }
}

/// Across whitespace, `::` stays data while a single colon still makes a label.
///
/// Measured: `__END__ ::foo();` stops the program (marker), `__END__::foo();`
/// dies calling the sub (code), and `__END__ :` keeps printing (label). A
/// label cannot be spelled `::`, so a spaced `::` is just data payload.
///
/// This direction is the dangerous one. Treating `__END__ ::foo()` as code
/// makes the scanner read the data section and *invent* include paths from it,
/// rather than merely losing one.
#[test]
fn a_spaced_double_colon_stays_inside_the_data_section() {
    for marker in ["__END__", "__DATA__"] {
        for gap in [" ", "  ", "\t"] {
            let source =
                format!("use lib 'real';\n{marker}{gap}::pkg::call();\nuse lib 'phantom';\n");

            assert_eq!(
                extract_use_lib_operations(&source),
                vec![UseLibAction::Add(vec![UseLibPath {
                    path: "real".to_string(),
                    from_findbin: false
                }])],
                "{marker}{gap:?}:: was read as code, inventing a path from data"
            );
        }
    }
}

/// An *adjacent* `::` is a package-qualified call, so the code region continues.
#[test]
fn an_adjacent_double_colon_is_a_package_qualified_call() {
    let source = "use lib 'real';\n__END__::foo();\nuse lib 'still_code';\n";

    assert_eq!(
        extract_use_lib_operations(source),
        vec![
            UseLibAction::Add(vec![UseLibPath { path: "real".to_string(), from_findbin: false }]),
            UseLibAction::Add(vec![UseLibPath {
                path: "still_code".to_string(),
                from_findbin: false,
            }]),
        ]
    );
}

/// A marker lookalike is ordinary code and must not truncate the scan.
#[test]
fn data_section_marker_lookalikes_do_not_end_the_scan() {
    let source = "use lib 'real';\n__ENDS__ = 1; use lib 'still_code';\n";

    assert_eq!(
        extract_use_lib_operations(source),
        vec![
            UseLibAction::Add(vec![UseLibPath { path: "real".to_string(), from_findbin: false }]),
            UseLibAction::Add(vec![UseLibPath {
                path: "still_code".to_string(),
                from_findbin: false,
            }]),
        ]
    );
}

/// `::` is the one punctuation that does not end the marker token.
///
/// `__END__::foo()` is a package-qualified call, not a data-section marker:
/// perl compiles and runs it, reaching the statements below. Treating it as a
/// marker would drop every pragma that follows.
#[test]
fn package_qualified_marker_lookalikes_do_not_end_the_scan() {
    let source = "use lib 'first';\n__END__::foo();\nuse lib 'second';\n";

    assert_eq!(
        extract_use_lib_operations(source),
        vec![
            UseLibAction::Add(vec![UseLibPath { path: "first".to_string(), from_findbin: false }]),
            UseLibAction::Add(vec![UseLibPath { path: "second".to_string(), from_findbin: false }]),
        ]
    );
}

/// A string literal ends a term, so `<<` after one is the shift operator.
///
/// `perl -c` accepts `my $x = 'a' <<'EOF';` with no terminator anywhere.
#[test]
fn shift_after_a_string_literal_does_not_hide_later_pragmas() {
    for source in
        ["my $x = 'a' <<'EOF';\nuse lib 'real';\n", "my $x = \"a\" <<'EOF';\nuse lib 'real';\n"]
    {
        assert_eq!(
            extract_use_lib_operations(source),
            vec![UseLibAction::Add(vec![UseLibPath {
                path: "real".to_string(),
                from_findbin: false,
            }])],
            "a shift after a string literal swallowed a later pragma: {source:?}"
        );
    }
}

/// A closing brace does *not* end a term, because Perl's braced indirect
/// filehandle form puts `<<` in term position.
///
/// `print {$fh} <<'EOF'` is a heredoc — `perl -c` demands the terminator — so
/// reading the `}` as a shift operand would scan the body as code and invent an
/// `@INC` root. Both the terminated and the still-being-typed forms are pinned:
/// the second must suppress rather than invent.
#[test]
fn braced_filehandle_heredoc_bodies_do_not_create_lib_operations() {
    let terminated = "print {$fh} <<'EOF';\nuse lib 'phantom';\nEOF\nuse lib 'real';\n";
    let still_being_typed = "print {$fh} <<'EOF';\nuse lib 'phantom_typing';\n";

    assert_eq!(
        extract_use_lib_operations(terminated),
        vec![UseLibAction::Add(vec![UseLibPath { path: "real".to_string(), from_findbin: false }])]
    );
    assert!(extract_use_lib_operations(still_being_typed).is_empty());
}

/// A data-section marker inside a quote-like literal is prose, and the rail
/// cannot tell — it has no `q{}` / `qq{}` state, by design.
///
/// `perl` runs the code after such a literal (the marker is not a marker), but
/// the scanner truncates there and loses later pragmas. This pins the boundary
/// as proof rather than silence. The direction is the acceptable one: candidates
/// are lost, never invented. Exact `q{}` handling belongs to the parser-native
/// `@INC` train (#10568/#10569).
#[test]
fn data_marker_inside_a_quote_like_literal_is_a_known_truncation() {
    let source = "my $text = q{\n__END__\n};\nuse lib 'unreachable_for_this_rail';\n";

    assert!(
        extract_use_lib_operations(source).is_empty(),
        "boundary changed: update the documented q{{}} limitation if this now resolves"
    );
}

/// A quoted heredoc delimiter may contain a backslash-escaped quote.
///
/// `perl -c` accepts `my $s = <<'E\'OF';` with terminator line `E'OF`. Stopping
/// the delimiter scan at the escaped quote took `E\` as the tag, so the real
/// terminator never matched and everything below the heredoc was suppressed.
#[test]
fn escaped_quote_in_a_heredoc_delimiter_is_read_whole() {
    let source = "my $s = <<'E\\'OF';\nuse lib 'phantom';\nE'OF\nuse lib 'real';\n";

    assert_eq!(
        extract_use_lib_operations(source),
        vec![UseLibAction::Add(vec![UseLibPath { path: "real".to_string(), from_findbin: false }])]
    );
}

/// A backslash inside a quoted heredoc delimiter is literal unless it escapes
/// that delimiter's own quote.
///
/// Verified against `perl -c`: `<<'E\OF'` terminates on `E\OF`, and `<<'E\\OF'`
/// terminates on `E\\OF` with both backslashes intact — Perl does not collapse
/// `\\` here the way it would inside an ordinary single-quoted string. Dropping
/// the backslash produced a tag that never matched, suppressing everything below.
#[test]
fn a_backslash_before_a_non_quote_stays_in_the_heredoc_delimiter() {
    let single = "my $s = <<'E\\OF';\nuse lib 'phantom';\nE\\OF\nuse lib 'single';\n";
    assert_eq!(
        extract_use_lib_operations(single),
        vec![UseLibAction::Add(vec![UseLibPath {
            path: "single".to_string(),
            from_findbin: false
        }])]
    );

    let doubled = "my $s = <<'E\\\\OF';\nuse lib 'phantom';\nE\\\\OF\nuse lib 'doubled';\n";
    assert_eq!(
        extract_use_lib_operations(doubled),
        vec![UseLibAction::Add(vec![UseLibPath {
            path: "doubled".to_string(),
            from_findbin: false
        }])]
    );

    let double_quoted = "my $s = <<\"E\\\"OF\";\nuse lib 'phantom';\nE\"OF\nuse lib 'dq';\n";
    assert_eq!(
        extract_use_lib_operations(double_quoted),
        vec![UseLibAction::Add(vec![UseLibPath { path: "dq".to_string(), from_findbin: false }])]
    );
}

/// A column-zero `=cut` opens POD even with no POD before it, so the rest of the
/// file is documentation.
///
/// This matches Perl exactly, which is why it is pinned rather than "fixed":
/// `print "before\n";\n=cut\nprint "after\n";` prints only `before`. A review
/// reading claimed Perl resumes code after a standalone `=cut`; the oracle says
/// otherwise, and the scanner already agrees with the oracle.
#[test]
fn standalone_pod_terminator_opens_pod_like_perl_does() {
    let source = "use lib 'first';\n=cut\nuse lib 'after_cut';\n";

    assert_eq!(
        extract_use_lib_operations(source),
        vec![UseLibAction::Add(vec![UseLibPath {
            path: "first".to_string(),
            from_findbin: false
        }])]
    );
}

/// Bareword confirmation must not be quadratic in the number of candidates.
///
/// `MASK<<SHIFT` is a tight bareword shift that Perl accepts (`perl` prints
/// `ok 8` for it), and the scanner must treat it as a heredoc candidate until
/// a terminator search says otherwise. Searching from each candidate to end of
/// file made that search quadratic: measured on this shape, 1,000 candidates
/// took 12ms, 2,000 took 49ms, and 4,000 took 193ms — a stall in a path that
/// runs on every keystroke. A set of the file's trimmed lines, built once,
/// answers the hopeless cases without a scan and restores linear behaviour:
/// the same 4,000 candidates take 2.9ms and 16,000 take 12ms.
///
/// The bound below is sized to sit between two measured numbers, both taken in
/// a debug build: 11.57ms linear and 3.14s quadratic, a factor of 272 apart.
/// At 2s it would take a ~170x slower runner to flake, and the regression stays
/// detectable on a runner up to ~50x slower. Loosening it is not free — at a
/// 10s bound a merely 2x slower runner takes 6.3s quadratic, under the bound,
/// and the test silently stops catching the thing it exists for.
#[test]
fn bareword_confirmation_stays_linear_in_candidate_count() {
    let candidates = 16_000;
    let mut source = String::from("use lib 'real';\n");
    for i in 0..candidates {
        source.push_str(&format!("my $m{i} = MASK{i}<<SHIFT{i};\n"));
    }
    source.push_str("use lib 'tail';\n");

    let started = std::time::Instant::now();
    let operations = extract_use_lib_operations(&source);
    let elapsed = started.elapsed();

    assert_eq!(
        operations,
        vec![
            UseLibAction::Add(vec![UseLibPath { path: "real".to_string(), from_findbin: false }]),
            UseLibAction::Add(vec![UseLibPath { path: "tail".to_string(), from_findbin: false }]),
        ],
        "unconfirmed bareword candidates changed the extracted paths"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "bareword confirmation took {elapsed:?} for {candidates} candidates against a 2s bound. \
         Measured in a debug build, the linear implementation takes ~12ms here and the quadratic \
         one ~3.1s, so this is the quadratic scan returning unless the runner is roughly 170x \
         slower than the machine this was sized on"
    );
}

/// An indented `__END__` / `__DATA__` still ends the code region.
///
/// Verified by running each form: `    __END__` (spaces) and a tab-indented
/// marker both print only the line above them, exactly like the column-zero
/// spelling. Requiring column zero left the data payload to be scanned as
/// code, which is the invention direction — text below the marker becomes an
/// `@INC` root the program never adds.
#[test]
fn an_indented_data_marker_still_ends_the_scan() {
    for marker in ["__END__", "__DATA__"] {
        for indent in ["    ", "\t", " "] {
            let source = format!("use lib 'real';\n{indent}{marker}\nprose; use lib 'phantom';\n");

            assert_eq!(
                extract_use_lib_operations(&source),
                vec![UseLibAction::Add(vec![UseLibPath {
                    path: "real".to_string(),
                    from_findbin: false
                }])],
                "{indent:?}{marker} was scanned as code, inventing a path from data"
            );
        }
    }
}

/// A bareword heredoc delimiter may open with a digit.
///
/// `my $s = <<123;` terminated by a `123` line runs and prints; without that
/// line Perl reports `Can't find string terminator "123"`, so it is a heredoc
/// rather than a shift. Rejecting digit-initial tags left the body to be
/// scanned as code and invented a root from it. Position still rules out the
/// shift readings — `$x<<2` and `1<<2` end a term and never reach this path.
#[test]
fn a_numeric_bareword_delimiter_opens_a_heredoc() {
    let numeric = "use lib 'real';\nmy $s = <<123;\nuse lib 'phantom';\n123\nuse lib 'tail';\n";
    assert_eq!(
        extract_use_lib_operations(numeric),
        vec![
            UseLibAction::Add(vec![UseLibPath { path: "real".to_string(), from_findbin: false }]),
            UseLibAction::Add(vec![UseLibPath { path: "tail".to_string(), from_findbin: false }]),
        ],
        "a <<123 heredoc body reached the rail"
    );

    // The shift readings are unaffected: both operands end a term.
    for (shift, expected) in [
        ("my $x = $y<<2;\nuse lib 'shift_var';\n", "shift_var"),
        ("my $x = 1<<2;\nuse lib 'shift_num';\n", "shift_num"),
    ] {
        assert_eq!(
            extract_use_lib_operations(shift),
            vec![UseLibAction::Add(vec![UseLibPath {
                path: expected.to_string(),
                from_findbin: false
            }])],
            "a tight numeric shift was read as a heredoc: {shift:?}"
        );
    }
}

/// POD between a closed block and a pragma must not hide the pragma.
///
/// `split_perl_statements` cuts on semicolons, so the slice after `sub f {
/// return 1; }` opens with that block's `}`. The POD branch only moved the
/// slice start when nothing had been seen yet, and the brace counted as
/// something, so the POD text stayed in front of the pragma —
/// `strip_statement_prefix` drops a leading brace but not a POD section, and
/// the pragma was never recognized.
///
/// The three controls isolate it to that interaction: POD without a preceding
/// block, a block without POD, and POD opening the file all worked already.
#[test]
fn pod_between_a_closed_block_and_a_pragma_keeps_the_pragma() {
    let real =
        vec![UseLibAction::Add(vec![UseLibPath { path: "real".to_string(), from_findbin: false }])];

    let block_then_pod = "sub f { return 1; }\n=pod\n\nprose\n\n=cut\nuse lib 'real';\n";
    assert_eq!(extract_use_lib_operations(block_then_pod), real, "block + POD hid the pragma");

    for control in [
        "my $x = 1;\n=pod\n\nprose\n\n=cut\nuse lib 'real';\n",
        "sub f { return 1; }\nuse lib 'real';\n",
        "=pod\n\nprose\n\n=cut\nuse lib 'real';\n",
    ] {
        assert_eq!(extract_use_lib_operations(control), real, "control regressed: {control:?}");
    }

    // A genuinely unterminated expression before the POD still suppresses:
    // the rail must not invent a statement boundary that Perl does not have.
    let unterminated = "my $x = (\n=pod\n\nprose\n\n=cut\nuse lib 'phantom';\n";
    assert_eq!(
        extract_use_lib_operations(unterminated),
        vec![],
        "an unterminated expression gained a statement boundary it should not have"
    );
}

/// An unfinished expression after a closed block still suppresses.
///
/// Only the stripped closing brace may be treated as ignorable. The quote
/// toggles and the heredoc opener each `continue` before the general content
/// tracker, so they had to set `has_code_content` explicitly; without that an
/// empty `''` or `""` after a block looked like no content at all, a following
/// POD section discarded it, and the pragma below was exposed as though it
/// were reachable code — inventing an `@INC` root from unfinished editor text.
#[test]
fn an_unfinished_expression_after_a_block_still_suppresses() {
    for unfinished in ["''", "\"\"", "'\\\\'", "my $x = 'unclosed", "<<EOF", "my $x = ("] {
        let source = format!(
            "sub f {{ return 1; }}\n{unfinished}\n=pod\n\nprose\n\n=cut\nuse lib 'phantom';\n"
        );

        assert_eq!(
            extract_use_lib_operations(&source),
            vec![],
            "{unfinished:?} was treated as empty, inventing a path from below the POD"
        );
    }
}
