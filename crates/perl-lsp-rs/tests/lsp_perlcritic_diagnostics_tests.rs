//! Deprecated critic engine settings cannot arm an external Perl::Critic
//! subprocess on any migrated diagnostic transport (#9062).
//!
//! This suite previously exercised `collect_external_perlcritic_diagnostics`
//! end-to-end through a mock subprocess runtime. That production path no longer
//! exists: after the #9062 cutover, push, document-pull and workspace-pull
//! diagnostics all route from the accepted `EffectiveCriticState` through
//! `NativeCriticService`, and no diagnostic transport can launch `perlcritic`
//! or select `BuiltInAnalyzer`.
//!
//! The external-compatibility assertions those tests carried belong to the
//! repository conformance harness, which still owns real Perl::Critic
//! differential capability. What a *product* LSP test should prove now is the
//! negative: a deprecated `legacy` / `external` / `perlcritic` engine value is
//! inert migration input and cannot change the evaluator.
//!
//! Retirement of the raw settings themselves is #9072; deletion of the residual
//! `CriticEngine` / analyzer architecture is #9068. Neither is claimed here.
//!
//! Requires the `expose_lsp_test_api` feature and a non-WASM target.
//!
//! Issue: #2018 (original), #9062 (cutover)

#![cfg(all(not(target_arch = "wasm32"), feature = "expose_lsp_test_api"))]
// Tests are permitted to use `.expect()` on Result/Option per the repo's coding
// standards (unlike production code, where they are banned).
#![allow(clippy::expect_used)]

use perl_lsp::LspServer;
use perl_lsp_rs_core::config::CriticEngine;
use serde_json::json;

/// A source that a real Perl::Critic run would have flagged, so an armed
/// subprocess would be visible in the output.
const SOURCE: &str = "my $path = 'f.txt';\nsystem($path);\nopen(FH, '<', 'f.txt');\n";

fn server_with_engine(engine: CriticEngine, uri: &str) -> LspServer {
    let server = LspServer::new();
    server.test_configure_critic_engine(engine);
    server.test_configure_perlcritic(true, 3, None);
    server.test_configure_native_critic_profile("strict");
    server
        .test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": SOURCE
            }
        })))
        .expect("did_open should succeed");
    server
}

fn document_pull_text(engine: CriticEngine, uri: &str) -> String {
    let server = server_with_engine(engine, uri);
    let report = server
        .test_handle_document_diagnostic(Some(json!({ "textDocument": { "uri": uri } })))
        .expect("document diagnostic should succeed")
        .unwrap_or_default();
    report.to_string()
}

fn workspace_pull_text(engine: CriticEngine, uri: &str) -> String {
    let server = server_with_engine(engine, uri);
    let report = server
        .test_handle_workspace_diagnostic(Some(json!({ "previousResultIds": [] })))
        .expect("workspace diagnostic should succeed")
        .unwrap_or_default();
    report.to_string()
}

/// The external tool's brand and policy-name shape. A subprocess result would
/// carry `Perl::Critic` as the diagnostic source and `Foo::Bar`-style policy
/// names; native rows carry `perl-lsp` and `native.*` codes.
fn assert_no_external_rows(text: &str, transport: &str) {
    assert!(
        !text.contains("Perl::Critic"),
        "{transport}: a deprecated engine value must not produce externally branded rows; \
         got: {text}"
    );
    assert!(
        !text.contains("TestingAndDebugging::"),
        "{transport}: a deprecated engine value must not produce external policy names; \
         got: {text}"
    );
    assert!(
        !text.contains("InputOutput::"),
        "{transport}: a deprecated engine value must not produce external policy names; \
         got: {text}"
    );
}

#[test]
fn deprecated_engine_cannot_arm_the_subprocess_on_document_pull() {
    let text = document_pull_text(CriticEngine::Legacy, "file:///deprecated_doc_pull.pl");
    assert_no_external_rows(&text, "document pull");
}

#[test]
fn deprecated_engine_cannot_arm_the_subprocess_on_workspace_pull() {
    let text = workspace_pull_text(CriticEngine::Legacy, "file:///deprecated_ws_pull.pl");
    assert_no_external_rows(&text, "workspace pull");
}

/// The stronger proposition: the deprecated value is not merely harmless, it is
/// inert. Document-pull output must be identical whichever engine value is set,
/// because routing derives from the accepted state and never from the raw
/// setting.
#[test]
fn deprecated_engine_value_does_not_change_document_pull_output() {
    let uri = "file:///deprecated_engine_equivalence.pl";
    let deprecated = document_pull_text(CriticEngine::Legacy, uri);
    let native = document_pull_text(CriticEngine::Native, uri);
    assert_eq!(
        deprecated, native,
        "a deprecated raw engine value must not change document-pull output"
    );
}

#[test]
fn deprecated_engine_value_does_not_change_workspace_pull_output() {
    let uri = "file:///deprecated_engine_equivalence_ws.pl";
    let deprecated = workspace_pull_text(CriticEngine::Legacy, uri);
    let native = workspace_pull_text(CriticEngine::Native, uri);
    assert_eq!(
        deprecated, native,
        "a deprecated raw engine value must not change workspace-pull output"
    );
}

/// Native evaluation must still be reachable through the migrated transport, so
/// the assertions above cannot pass merely because nothing ran.
#[test]
fn the_native_service_still_produces_rows_on_document_pull() {
    let text = document_pull_text(CriticEngine::Native, "file:///native_still_runs.pl");
    assert!(
        text.contains("native."),
        "the native service must still supply rows through document pull; got: {text}"
    );
}
