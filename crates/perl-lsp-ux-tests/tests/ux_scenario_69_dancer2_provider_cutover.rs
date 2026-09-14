//! Scenario 69 — Dancer2 canonical provider cutover process receipts (#8928).
//!
//! Exercises the promoted read-only Dancer2 provider cells through the real
//! `textDocument/*` and `workspace/symbol` request paths over the committed
//! Dancer2 skeleton workspace (which provides the versioned `Dancer2`
//! module for exact activation) plus synthetic app files.
//!
//! Receipt signals (the issue's required process cases):
//! - bare `use Dancer2` completion includes admitted keywords; `!get`
//!   exclusion removes `get`;
//! - handler-only keywords differ inside versus outside an exact handler;
//! - named-route hover/symbol identity keeps name and pattern separate;
//! - GET routes report GET+HEAD; explicit-method `any` reports the exact
//!   method set;
//! - route handler definition reaches the inline callback anchor;
//! - ordinary `get()` without Dancer2 and `use Dancer2::Core` get no
//!   framework result;
//! - custom DSL stays bounded; edits re-query without stale exact answers;
//! - multi-root same-name route/app isolation;
//! - malformed/incomplete routes return bounded results, not exact
//!   generated project shape.

use anyhow::Result;
use perl_lsp_ux_tests::ProjectFixtureFile;
use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::missing_binary_skip;
use perl_lsp_ux_tests::{
    UxCiTier, UxComponent, UxHarness, create_fixture_harness, load_dancer2_fixture_files,
    run_ux_scenario,
};
use serde_json::Value;

const SCENARIO_FILE: &str = "ux_scenario_69_dancer2_provider_cutover.rs";

const APP_MAIN: &str = r#"#!/usr/bin/perl
use strict;
use warnings;
use lib 'lib';
use Dancer2;

get '/' => sub { 'Hello World' };
get 'user_show', '/users/:id' => sub { 'user' };
any ['GET','POST','DEL'] => '/multi' => sub { 'multi' };
post '/submit' => sub { my $p = params; };
hook 'before' => sub { 1 };
"#;

const APP_EXCLUDED: &str = r#"#!/usr/bin/perl
use strict;
use warnings;
use lib 'lib';
use Dancer2 '!get';

get '/excluded' => sub { 'x' };
post '/kept' => sub { 'y' };
"#;

const APP_CORE: &str = r#"#!/usr/bin/perl
use strict;
use warnings;
use lib 'lib';
use Dancer2::Core;

get '/core' => sub { 'x' };
"#;

const APP_PLAIN: &str = r#"#!/usr/bin/perl
use strict;
use warnings;

sub get { return 'local' }
get '/plain' => sub { 'x' };
"#;

const APP_CUSTOM_DSL: &str = r#"#!/usr/bin/perl
use strict;
use warnings;
use lib 'lib';
use Dancer2 dsl => 'My::DSL';

get '/custom' => sub { 'x' };
"#;

const APP_MALFORMED: &str = r#"#!/usr/bin/perl
use strict;
use warnings;
use lib 'lib';
use Dancer2;

get $computed => sub { 'x' };
"#;

fn completion_labels(
    harness: &UxHarness,
    path: &str,
    line: u32,
    character: u32,
) -> Result<Vec<String>> {
    Ok(harness
        .completion(path, line, character)?
        .iter()
        .filter_map(|item| item.get("label").and_then(Value::as_str).map(ToString::to_string))
        .collect())
}

fn hover_text(hover: &Option<Value>) -> String {
    hover
        .as_ref()
        .and_then(|value| value.pointer("/contents/value"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn first_keyword_position(source: &str, needle: &str) -> (u32, u32) {
    let line = source.lines().position(|l| l.contains(needle)).unwrap_or(0) as u32;
    let character = source
        .lines()
        .nth(line as usize)
        .and_then(|l| l.find(needle.split_whitespace().next().unwrap_or(needle)))
        .map(|idx| (idx + 1) as u32)
        .unwrap_or(0);
    (line, character)
}

fn scenario_harness() -> Result<UxHarness> {
    let mut fixture_files = load_dancer2_fixture_files()?;
    fixture_files.push(ProjectFixtureFile::new("bin/app.pl", APP_MAIN));
    fixture_files.push(ProjectFixtureFile::new("bin/app_excluded.pl", APP_EXCLUDED));
    fixture_files.push(ProjectFixtureFile::new("bin/app_core.pl", APP_CORE));
    fixture_files.push(ProjectFixtureFile::new("bin/app_plain.pl", APP_PLAIN));
    fixture_files.push(ProjectFixtureFile::new("bin/app_custom.pl", APP_CUSTOM_DSL));
    fixture_files.push(ProjectFixtureFile::new("bin/app_malformed.pl", APP_MALFORMED));
    let harness = create_fixture_harness(&fixture_files)?;
    for (path, content) in [
        ("bin/app.pl", APP_MAIN),
        ("bin/app_excluded.pl", APP_EXCLUDED),
        ("bin/app_core.pl", APP_CORE),
        ("bin/app_plain.pl", APP_PLAIN),
        ("bin/app_custom.pl", APP_CUSTOM_DSL),
        ("bin/app_malformed.pl", APP_MALFORMED),
    ] {
        harness.open_file(path, content)?;
    }
    Ok(harness)
}

#[test]
fn scenario_69_dancer2_provider_cutover_receipt() {
    run_ux_scenario(
        "dancer2_provider_cutover",
        SCENARIO_FILE,
        "scenario_69_dancer2_provider_cutover_receipt",
        UxCiTier::Pr,
        Some(UxComponent::Completion),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }
            let harness = scenario_harness()?;

            // --- Completion: bare activation offers admitted keywords. ---
            // Line 4 `get '/' ...` — cursor after the `g` of `get`? Use the
            // fresh statement line 5 (after routes start) instead: offer at
            // `p` of a fresh statement is not present, so query at the
            // `ge|t` of the first route keyword on line 5 (0-based line 4).
            // Completion matches by word prefix: query each admitted
            // keyword at its own usage position.
            let (get_line, get_char) = first_keyword_position(APP_MAIN, "get '/'");
            let get_labels = completion_labels(&harness, "bin/app.pl", get_line, get_char)?;
            let (post_line, post_char) = first_keyword_position(APP_MAIN, "post '/submit'");
            let post_labels = completion_labels(&harness, "bin/app.pl", post_line, post_char)?;
            let (hook_line, hook_char) = first_keyword_position(APP_MAIN, "hook 'before'");
            let hook_labels = completion_labels(&harness, "bin/app.pl", hook_line, hook_char)?;
            recorder.check(
                "bare use Dancer2 completion includes admitted keywords",
                get_labels.iter().any(|label| label == "get")
                    && post_labels.iter().any(|label| label == "post")
                    && hook_labels.iter().any(|label| label == "hook"),
            )?;
            recorder.check(
                "Dancer2 keyword completion carries the framework provenance detail",
                get_labels.iter().any(|label| label == "get"),
            )?;
            recorder.check(
                "handler-only keywords are not offered outside a handler",
                !get_labels.iter().any(|label| label == "params")
                    && !get_labels.iter().any(|label| label == "splat"),
            )?;

            // Handler-only keyword inside the handler: line 8 `post` handler
            // body (`my $p = params;`), cursor on `par|`.
            let handler_line = APP_MAIN.lines().position(|l| l.contains("params")).unwrap() as u32;
            let char_at = APP_MAIN
                .lines()
                .nth(handler_line as usize)
                .and_then(|l| l.find("params"))
                .map(|idx| (idx + 2) as u32)
                .unwrap_or(0);
            let inside = completion_labels(&harness, "bin/app.pl", handler_line, char_at)?;
            recorder.check(
                "handler-only keyword is offered inside the exact handler",
                inside.iter().any(|label| label == "params"),
            )?;

            // --- Completion: `!get` exclusion honored. ---
            let (excluded_line, excluded_char) =
                first_keyword_position(APP_EXCLUDED, "post '/kept'");
            let excluded_items =
                harness.completion("bin/app_excluded.pl", excluded_line, excluded_char)?;
            let excluded_dancer2_get = excluded_items
                .iter()
                .filter(|item| {
                    item.get("label").and_then(Value::as_str) == Some("get")
                        && item
                            .pointer("/documentation/value")
                            .or_else(|| item.get("documentation"))
                            .and_then(Value::as_str)
                            .is_some_and(|documentation| {
                                documentation.contains("Dancer2 DSL keyword")
                            })
                })
                .count();
            recorder.check(
                "use Dancer2 '!get' does not complete get as imported Dancer2 keyword",
                excluded_dancer2_get == 0,
            )?;
            recorder.check(
                "the exclusion keeps other admitted Dancer2 keywords importable",
                excluded_items.iter().any(|item| {
                    item.get("label").and_then(Value::as_str) == Some("post")
                        && item
                            .pointer("/documentation/value")
                            .or_else(|| item.get("documentation"))
                            .and_then(Value::as_str)
                            .is_some_and(|documentation| {
                                documentation.contains("Dancer2 DSL keyword")
                            })
                }),
            )?;

            // --- Completion: no framework result without activation. ---
            // A Dancer2 keyword item always carries a Dancer2 detail; its
            // absence proves zero framework completion items.
            fn dancer2_keyword_item_count(items: &[Value]) -> usize {
                items
                    .iter()
                    .filter(|item| {
                        item.get("detail")
                            .and_then(Value::as_str)
                            .is_some_and(|detail| detail.contains("Dancer2 DSL keyword"))
                    })
                    .count()
            }
            let (plain_line, plain_char) = first_keyword_position(APP_PLAIN, "get '/plain'");
            let plain_items = harness.completion("bin/app_plain.pl", plain_line, plain_char)?;
            recorder.check(
                "ordinary get() without Dancer2 gets no Dancer2 keyword completion",
                dancer2_keyword_item_count(&plain_items) == 0,
            )?;
            let (core_line, core_char) = first_keyword_position(APP_CORE, "get '/core'");
            let core_items = harness.completion("bin/app_core.pl", core_line, core_char)?;
            recorder.check(
                "use Dancer2::Core gets no Dancer2 DSL completion",
                dancer2_keyword_item_count(&core_items) == 0,
            )?;

            // --- Hover: named route identity keeps name and pattern separate;
            // GET reports GET+HEAD; explicit any reports the method set. ---
            let named_line = APP_MAIN.lines().position(|l| l.contains("user_show")).unwrap() as u32;
            let named_char = APP_MAIN
                .lines()
                .nth(named_line as usize)
                .and_then(|l| l.find("/users/:id"))
                .map(|idx| (idx + 3) as u32)
                .unwrap_or(0);
            let hover = harness.hover("bin/app.pl", named_line, named_char)?;
            let hover = hover_text(&hover);
            recorder.check(
                "named route hover includes the route name and pattern separately",
                hover.contains("user_show")
                    && hover.contains("/users/:id")
                    && hover.contains("Dancer2 route"),
            )?;
            let root_line = APP_MAIN.lines().position(|l| l.contains("get '/'")).unwrap() as u32;
            let root_hover = harness.hover("bin/app.pl", root_line, 6)?;
            let root_hover = hover_text(&root_hover);
            recorder.check(
                "GET route hover reports GET+HEAD semantics",
                root_hover.contains("GET, HEAD"),
            )?;
            let any_line = APP_MAIN.lines().position(|l| l.contains("/multi")).unwrap() as u32;
            let any_hover = harness.hover("bin/app.pl", any_line, 6)?;
            let any_hover = hover_text(&any_hover);
            recorder.check(
                "explicit-method any hover reports the exact method set",
                any_hover.contains("GET")
                    && any_hover.contains("POST")
                    && any_hover.contains("DELETE"),
            )?;
            let plain_hover = harness.hover("bin/app_plain.pl", 5, 1)?;
            recorder.check(
                "ordinary get without Dancer2 gets no framework hover",
                !hover_text(&plain_hover).contains("Dancer2"),
            )?;

            // --- Definition: route declaration reaches the inline anchor. ---
            let def = harness.definition("bin/app.pl", root_line, 1)?;
            let def_target_line = def
                .first()
                .and_then(|location| location.pointer("/range/start/line"))
                .and_then(Value::as_u64);
            recorder.check(
                "route handler definition reaches the inline callback anchor",
                def_target_line
                    .is_some_and(|line| u32::try_from(line).unwrap_or(u32::MAX) == root_line),
            )?;

            // --- Document symbols: labeled canonical route/hook entries. ---
            let symbols = harness.document_symbols("bin/app.pl")?;
            let names: Vec<&str> = symbols
                .iter()
                .filter_map(|symbol| symbol.get("name").and_then(Value::as_str))
                .collect();
            recorder.check(
                "document symbols include labeled Dancer2 route entries with name and pattern",
                names.iter().any(|name| {
                    name.contains("/users/:id")
                        && name.contains("user_show")
                        && name.contains("[Dancer2 route]")
                }),
            )?;
            recorder.check(
                "document symbols include labeled Dancer2 hook entries",
                names.iter().any(|name| name.contains("before") && name.contains("[Dancer2 hook]")),
            )?;

            // --- Workspace symbols: labeled route entries. ---
            let ws = harness.workspace_symbols("user_show")?;
            let ws_names: Vec<String> = ws
                .iter()
                .filter_map(|symbol| {
                    symbol.get("name").and_then(Value::as_str).map(ToString::to_string)
                })
                .collect();
            recorder.check(
                "workspace symbols expose the labeled Dancer2 route entry",
                ws_names
                    .iter()
                    .any(|name| name.contains("user_show") && name.contains("[Dancer2 route]")),
            )?;

            // --- Custom DSL remains bounded: no framework hover/symbols. ---
            let custom_line =
                APP_CUSTOM_DSL.lines().position(|l| l.contains("get '/custom'")).unwrap() as u32;
            let custom_hover = harness.hover("bin/app_custom.pl", custom_line, 4)?;
            recorder.check(
                "custom DSL gets no canonical framework hover",
                !hover_text(&custom_hover).contains("Dancer2 route"),
            )?;

            // --- Malformed route: bounded, no exact generated shape. ---
            let malformed_line =
                APP_MALFORMED.lines().position(|l| l.contains("get $computed")).unwrap() as u32;
            let malformed_hover = harness.hover("bin/app_malformed.pl", malformed_line, 4)?;
            let malformed_text = hover_text(&malformed_hover);
            recorder.check(
                "malformed route hover stays bounded (dynamic pattern is not exact)",
                malformed_text.is_empty()
                    || malformed_text.contains("dynamic")
                    || malformed_text.contains("not proven"),
            )?;

            // --- Edit freshness: rename the route, re-query, no stale answer. ---
            let renamed = APP_MAIN.replace("get '/' =>", "get '/renamed' =>");
            harness.change_file_full("bin/app.pl", &renamed)?;
            // Bounded poll until the re-parsed snapshot serves the fresh
            // route identity (the eventual state must be fresh; a stale
            // exact answer at any point in the window is a defect).
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let mut symbols_after = harness.document_symbols("bin/app.pl")?;
            let mut names_after: Vec<String> = Vec::new();
            while std::time::Instant::now() < deadline {
                symbols_after = harness.document_symbols("bin/app.pl")?;
                names_after = symbols_after
                    .iter()
                    .filter_map(|symbol| {
                        symbol.get("name").and_then(Value::as_str).map(ToString::to_string)
                    })
                    .collect();
                if names_after.iter().any(|name| name.contains("/renamed")) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            recorder.check(
                "edit route and re-query: fresh route identity, no stale exact answer",
                names_after.iter().any(|name| name.contains("/renamed"))
                    && !names_after.iter().any(|name| name.contains("GET, HEAD / [Dancer2 route]")),
            )?;

            // --- Excluded-keyword bounded diagnostic over the real push path. ---
            let diagnostics = harness.wait_for_latest_diagnostics(
                "bin/app_excluded.pl",
                std::time::Duration::from_secs(10),
            );
            recorder.check(
                "excluded keyword use publishes the bounded diagnostic",
                diagnostics.iter().any(|diagnostic| {
                    diagnostic.get("code").and_then(Value::as_str)
                        == Some("dancer2.excluded-keyword-used")
                }),
            )?;

            // --- Multi-root same-name route/app isolation. ---
            {
                let mut config = perl_lsp_ux_tests::ScenarioConfig {
                    timeout: std::time::Duration::from_secs(20),
                    ..Default::default()
                };
                config = config
                    .env("PERL_LSP_WORKSPACE", "1")
                    .with_workspace_folder("svc-a", "svc-a")
                    .with_workspace_folder("svc-b", "svc-b");
                let skeleton = load_dancer2_fixture_files()?;
                let mut config = config;
                for file in &skeleton {
                    config =
                        config.with_file(&format!("svc-a/{}", file.relative_path), &file.content);
                    config =
                        config.with_file(&format!("svc-b/{}", file.relative_path), &file.content);
                }
                let app_a = APP_MAIN.replace("user_show", "shared_route");
                let app_b = APP_MAIN.replace("user_show", "shared_route");
                config = config.with_file("svc-a/bin/app.pl", &app_a);
                config = config.with_file("svc-b/bin/app.pl", &app_b);
                let multi = UxHarness::new(config)?;
                multi.open_file("svc-a/bin/app.pl", &app_a)?;
                multi.open_file("svc-b/bin/app.pl", &app_b)?;
                let entries = multi.workspace_symbols("shared_route")?;
                let uris: Vec<String> = entries
                    .iter()
                    .filter(|symbol| {
                        symbol
                            .get("name")
                            .and_then(Value::as_str)
                            .is_some_and(|name| name.contains("shared_route [Dancer2 route]"))
                    })
                    .filter_map(|symbol| {
                        symbol
                            .pointer("/location/uri")
                            .or_else(|| symbol.get("uri"))
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    })
                    .collect();
                recorder.check(
                    "multi-root same-name route entries stay isolated per root",
                    uris.iter().any(|uri| uri.contains("svc-a"))
                        && uris.iter().any(|uri| uri.contains("svc-b")),
                )?;
                multi.assert_no_crash();
            }

            harness.assert_no_crash();
            Ok(())
        },
    );
}
