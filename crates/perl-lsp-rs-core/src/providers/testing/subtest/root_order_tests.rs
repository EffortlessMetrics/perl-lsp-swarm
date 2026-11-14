//! Source ordering at the outline root is distinct from child priority ordering.

use super::{DocumentSymbol, discover_subtests, nest_subtest_symbols_in_outline};

fn outline(source: &str) -> anyhow::Result<Vec<DocumentSymbol>> {
    let mut parser = perl_parser::Parser::new(source);
    let ast = parser.parse().map_err(|error| anyhow::anyhow!("ordering fixture: {error}"))?;
    let mut symbols =
        crate::providers::document_symbols::source_backed_document_symbols_from_ast(&ast, source)
            .symbols;
    let subtests = discover_subtests(&ast, source);
    anyhow::ensure!(!subtests.is_empty(), "fixture must exercise subtest insertion");
    nest_subtest_symbols_in_outline(&mut symbols, &subtests, source);
    Ok(symbols)
}

fn ensure_names(symbols: &[DocumentSymbol], expected: &[&str]) -> anyhow::Result<()> {
    let actual: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
    anyhow::ensure!(actual == expected, "expected outline {expected:?}, got {actual:?}");
    Ok(())
}

#[test]
fn root_collections_keep_source_order_when_subtest_is_inserted() -> anyhow::Result<()> {
    let source = "my @items = ();\n\
        my %items_by_name = ();\n\
        subtest 'middle' => sub { ok(1); };\n\
        sub later { return 1; }\n";
    let symbols = outline(source)?;
    ensure_names(&symbols, &["@items", "%items_by_name", "middle", "later"])?;
    let inserted = symbols.get(2).ok_or_else(|| anyhow::anyhow!("missing inserted subtest"))?;
    anyhow::ensure!(inserted.detail == "subtest", "middle must be the discovered subtest");
    Ok(())
}

#[test]
fn package_collections_keep_priority_order_when_subtest_is_inserted() -> anyhow::Result<()> {
    let source = "package Collections;\n\
        my @items = ();\n\
        my %items_by_name = ();\n\
        subtest 'middle' => sub { ok(1); };\n\
        sub later { return 1; }\n";
    let symbols = outline(source)?;
    let package = symbols
        .iter()
        .find(|symbol| symbol.name == "Collections")
        .ok_or_else(|| anyhow::anyhow!("package must own the same mixed siblings"))?;
    ensure_names(&package.children, &["middle", "later", "@items", "%items_by_name"])?;
    anyhow::ensure!(
        !symbols.iter().any(|symbol| symbol.detail == "subtest"),
        "package subtest must remain nested"
    );
    Ok(())
}

#[test]
fn root_order_uses_character_position_within_one_line() -> anyhow::Result<()> {
    let symbols = outline("my @items = (); subtest 'middle' => sub {}; sub later {}")?;
    ensure_names(&symbols, &["@items", "middle", "later"])?;
    anyhow::ensure!(symbols.iter().all(|symbol| symbol.range.start.line == 0), "single-line roots");
    for adjacent in symbols.windows(2) {
        let [left, right] = adjacent else { anyhow::bail!("expected adjacent symbol pair") };
        anyhow::ensure!(
            left.range.start.character < right.range.start.character,
            "root character positions must increase"
        );
    }
    Ok(())
}

#[test]
fn multiple_root_subtests_keep_their_positions_among_existing_symbols() -> anyhow::Result<()> {
    let source = "subtest 'first' => sub {};\n\
        my @items = ();\n\
        subtest 'middle' => sub {};\n\
        sub later {}\n\
        subtest 'last' => sub {};\n";
    let symbols = outline(source)?;
    ensure_names(&symbols, &["first", "@items", "middle", "later", "last"])?;
    let discovered = symbols.iter().filter(|symbol| symbol.detail == "subtest").count();
    anyhow::ensure!(discovered == 3, "expected three inserted subtests, got {discovered}");
    Ok(())
}
