//! Denominator-integrity contract for the editor intelligence scorecard.
//!
//! The shared gold loaders intentionally permit empty fixture selections and
//! empty assertion arrays. That is useful at the data layer, but a scorecard
//! must fail rather than report success when one of its measured feature
//! families or fixtures contributes no evidence.
//!
//! ## Verify
//!
//! ```bash
//! cargo test -p perl-lsp-rs --test editor_intelligence_scorecard_integrity
//! ```

use perl_corpus::gold::{
    load_completion_gold_fixtures, load_document_symbol_gold_fixtures, load_gold_fixtures,
    load_goto_gold_fixtures, load_hover_gold_fixtures, load_rename_gold_fixtures,
};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn gold_corpus_root() -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let crate_dir = PathBuf::from(manifest);
    let workspace_root = crate_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| crate_dir.clone());
    workspace_root.join("test_corpus").join("gold")
}

fn check_denominator<I>(
    failures: &mut Vec<String>,
    feature: &str,
    root: &Path,
    fixtures: I,
) where
    I: IntoIterator<Item = (String, usize)>,
{
    let mut fixture_count = 0usize;

    for (fixture_name, assertion_count) in fixtures {
        fixture_count += 1;
        if assertion_count == 0 {
            failures.push(format!(
                "{feature} scorecard fixture '{fixture_name}' has no assertions"
            ));
        }
    }

    if fixture_count == 0 {
        failures.push(format!(
            "{feature} scorecard denominator is empty: no fixtures loaded from {}",
            root.display()
        ));
    }
}

#[test]
fn editor_intelligence_scorecard_denominators_are_non_vacuous() -> TestResult {
    let root = gold_corpus_root();
    let mut failures = Vec::new();

    let hover = load_hover_gold_fixtures(&root)?;
    check_denominator(
        &mut failures,
        "hover",
        &root,
        hover.into_iter().map(|fixture| {
            let assertion_count = fixture.hover_assertions.len();
            (fixture.name, assertion_count)
        }),
    );

    let goto = load_goto_gold_fixtures(&root)?;
    check_denominator(
        &mut failures,
        "go-to-definition",
        &root,
        goto.into_iter().map(|fixture| {
            let assertion_count = fixture.goto_assertions.len();
            (fixture.name, assertion_count)
        }),
    );

    let completion = load_completion_gold_fixtures(&root)?;
    check_denominator(
        &mut failures,
        "completion",
        &root,
        completion.into_iter().map(|fixture| {
            let assertion_count = fixture.completion_assertions.len();
            (fixture.name, assertion_count)
        }),
    );

    let diagnostics = load_gold_fixtures(&root)?;
    check_denominator(
        &mut failures,
        "diagnostics",
        &root,
        diagnostics.into_iter().map(|fixture| {
            let assertion_count = fixture.expected.diagnostics.len();
            (fixture.name, assertion_count)
        }),
    );

    let document_symbols = load_document_symbol_gold_fixtures(&root)?;
    check_denominator(
        &mut failures,
        "document symbols",
        &root,
        document_symbols.into_iter().map(|fixture| {
            let assertion_count = fixture.symbol_assertions.len();
            (fixture.name, assertion_count)
        }),
    );

    let rename = load_rename_gold_fixtures(&root)?;
    check_denominator(
        &mut failures,
        "rename",
        &root,
        rename.into_iter().map(|fixture| {
            let assertion_count = fixture.rename_assertions.len();
            (fixture.name, assertion_count)
        }),
    );

    assert!(
        failures.is_empty(),
        "editor intelligence scorecard has vacuous denominators:\n{}",
        failures.join("\n")
    );

    Ok(())
}
