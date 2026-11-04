//! Representative declaration-bank tests used by workspace parity checks.
//!
//! These fixtures intentionally use real parser input (not hand-built AST nodes)
//! so `extract_symbol_decls()` behavior is locked against concrete Perl snippets.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use anyhow::Result;
use perl_parser_core::Parser;
use perl_symbol::SymbolKind;
use perl_symbol::extract_symbol_decls;

struct SurfaceCase {
    label: &'static str,
    src: &'static str,
    expected_names: &'static [&'static str],
}

fn parse(src: &str) -> Result<perl_parser_core::Node> {
    let mut parser = Parser::new(src);
    Ok(parser.parse()?)
}

fn assert_parsed_wrapper_initializer(source: &str) -> Result<()> {
    let ast = parse(source)?;
    fn has_wrapper_initializer(node: &perl_parser_core::Node) -> bool {
        if let perl_ast::NodeKind::FunctionCall { name, args } = &node.kind
            && matches!(name.as_str(), "const" | "Readonly")
            && args.iter().any(|arg| {
                matches!(
                    &arg.kind,
                    perl_ast::NodeKind::VariableDeclaration { initializer: Some(_), .. }
                        | perl_ast::NodeKind::VariableListDeclaration { initializer: Some(_), .. }
                )
            })
        {
            return true;
        }
        let mut found = false;
        node.for_each_child(|child| found |= has_wrapper_initializer(child));
        found
    }
    assert!(has_wrapper_initializer(&ast), "fixture must exercise a wrapper initializer");
    let declarations = extract_symbol_decls(&ast, Some("main"));
    assert_eq!(declarations.len(), 2, "one constant and one nested named sub");
    assert_eq!(declarations[0].kind, SymbolKind::Constant);
    assert_eq!(declarations[0].name, "X");
    assert_eq!(declarations[0].anchor_span, Some((26, 28)));
    assert_eq!(declarations[1].kind, SymbolKind::Subroutine);
    assert_eq!(declarations[1].name, "ub");
    assert_eq!(declarations[1].qualified_name, "main::ub");
    assert_eq!(declarations[1].full_span, (37, 46));
    assert_eq!(declarations[1].anchor_span, Some((41, 43)));
    Ok(())
}

#[test]
fn const_fast_real_parser_preserves_initializer_declarations() -> Result<()> {
    assert_parsed_wrapper_initializer("use Const::Fast; const my $X = sub { sub ub {} };")
}

#[test]
fn readonly_real_parser_preserves_initializer_declarations() -> Result<()> {
    assert_parsed_wrapper_initializer("use Readonly; Readonly my $X = sub { sub ub {} };")
}

#[test]
fn surface_bank_covers_representative_workspace_parity_constructs() -> Result<()> {
    let cases = vec![
        SurfaceCase {
            label: "package + sub",
            src: "package My::Pkg; sub run { 1 }",
            expected_names: &["My::Pkg", "run"],
        },
        SurfaceCase {
            label: "method in class",
            src: "class My::Class { method tick () { 1 } }",
            expected_names: &["My::Class", "tick"],
        },
        SurfaceCase { label: "my scalar", src: "my $count = 1;", expected_names: &["count"] },
        SurfaceCase { label: "our hash", src: "our %CONFIG = ();", expected_names: &["CONFIG"] },
        SurfaceCase {
            label: "array list declaration",
            src: "my ($x, @vals, %opts) = (1);",
            expected_names: &["x", "vals", "opts"],
        },
        SurfaceCase {
            label: "use constant",
            src: "package C; use constant PI => 3.14;",
            expected_names: &["C", "PI"],
        },
        SurfaceCase {
            label: "Const::Fast",
            src: "use Const::Fast; const my $MAX => 3;",
            expected_names: &["MAX"],
        },
        SurfaceCase {
            label: "Readonly",
            src: "use Readonly; Readonly my $NAME => 'n';",
            expected_names: &["NAME"],
        },
        SurfaceCase {
            // `state` is a Perl 5.10+ declarator (like `my` but lexically
            // scoped to the sub across invocations). extract_symbol_decls
            // must recognise it as a variable declaration.
            label: "state variable",
            src: "use feature 'state'; sub counter { state $count = 0; $count }",
            expected_names: &["count"],
        },
    ];

    for case in cases {
        let ast = parse(case.src)?;
        let decls = extract_symbol_decls(&ast, Some("main"));
        for expected in case.expected_names {
            assert!(
                decls.iter().any(|decl| decl.name == *expected),
                "case '{}' expected '{}' in {:?}",
                case.label,
                expected,
                decls.iter().map(|decl| (&decl.name, &decl.kind)).collect::<Vec<_>>()
            );
        }
    }

    Ok(())
}

#[test]
fn surface_bank_tracks_constant_kind_for_constant_sources() -> Result<()> {
    let ast = parse(
        "package K; use constant RATE => 1; use Const::Fast; const my $FLAG => 1; use Readonly; Readonly my $NAME => 'x';",
    )?;
    let decls = extract_symbol_decls(&ast, Some("main"));

    for name in ["RATE", "FLAG", "NAME"] {
        let decl = decls
            .iter()
            .find(|decl| decl.name == name)
            .ok_or_else(|| anyhow::anyhow!("missing constant declaration for {name}"))?;
        assert_eq!(decl.kind, SymbolKind::Constant, "{name} should be Constant kind");
    }

    Ok(())
}
