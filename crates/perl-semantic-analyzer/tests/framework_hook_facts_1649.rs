//! Regression tests for #1649: builder hooks are not provided generated members.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::generated_member_extractor::GeneratedMemberExtractor;
use perl_semantic_analyzer::analysis::symbol::{SymbolExtractor, SymbolKind};
use perl_semantic_facts::{AttributeMethodHookKind, FileId, GeneratedMemberKind};

#[test]
fn lazy_builder_hook_is_not_a_generated_member() -> Result<(), String> {
    let code = "package MyApp::Config;\nuse Moo;\nhas 'data' => (is => 'lazy');\n1;";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|e| format!("parse failed: {e:?}"))?;

    let members = GeneratedMemberExtractor::extract(&ast, "MyApp::Config", FileId(1));
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].name, "data");
    assert_eq!(members[0].kind, GeneratedMemberKind::Getter);

    let hooks = GeneratedMemberExtractor::extract_hooks(&ast, "MyApp::Config", FileId(1));
    let hook =
        hooks.iter().find(|h| h.method_name == "_build_data").ok_or("missing lazy builder hook")?;
    assert_eq!(hook.kind, AttributeMethodHookKind::Builder);
    assert!(!hook.explicit_name);
    Ok(())
}

#[test]
fn builder_hook_name_is_not_synthesized_as_has_subroutine() -> Result<(), String> {
    let code = "package MyApp::Config;\nuse Moo;\nhas 'data' => (is => 'lazy');\n1;";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|e| format!("parse failed: {e:?}"))?;
    let table = SymbolExtractor::new().extract(&ast);

    let subs: Vec<_> = table
        .symbols
        .get("_build_data")
        .map(|entries| entries.iter().collect())
        .unwrap_or_default();
    assert!(
        subs.is_empty(),
        "builder hook must not become a synthesized subroutine symbol: {subs:?}"
    );

    let data_symbols: Vec<_> =
        table.symbols.get("data").map(|entries| entries.iter().collect()).unwrap_or_default();
    assert!(
        data_symbols
            .iter()
            .any(|sym| sym.declaration.as_deref() == Some("has")
                && sym.kind != SymbolKind::Subroutine),
        "expected attribute symbol for data: {data_symbols:?}"
    );
    assert!(
        data_symbols
            .iter()
            .any(|sym| { sym.attributes.iter().any(|tag| tag == "builder=_build_data") }),
        "attribute metadata should retain lazy builder hook: {data_symbols:?}"
    );
    Ok(())
}
