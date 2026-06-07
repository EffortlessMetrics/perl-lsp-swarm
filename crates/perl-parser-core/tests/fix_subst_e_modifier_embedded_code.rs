mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::NodeKind;

/// Walk the AST and collect all Substitution nodes
fn collect_substitutions(node: &perl_parser_core::Node) -> Vec<(String, bool)> {
    let mut results = Vec::new();
    if let NodeKind::Substitution { modifiers, has_embedded_code, .. } = &node.kind {
        results.push((modifiers.clone(), *has_embedded_code));
    }
    for child in node.children() {
        results.extend(collect_substitutions(child));
    }
    results
}

#[test]
fn test_subst_e_modifier_sets_has_embedded_code() {
    let source = r#"$s =~ s/(\w+)/uc($1)/e;"#;
    let ast = parse(source);
    let subs = collect_substitutions(&ast);
    assert!(!subs.is_empty(), "expected at least one Substitution node");
    for (mods, has_code) in &subs {
        if mods.contains('e') {
            assert!(
                *has_code,
                "s///e with modifiers '{}' should have has_embedded_code=true",
                mods
            );
        }
    }
}

#[test]
fn test_subst_ee_modifier_sets_has_embedded_code() {
    let source = r#"$t =~ s/\$(\w+)/$$1/ee;"#;
    let ast = parse(source);
    let subs = collect_substitutions(&ast);
    assert!(!subs.is_empty(), "expected at least one Substitution node");
    for (mods, has_code) in &subs {
        if mods.contains('e') {
            assert!(
                *has_code,
                "s///ee with modifiers '{}' should have has_embedded_code=true",
                mods
            );
        }
    }
}

#[test]
fn test_subst_gi_modifier_does_not_set_has_embedded_code() {
    // s///gi should NOT set has_embedded_code (no 'e' modifier, no (?{...}))
    let source = r#"$s =~ s/foo/bar/gi;"#;
    let ast = parse(source);
    let subs = collect_substitutions(&ast);
    for (mods, has_code) in &subs {
        if !mods.contains('e') {
            assert!(
                !has_code,
                "s///gi with modifiers '{}' should NOT have has_embedded_code=true",
                mods
            );
        }
    }
}

#[test]
fn test_subst_embedded_code_in_pattern_still_works() {
    // (?{...}) in the pattern still triggers has_embedded_code
    let source = r#"$s =~ s/(?{1+1})/replacement/;"#;
    let ast = parse(source);
    let subs = collect_substitutions(&ast);
    assert!(!subs.is_empty(), "expected Substitution node");
    for (_, has_code) in &subs {
        assert!(*has_code, "s/// with (?{{...}}) pattern should have has_embedded_code=true");
    }
}

#[test]
fn test_subst_e_modifier_standalone_clean_parse() {
    // Ensure the expression parses cleanly (no Error nodes)
    assert_clean_parse(r#"$s =~ s/(\w+)/uc($1)/e;"#);
    assert_clean_parse(r#"$x =~ s/foo/lc($&)/ge;"#);
    assert_clean_parse(r#"$y =~ s/\$(\w+)/$$1/ee;"#);
}
