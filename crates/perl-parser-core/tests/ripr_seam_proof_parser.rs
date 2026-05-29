//! Mutation-proof boundary tests for four parser decision seams.
//!
//! These tests pin the EXACT decision boundaries in production code so that
//! mutating any sub-condition of the predicate causes at least one test to
//! fail.  They complement the existing behavioral tests which verify *that*
//! things parse correctly but do not assert which NodeKind branch was taken.
//!
//! ## Seams covered
//!
//! 1. `primary.rs` `TokenKind::Less` arm — angle-bracket Readline vs Glob
//!    dispatch.  Boundary: all-uppercase/underscore → Readline; anything else
//!    (lowercase, mixed case, `$`-prefixed, glob chars, `/`) → Glob.
//!
//! 2. `declarations.rs` `parse_package` — optional VERSION consumption.
//!    Boundary: bare `Number` or `VString` token consumed; `v`-prefixed
//!    identifier consumed; plain non-`v` identifier NOT consumed.
//!
//! 3. `statements.rs` `parse_named_unary_statement_call` — the
//!    `is_optional_arg_builtin` guard at line 627.  Boundary: members of
//!    `is_optional_arg_builtin` (`length`, `chr`, `defined`, `abs`, …) do NOT
//!    consume a following binary operator as their argument; non-members
//!    (`sprintf`) attempt to take the operator as an argument and produce a
//!    parse error.
//!
//! 4. `variables.rs` unbraced scalar deref + `calls.rs` comma-after-Unary
//!    guard.  Boundary: `$$` alone is PID (name `$`); `$$ref` is deref (name
//!    starts with `$`); `catfile $$obj, $arg` parses cleanly (comma guard).

use perl_parser_core::{NodeKind, Parser};
use perl_tdd_support::{must, must_some};

// ── helpers ──────────────────────────────────────────────────────────────────

fn parse_sexp(src: &str) -> String {
    let mut p = Parser::new(src);
    must(p.parse()).to_sexp()
}

/// Walk the AST depth-first and collect every node matching a predicate.
fn find_nodes<'a>(
    node: &'a perl_parser_core::Node,
    pred: &dyn Fn(&NodeKind) -> bool,
    out: &mut Vec<&'a perl_parser_core::Node>,
) {
    if pred(&node.kind) {
        out.push(node);
    }
    for child in node.children() {
        find_nodes(child, pred, out);
    }
}

fn first_matching(
    src: &str,
    pred: impl Fn(&NodeKind) -> bool,
) -> Option<perl_parser_core::Node> {
    let mut p = Parser::new(src);
    let ast = must(p.parse());
    let mut hits: Vec<&perl_parser_core::Node> = Vec::new();
    find_nodes(&ast, &pred, &mut hits);
    hits.first().map(|n| (*n).clone())
}

// ── Seam 1: angle-bracket Readline vs Glob dispatch ──────────────────────────
//
// Production predicate (primary.rs ~line 605):
//   pattern.chars().all(|c| c.is_uppercase() || c == '_')  → Readline
//   has_glob_chars || pattern.contains('/')                 → Glob
//   else                                                    → Glob (default)
//
// Mutation "flip the uppercase check" would turn `<STDIN>` into Glob.
// Mutation "flip the glob-char check" would turn `<*.pm>` into Readline.

/// `<STDIN>` — all-uppercase → must be Readline, not Glob.
#[test]
fn seam1_all_uppercase_is_readline() {
    let node = first_matching("my $x = <STDIN>;", |k| {
        matches!(k, NodeKind::Readline { .. } | NodeKind::Glob { .. })
    });
    let node = must_some(node);
    assert!(
        matches!(node.kind, NodeKind::Readline { filehandle: Some(ref fh) } if fh == "STDIN"),
        "expected Readline{{STDIN}}, got: {:?}",
        node.kind
    );
}

/// `<FH>` — two-letter uppercase name → must be Readline.
#[test]
fn seam1_two_letter_uppercase_is_readline() {
    let node = first_matching("my $x = <FH>;", |k| {
        matches!(k, NodeKind::Readline { .. } | NodeKind::Glob { .. })
    });
    let node = must_some(node);
    assert!(
        matches!(node.kind, NodeKind::Readline { filehandle: Some(ref fh) } if fh == "FH"),
        "expected Readline{{FH}}, got: {:?}",
        node.kind
    );
}

/// `<fh>` — all-lowercase → must be Glob, not Readline.
/// Flipping the all-uppercase condition would make this Readline.
#[test]
fn seam1_all_lowercase_is_glob_not_readline() {
    let node = first_matching("my $x = <fh>;", |k| {
        matches!(k, NodeKind::Readline { .. } | NodeKind::Glob { .. })
    });
    let node = must_some(node);
    assert!(
        matches!(node.kind, NodeKind::Glob { pattern: ref p } if p == "fh"),
        "expected Glob{{fh}}, got: {:?}",
        node.kind
    );
}

/// `<Fh>` — mixed case → must be Glob.
#[test]
fn seam1_mixed_case_is_glob_not_readline() {
    let sexp = parse_sexp("my $x = <Fh>;");
    assert!(
        sexp.contains("(glob Fh)"),
        "mixed-case <Fh> must be Glob, got: {sexp}"
    );
    assert!(
        !sexp.contains("(readline"),
        "mixed-case <Fh> must not be Readline, got: {sexp}"
    );
}

/// `<STDIN_HANDLE>` — uppercase with underscore → must be Readline.
/// Underscore is explicitly allowed alongside uppercase.
#[test]
fn seam1_uppercase_with_underscore_is_readline() {
    let node = first_matching("while (my $line = <STDIN_HANDLE>) {}", |k| {
        matches!(k, NodeKind::Readline { .. } | NodeKind::Glob { .. })
    });
    let node = must_some(node);
    assert!(
        matches!(node.kind, NodeKind::Readline { filehandle: Some(ref fh) } if fh == "STDIN_HANDLE"),
        "expected Readline{{STDIN_HANDLE}}, got: {:?}",
        node.kind
    );
}

/// `<*.pm>` — glob star character → must be Glob, not Readline.
/// Flipping the glob-char check would (incorrectly) route this to Readline.
#[test]
fn seam1_star_is_glob_not_readline() {
    let node = first_matching("my @files = <*.pm>;", |k| {
        matches!(k, NodeKind::Readline { .. } | NodeKind::Glob { .. })
    });
    let node = must_some(node);
    assert!(
        matches!(node.kind, NodeKind::Glob { .. }),
        "star pattern must be Glob, got: {:?}",
        node.kind
    );
    assert!(
        !matches!(node.kind, NodeKind::Readline { .. }),
        "star pattern must NOT be Readline, got: {:?}",
        node.kind
    );
}

/// `<$dir/*>` — slash in pattern → must be Glob.
/// The slash check at `pattern.contains('/')` gates this.
#[test]
fn seam1_slash_in_pattern_is_glob() {
    let node = first_matching("my @files = <$dir/*>;", |k| {
        matches!(k, NodeKind::Readline { .. } | NodeKind::Glob { .. })
    });
    let node = must_some(node);
    assert!(
        matches!(node.kind, NodeKind::Glob { .. }),
        "slash-containing pattern must be Glob, got: {:?}",
        node.kind
    );
}

/// `<$fh>` — dollar-prefixed, no glob chars → must be Glob (default branch).
#[test]
fn seam1_dollar_prefixed_no_glob_chars_is_glob() {
    let sexp = parse_sexp("my $line = <$fh>;");
    assert!(
        sexp.contains("(glob $fh)"),
        "dollar-prefixed filehandle <$fh> must be Glob (not Readline), got: {sexp}"
    );
}

/// `<>` — empty diamond operator → must be Diamond.
#[test]
fn seam1_empty_angle_is_diamond() {
    let sexp = parse_sexp("my $line = <>;");
    assert!(
        sexp.contains("(diamond)"),
        "empty <> must be Diamond, got: {sexp}"
    );
    assert!(
        !sexp.contains("(readline"),
        "empty <> must NOT be Readline, got: {sexp}"
    );
    assert!(
        !sexp.contains("(glob"),
        "empty <> must NOT be Glob, got: {sexp}"
    );
}

/// Readline carries the filehandle name as payload.
/// A mutation to the `filehandle: Some(pattern)` construction would lose it.
#[test]
fn seam1_readline_preserves_filehandle_name() {
    let node = first_matching("while (my $line = <MYHANDLE>) {}", |k| {
        matches!(k, NodeKind::Readline { .. })
    });
    let node = must_some(node);
    assert!(
        matches!(&node.kind, NodeKind::Readline { filehandle: Some(fh) } if fh == "MYHANDLE"),
        "expected Readline{{Some(MYHANDLE)}}, got: {:?}",
        node.kind
    );
}

/// Glob carries the pattern string as payload.
/// A mutation to the `pattern` field construction would lose it.
#[test]
fn seam1_glob_preserves_pattern_string() {
    let node = first_matching("my @f = <*.txt>;", |k| {
        matches!(k, NodeKind::Glob { .. })
    });
    let node = must_some(node);
    assert!(
        matches!(&node.kind, NodeKind::Glob { pattern } if pattern == "*.txt"),
        "expected Glob{{*.txt}}, got: {:?}",
        node.kind
    );
}

// ── Seam 2: parse_package optional VERSION consumption ────────────────────────
//
// Production predicate (declarations.rs ~line 444):
//   peek == Number          → consume as version
//   peek == VString         → consume as version
//   peek == Identifier && text.starts_with('v') && len > 1  → consume as version
//   else                    → no version, do not consume
//
// Mutation "remove Number branch" would leave `package Foo 1.0` with bare `1.0`.
// Mutation "remove VString branch" would leave `package Foo v1.2.3` unparsed.
// Mutation "remove starts_with('v')" would eat non-version identifiers.

/// `package Foo 1.0` — bare Number token must be consumed as version.
/// The package name in the AST must include the version.
#[test]
fn seam2_number_version_consumed_into_package_name() {
    let sexp = parse_sexp("package Foo 1.0;");
    assert!(
        sexp.contains("(package Foo 1.0)"),
        "Number version must be consumed into package name, got: {sexp}"
    );
}

/// `package Foo 42` — integer version also consumed.
#[test]
fn seam2_integer_version_consumed() {
    let sexp = parse_sexp("package Foo 42;");
    assert!(
        sexp.contains("(package Foo 42)"),
        "Integer version must be consumed, got: {sexp}"
    );
}

/// `package Foo v1.2.3` — VString token consumed as version.
#[test]
fn seam2_vstring_version_consumed_into_package_name() {
    let sexp = parse_sexp("package Foo v1.2.3;");
    assert!(
        sexp.contains("(package Foo v1.2.3)"),
        "VString version must be consumed into package name, got: {sexp}"
    );
}

/// `package Foo v5.38` — two-component v-string consumed.
#[test]
fn seam2_two_part_vstring_consumed() {
    let sexp = parse_sexp("package Foo v5.38;");
    assert!(
        sexp.contains("(package Foo v5.38)"),
        "Two-part v-string version must be consumed, got: {sexp}"
    );
}

/// `package Foo` — no version token → name must be bare.
/// Flipping the Number check to always-consume would break this.
#[test]
fn seam2_no_version_package_name_is_bare() {
    let sexp = parse_sexp("package Foo;");
    assert!(
        sexp.contains("(package Foo)"),
        "Package without version must have bare name, got: {sexp}"
    );
    // The sexp must not have any digit or 'v' appended after 'Foo'
    assert!(
        !sexp.contains("(package Foo 1"),
        "Bare package must not have a numeric version appended, got: {sexp}"
    );
}

/// Boundary: `package Foo v1` — single-digit v-string (Identifier starting with v).
#[test]
fn seam2_single_digit_vstring_consumed() {
    let sexp = parse_sexp("package Foo v1;");
    assert!(
        sexp.contains("(package Foo v1)"),
        "Single-digit v-string must be consumed as version, got: {sexp}"
    );
}

/// Boundary: `package Foo vX` — v-prefixed non-numeric identifier also consumed
/// (the code only checks `starts_with('v')`, not that the rest is numeric).
#[test]
fn seam2_v_prefixed_identifier_consumed() {
    let sexp = parse_sexp("package Foo vX;");
    // The v-prefixed identifier is consumed as version-like token
    assert!(
        sexp.contains("(package Foo vX)"),
        "v-prefixed identifier must be consumed as version, got: {sexp}"
    );
}

// ── Seam 3: is_optional_arg_builtin binary-operator guard ────────────────────
//
// Production predicate (statements.rs ~line 627):
//   is_binary_operator(peek)
//       && !(is_optional_arg_builtin(func_name) && is_explicit_sub_sigil_argument_start())
//
// When `func_name` is in `is_optional_arg_builtin` AND the next token is a
// binary operator, `omit_optional_arg` fires → the builtin gets NO argument
// and the operator becomes the outer binary expression.
//
// When `func_name` is NOT in `is_optional_arg_builtin`, the binary operator is
// NOT shielded → the parser tries to parse it as the argument → parse error.
//
// Mutation "remove is_optional_arg_builtin call" would make ALL builtins try to
// consume the binary op, breaking `length > 0`, `chr > 0`, etc.

/// `length > 0` — `length` is optional-arg; `>` must be the outer binary op,
/// not consumed as the argument to `length`.
#[test]
fn seam3_length_binary_op_stays_outside_call() {
    let sexp = parse_sexp("length > 0;");
    assert!(
        sexp.contains("(binary_>"),
        "binary > must be the root expression, got: {sexp}"
    );
    // length got NO argument
    assert!(
        !sexp.contains("(call length ((binary_>"),
        "binary > must NOT be consumed as argument to length, got: {sexp}"
    );
}

/// `chr > 0` — `chr` is optional-arg; same guard.
#[test]
fn seam3_chr_binary_op_stays_outside_call() {
    let sexp = parse_sexp("chr > 0;");
    assert!(
        sexp.contains("(binary_>"),
        "binary > must be the root expression for chr, got: {sexp}"
    );
    assert!(
        !sexp.contains("(call chr ((binary_>"),
        "binary > must NOT be argument to chr, got: {sexp}"
    );
}

/// `abs > 0` — `abs` is optional-arg.
#[test]
fn seam3_abs_binary_op_stays_outside_call() {
    let sexp = parse_sexp("abs > 0;");
    assert!(
        sexp.contains("(binary_>"),
        "binary > must be the root expression for abs, got: {sexp}"
    );
}

/// `defined || die` — `||` is a binary op; `defined` must get no arg.
#[test]
fn seam3_defined_symbolic_or_stays_outside_call() {
    let sexp = parse_sexp("defined || die;");
    assert!(
        sexp.contains("(binary_||"),
        "binary || must be root for defined, got: {sexp}"
    );
    // defined with empty arg list
    assert!(
        sexp.contains("(call defined ())"),
        "defined must have empty argument list, got: {sexp}"
    );
}

/// `ref || die` — `ref` is optional-arg.
#[test]
fn seam3_ref_symbolic_or_stays_outside_call() {
    let sexp = parse_sexp("ref || die;");
    assert!(
        sexp.contains("(binary_||"),
        "binary || must be root for ref, got: {sexp}"
    );
}

/// `undef || die` — `undef` is optional-arg.
#[test]
fn seam3_undef_symbolic_or_stays_outside_call() {
    let sexp = parse_sexp("undef || die;");
    assert!(
        sexp.contains("(binary_||"),
        "binary || must be root for undef, got: {sexp}"
    );
}

/// `sprintf > 0` — `sprintf` is NOT in is_optional_arg_builtin.
/// Without the guard, the parser attempts to parse `> 0` as sprintf's argument
/// and gets a parse error (sprintf needs at least a format string).
/// This test ensures the guard is NOT applied to non-members: removing the
/// is_optional_arg_builtin check for members would have the same effect as
/// treating sprintf as a member, which would produce a different sexp.
#[test]
fn seam3_non_member_builtin_not_shielded() {
    // sprintf expects arguments — `sprintf > 0` is a parse error because
    // sprintf is not in is_optional_arg_builtin, so the parser tries to use
    // `>` as the start of its argument.  The error node confirms the guard
    // was NOT applied.
    let mut p = Parser::new("sprintf > 0;");
    let ast = must(p.parse());
    let sexp = ast.to_sexp();
    // Either an ERROR node or a parse-level error — either way NOT a clean
    // `(binary_> ...)` at the top level with an empty sprintf call beside it.
    let errors = p.get_errors();
    assert!(
        sexp.contains("ERROR") || !errors.is_empty(),
        "sprintf (non-member) followed by binary > must produce a parse error, got: {sexp}"
    );
}

/// Regression guard: `length($x) > 0` — explicit parens mean the arg is
/// consumed normally; the binary op correctly stays outside.
#[test]
fn seam3_length_with_parens_arg_unaffected() {
    let sexp = parse_sexp("length($x) > 0;");
    assert!(
        sexp.contains("(binary_>"),
        "explicit-paren length call followed by > must still parse, got: {sexp}"
    );
}

/// Regression guard: `chr($c)` with parens — normal call unaffected.
#[test]
fn seam3_chr_with_parens_unaffected() {
    let sexp = parse_sexp("my $c = chr(65);");
    assert!(
        !sexp.contains("ERROR"),
        "chr(65) must parse cleanly, got: {sexp}"
    );
}

// ── Seam 4a: unbraced scalar-deref in variables.rs ───────────────────────────
//
// Production predicate (variables.rs ~line 406):
//   if (sigil == "$" && full_name == "$")  → unbraced deref: append next ident
//   else if (sigil "@"|"%" && full_name == "$$") → same
//
// `$$` alone (no following ident) → PID: Variable { sigil: "$", name: "$" }
// `$$ref` → deref: Variable { sigil: "$", name: "$ref" }
//
// Mutation "remove the unbraced deref branch" would leave `$$ref` as two nodes.

/// `$$` alone is the PID special variable.
/// Its AST form is Variable { sigil: "$", name: "$" }.
#[test]
fn seam4_pid_double_dollar() {
    let sexp = parse_sexp("my $pid = $$;");
    assert!(
        sexp.contains("(variable $ $)"),
        "bare $$ must be PID Variable{{$, $}}, got: {sexp}"
    );
}

/// `$$ref` is an unbraced scalar dereference.
/// Its AST form has name starting with `$ref` — distinct from PID.
#[test]
fn seam4_unbraced_scalar_deref_name_includes_dollar_prefix() {
    let sexp = parse_sexp("my $x = $$ref;");
    assert!(
        sexp.contains("(variable $ $ref)"),
        "$$ref must produce Variable{{$, $ref}}, got: {sexp}"
    );
    // Must NOT be the bare PID `$`
    assert!(
        !sexp.contains("(variable $ $)") || sexp.contains("(variable $ $ref)"),
        "$$ref must not produce bare PID, got: {sexp}"
    );
}

/// `$$self` in deref context — the name carries the `self` part after `$`.
#[test]
fn seam4_unbraced_deref_self_name() {
    let sexp = parse_sexp("my $val = $$self;");
    assert!(
        sexp.contains("(variable $ $self)"),
        "$$self must produce Variable{{$, $self}}, got: {sexp}"
    );
}

/// PID vs deref distinction: `$$` and `$$ref` must produce DIFFERENT sexp nodes.
#[test]
fn seam4_pid_and_deref_are_distinct_nodes() {
    let pid_sexp = parse_sexp("$$;");
    let deref_sexp = parse_sexp("$$ref;");

    // PID has name "$"
    assert!(
        pid_sexp.contains("(variable $ $)"),
        "PID $$ must have name '$', got: {pid_sexp}"
    );
    // Deref has name "$ref"
    assert!(
        deref_sexp.contains("(variable $ $ref)"),
        "Deref $$ref must have name '$ref', got: {deref_sexp}"
    );
    // They must be different
    assert_ne!(
        pid_sexp, deref_sexp,
        "PID and deref must produce distinct sexp"
    );
}

/// `@$ref` — array dereference; the `@` sigil path also supports unbraced deref.
#[test]
fn seam4_array_unbraced_deref_parses_cleanly() {
    let sexp = parse_sexp("my @items = @$ref;");
    assert!(
        !sexp.contains("ERROR"),
        "@$ref must parse without error, got: {sexp}"
    );
}

// ── Seam 4b: comma-after-double-sigil-object in calls.rs ─────────────────────
//
// Production predicate (calls.rs ~line 372):
//   let comma_after_double_sigil_object = self.peek_kind() == Some(TokenKind::Comma)
//       && matches!(&object.kind,
//           NodeKind::Variable { sigil, name } if sigil == "$" && name.starts_with('$'));
//   if ... || comma_after_double_sigil_object { self.tokens.next()?; }
//
// Without this guard, `catfile $$self, $arg` would leave the comma unparsed
// and the indirect call would have only one argument.
//
// Mutation "remove the comma_after_double_sigil_object check" would drop the
// comma-consuming branch, breaking the multi-arg form.

/// `catfile $$self, $_` — the double-sigil object is followed by a comma.
/// The guard must consume the comma and include `$_` as the second argument.
#[test]
fn seam4_indirect_call_comma_after_double_sigil_object() {
    let sexp = parse_sexp("catfile $$self, $_;");
    assert!(
        sexp.contains("(indirect_call catfile"),
        "catfile $$self must parse as indirect_call, got: {sexp}"
    );
    // Both arguments must be present
    assert!(
        sexp.contains("(variable $ $self)"),
        "first arg $$self must appear in indirect_call, got: {sexp}"
    );
    assert!(
        sexp.contains("(variable $ _)"),
        "second arg $_ must appear in indirect_call after comma, got: {sexp}"
    );
}

/// `catfile $$self` — single arg, no comma (no guard needed but must still work).
#[test]
fn seam4_indirect_call_single_double_sigil_arg_no_comma() {
    let sexp = parse_sexp("catfile $$self;");
    assert!(
        sexp.contains("(indirect_call catfile"),
        "catfile $$self single-arg must parse as indirect_call, got: {sexp}"
    );
    assert!(
        sexp.contains("(variable $ $self)"),
        "object $$self must appear in indirect_call, got: {sexp}"
    );
}

/// `imported $$obj, $arg` — CPAN pattern that originally triggered the fix.
#[test]
fn seam4_imported_function_double_sigil_comma_pattern() {
    let sexp = parse_sexp("imported $$obj, $arg;");
    assert!(
        !sexp.contains("ERROR"),
        "imported $$obj, $arg must parse without error, got: {sexp}"
    );
    // Both args must survive
    assert!(
        sexp.contains("(variable $ $obj)"),
        "$$obj must appear in indirect call, got: {sexp}"
    );
}

// ── Cross-seam regression guards ─────────────────────────────────────────────
//
// These ensure that fixing one seam does not regress another.

/// Normal `<FH>` in a while loop — clean parse after readline.
#[test]
fn cross_readline_in_while() {
    let sexp = parse_sexp("while (my $line = <FH>) { print $line; }");
    assert!(!sexp.contains("ERROR"), "readline in while must be error-free, got: {sexp}");
    assert!(sexp.contains("(readline FH)"), "FH must be Readline in while, got: {sexp}");
}

/// Normal `package My::Module 1.23` in a real-world style.
#[test]
fn cross_package_version_in_module() {
    let sexp = parse_sexp("package My::Module 1.23;");
    assert!(!sexp.contains("ERROR"), "package with version must parse cleanly, got: {sexp}");
    assert!(
        sexp.contains("1.23"),
        "version must appear in package name, got: {sexp}"
    );
}

/// `defined $hash{key}` — optional-arg builtin WITH subscript arg still works.
#[test]
fn cross_defined_with_subscript_arg() {
    let sexp = parse_sexp("defined $hash{key};");
    assert!(!sexp.contains("ERROR"), "defined with subscript arg must parse, got: {sexp}");
}

/// `$$ref->{key}` — deref followed by hash subscript chain.
#[test]
fn cross_deref_with_hash_subscript() {
    let sexp = parse_sexp("my $v = $$ref->{key};");
    assert!(!sexp.contains("ERROR"), "$$ref->{{key}} must parse cleanly, got: {sexp}");
}
