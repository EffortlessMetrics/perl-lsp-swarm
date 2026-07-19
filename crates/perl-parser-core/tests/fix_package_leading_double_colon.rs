mod cpan_test_helpers;
use cpan_test_helpers::*;

// Issue #2834: package declaration with a leading :: qualifier.
// In Perl, `package ::My::App;` declares a package in the main namespace
// (verified against perl 5.38.2: `perl -ce 'package ::My::App; 1;'` is OK).
// parse_subroutine already handles `sub ::PCDATA { }`; parse_package did not,
// rejecting the leading `::` with "expected identifier, found '::'".

/// Extract the package name from the first `Package` node in a parsed program.
fn package_name(source: &str) -> Option<String> {
    let ast = parse(source);
    match &ast.kind {
        perl_parser_core::NodeKind::Program { statements } => {
            for stmt in statements {
                let node = match &stmt.kind {
                    perl_parser_core::NodeKind::ExpressionStatement { expression } => expression,
                    _ => stmt,
                };
                if let perl_parser_core::NodeKind::Package { name, .. } = &node.kind {
                    return Some(name.clone());
                }
            }
            None
        }
        _ => None,
    }
}

#[test]
fn test_package_leading_double_colon_statement() {
    assert_clean_parse(r#"package ::My::App;"#);
    assert_eq!(package_name(r#"package ::My::App;"#), Some("::My::App".to_string()));
}

#[test]
fn test_package_leading_double_colon_block() {
    assert_clean_parse(r#"package ::My::App { our $x = 1; }"#);
    assert_eq!(package_name(r#"package ::My::App { our $x = 1; }"#), Some("::My::App".to_string()),);
}

#[test]
fn test_package_leading_double_colon_single_segment() {
    assert_clean_parse(r#"package ::Foo;"#);
    assert_eq!(package_name(r#"package ::Foo;"#), Some("::Foo".to_string()));
}

#[test]
fn test_package_leading_double_colon_deeply_qualified() {
    assert_clean_parse(r#"package ::DB_File::Foo::Bar;"#);
    assert_eq!(
        package_name(r#"package ::DB_File::Foo::Bar;"#),
        Some("::DB_File::Foo::Bar".to_string()),
    );
}

#[test]
fn test_package_without_leading_colon_still_parses() {
    // Regression guard: ordinary package declarations must be unchanged.
    assert_clean_parse(r#"package My::App;"#);
    assert_eq!(package_name(r#"package My::App;"#), Some("My::App".to_string()));
}

#[test]
fn test_package_leading_double_colon_with_version() {
    // The leading-:: name must not swallow a following version number.
    assert_clean_parse(r#"package ::My::App 1.23;"#);
    assert_eq!(package_name(r#"package ::My::App 1.23;"#), Some("::My::App 1.23".to_string()),);
}
