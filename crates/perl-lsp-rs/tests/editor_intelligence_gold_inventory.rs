//! Temporary gold-corpus routing bridge for completion scope evidence.
//!
//! The executable scorecard discovers fixtures under `test_corpus/gold/` at
//! runtime, but current typed CI scope does not project fixture-only changes
//! onto the `perl-lsp-rs` test crate. Keep these exact controls named here so
//! PR #13145 both selects the current package lane and proves the new sidecars
//! are discoverable with their load-bearing assertions.
//!
//! This is not another corpus registry or denominator authority. Issue #13168
//! owns durable external-input routing; issue #13454 owns generated assertion-
//! mode witness coverage. Retire or absorb this bridge when those owners land.

use perl_corpus::gold::{
    CompletionAssertionKind, CompletionGoldFixture, load_completion_gold_fixtures,
};
use std::path::PathBuf;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn gold_corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_corpus/gold")
}

fn fixture_named<'a>(
    fixtures: &'a [CompletionGoldFixture],
    name: &str,
) -> Result<&'a CompletionGoldFixture, std::io::Error> {
    fixtures.iter().find(|fixture| fixture.name == name).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("completion gold fixture '{name}' was not discovered"),
        )
    })
}

#[test]
fn completion_scope_controls_are_in_the_scorecard_denominator() -> TestResult {
    let fixtures = load_completion_gold_fixtures(gold_corpus_root())?;

    let sibling = fixture_named(&fixtures, "completion_scope_sibling")?;
    assert!(
        sibling.completion_assertions.iter().any(|assertion| {
            assertion.line == 5
                && assertion.character == 11
                && matches!(
                    &assertion.kind,
                    CompletionAssertionKind::CompletionPresent { expected_label }
                        if expected_label == "$sib_level_top"
                )
        }),
        "sibling-scope fixture must keep its visible ancestor positive control"
    );
    assert!(
        sibling.completion_assertions.iter().any(|assertion| {
            assertion.line == 5
                && assertion.character == 11
                && matches!(
                    &assertion.kind,
                    CompletionAssertionKind::CompletionNoiseAbsent { forbidden_label }
                        if forbidden_label == "$sib_left"
                )
        }),
        "sibling-scope fixture must reject the ended sibling lexical"
    );

    let ranking = fixture_named(&fixtures, "completion_scope_ranking")?;
    assert!(
        ranking.completion_assertions.iter().any(|assertion| {
            assertion.line == 6
                && assertion.character == 19
                && matches!(
                    &assertion.kind,
                    CompletionAssertionKind::CompletionTop1 { expected_label }
                        if expected_label == "$scope_z_local"
                )
        }),
        "scope-ranking fixture must require the immediate lexical at Top-1"
    );
    assert!(
        ranking.completion_assertions.iter().any(|assertion| {
            assertion.line == 6
                && assertion.character == 19
                && matches!(
                    &assertion.kind,
                    CompletionAssertionKind::CompletionTop5 { expected_label }
                        if expected_label == "$scope_a_ancestor"
                )
        }),
        "scope-ranking fixture must keep the visible ancestor in Top-5"
    );
    assert!(
        ranking.completion_assertions.iter().any(|assertion| {
            assertion.line == 6
                && assertion.character == 19
                && matches!(
                    &assertion.kind,
                    CompletionAssertionKind::CompletionNoiseAbsent { forbidden_label }
                        if forbidden_label == "$scope_0_sibling"
                )
        }),
        "scope-ranking fixture must reject the ended sibling lexical"
    );
    assert!(
        ranking.completion_assertions.iter().any(|assertion| {
            assertion.line == 6
                && assertion.character == 19
                && matches!(
                    &assertion.kind,
                    CompletionAssertionKind::CompletionNoiseAbsent { forbidden_label }
                        if forbidden_label == "$scope_1_future"
                )
        }),
        "scope-ranking fixture must reject the later declaration before its program point"
    );

    Ok(())
}
