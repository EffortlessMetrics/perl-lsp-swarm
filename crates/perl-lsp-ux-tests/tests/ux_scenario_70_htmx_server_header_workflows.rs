//! Scenario 70 — htmx server-side header authoring over real web stacks (#14948).
//!
//! Exercises the canonical htmx header catalog through the real
//! `textDocument/completion` request path — not the helper functions — in
//! three representative Perl server styles:
//!
//! - Dancer2 over the committed skeleton (`Dancer2::Core::Request::header`,
//!   `Dancer2::Core::Response::header`);
//! - Mojolicious over the committed skeleton (`$c->req->headers->header`,
//!   `$c->res->headers->header`);
//! - framework-neutral PSGI/Plack (`Plack::Request->header` and a raw PSGI
//!   response header array).
//!
//! Each fixture detects an htmx request through a canonical request header,
//! returns a partial response, emits a canonical response header, and passes
//! one dynamic value that must remain unrestricted.
//!
//! Receipt signals (#14948's required cases), per stack:
//! - the fixture opens without parse-error diagnostics (parser acceptance);
//! - a request-header detection site completes `HX-Request` with the
//!   request-direction detail (provider output);
//! - a response-header emission site completes the whole `HX-Trigger*` family
//!   with per-item direction details (the catalog is direction *metadata*;
//!   call-site direction filtering is not claimed);
//! - case-insensitive typing inserts the canonical spelling;
//! - every htmx item carries a `textEdit` that replaces exactly the typed
//!   prefix inside the quotes (LSP serialization);
//! - a dynamic header value and an ordinary path string receive no `HX-*`
//!   item, and no htmx diagnostic is published (fail-closed boundary).
//!
//! Explicitly unsupported here: `hx-*` attribute completion inside template
//! documents (`.html.ep`, Template Toolkit, inline templates) waits on the
//! template ingress contract (#14114, #13964) and is neither asserted nor
//! claimed; hover, diagnostics, and header-value completion are later #14102
//! children.

use anyhow::Result;
use perl_lsp_ux_tests::ProjectFixtureFile;
use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::missing_binary_skip;
use perl_lsp_ux_tests::{
    UxCiTier, UxComponent, UxHarness, UxRunRecorder, create_fixture_harness,
    load_dancer2_fixture_files, load_mojolicious_fixture_files, run_ux_scenario,
};
use serde_json::Value;
use std::time::Duration;

const SCENARIO_FILE: &str = "ux_scenario_70_htmx_server_header_workflows.rs";

/// Parse-error diagnostic codes owned by the diagnostics provider.
const PARSE_ERROR_CODES: &[&str] = &["PL001", "PL002", "PL003"];

/// Completion sites shared by every fixture. Each needle is unique within its
/// file so the cursor lands right after it, inside the quotes the editor
/// auto-closed: `'HX-Req|')`.
const REQUEST_SITE: &str = "'HX-Req";
const RESPONSE_SITE: &str = "'HX-Tri";
const CASE_INSENSITIVE_SITE: &str = "'hx-red";
const DYNAMIC_VALUE_SITE: &str = "=> $event_name";
const ORDINARY_STRING_SITE: &str = "'templates/";

/// Dancer2 DSL app. Fixture authority: Dancer2 1.1.1 core request/response
/// `header` methods as vendored in `test_corpus/real_projects/dancer2_skeleton`.
const DANCER2_APP: &str = r#"#!/usr/bin/perl
# htmx request/response header workflow over the Dancer2 1.1.1 core API
# (Dancer2::Core::Request::header, Dancer2::Core::Response::header).
use strict;
use warnings;
use lib 'lib';
use Dancer2;

get '/fragments' => sub {
    if (request->header('HX-Req')) {
        my $event_name = params->{event};
        response->header('HX-Tri' => $event_name);
        return '<li>fragment</li>';
    }
    return template 'templates/';
};

get '/refresh' => sub {
    response->header('hx-red' => '/');
    return '';
};

true;
"#;

/// Mojolicious controller. Fixture authority: Mojolicious 9.34
/// `Mojolicious::Controller::req`/`res` and `Mojo::Headers::header`, over
/// `test_corpus/real_projects/mojolicious_skeleton`.
const MOJOLICIOUS_CONTROLLER: &str = r#"package MyApp::Controller::Fragments;
# htmx request/response header workflow over the Mojolicious 9.34 controller
# API ($c->req->headers->header, $c->res->headers->header).
use Mojo::Base 'Mojolicious::Controller';

sub list {
    my $c = shift;
    if ($c->req->headers->header('HX-Req')) {
        my $event_name = $c->param('event');
        $c->res->headers->header('HX-Tri' => $event_name);
        return $c->render(text => '<li>fragment</li>');
    }
    return $c->render(template => 'templates/');
}

sub refresh {
    my $c = shift;
    $c->res->headers->header('hx-red' => '/');
    return $c->rendered(204);
}

1;
"#;

/// Framework-neutral PSGI app. Fixture authority: PSGI 1.1 response arrays and
/// Plack::Request 1.0051 `header`; no framework helper is claimed.
const PSGI_APP: &str = r#"# htmx request/response header workflow over PSGI 1.1 response arrays and
# Plack::Request->header; no framework helper is claimed.
use strict;
use warnings;
use Plack::Request;

my $app = sub {
    my $env = shift;
    my $req = Plack::Request->new($env);
    if ($env->{PATH_INFO} eq '/refresh') {
        return [200, ['hx-red' => '/'], ['']];
    }
    if ($req->header('HX-Req')) {
        my $event_name = $req->parameters->{event};
        return [200, ['Content-Type' => 'text/html', 'HX-Tri' => $event_name], ['<li>fragment</li>']];
    }
    my $page = 'templates/';
    return [200, ['Content-Type' => 'text/html'], [$page]];
};

$app;
"#;

struct Stack {
    name: &'static str,
    path: &'static str,
    source: &'static str,
}

const STACKS: &[Stack] = &[
    Stack { name: "dancer2", path: "bin/app.pl", source: DANCER2_APP },
    Stack {
        name: "mojolicious",
        path: "lib/MyApp/Controller/Fragments.pm",
        source: MOJOLICIOUS_CONTROLLER,
    },
    Stack { name: "psgi", path: "app.psgi", source: PSGI_APP },
];

/// Zero-based (line, character) of the position immediately after the first
/// occurrence of `needle`; every fixture is ASCII, so byte and UTF-16 columns
/// coincide.
fn cursor_after(source: &str, needle: &str) -> (u32, u32) {
    let line = source.lines().position(|l| l.contains(needle)).unwrap_or(0);
    let column = source
        .lines()
        .nth(line)
        .and_then(|l| l.find(needle))
        .map(|idx| idx + needle.len())
        .unwrap_or(0);
    (line as u32, column as u32)
}

fn labels(items: &[Value]) -> Vec<&str> {
    items.iter().filter_map(|item| item.get("label").and_then(Value::as_str)).collect()
}

fn htmx_labels(items: &[Value]) -> Vec<&str> {
    labels(items)
        .into_iter()
        .filter(|label| label.to_ascii_lowercase().starts_with("hx-"))
        .collect()
}

fn item_detail<'a>(items: &'a [Value], label: &str) -> Option<&'a str> {
    items
        .iter()
        .find(|item| item.get("label").and_then(Value::as_str) == Some(label))
        .and_then(|item| item.get("detail").and_then(Value::as_str))
}

/// True when `item` carries a plain `textEdit` replacing exactly
/// `[prefix_start, cursor)` on `line` with `new_text`.
fn text_edit_replaces_prefix(
    item: &Value,
    line: u32,
    prefix_start: u32,
    cursor: u32,
    new_text: &str,
) -> bool {
    let range_point = |pointer: &str| item.pointer(pointer).and_then(Value::as_u64);
    item.pointer("/textEdit/newText").and_then(Value::as_str) == Some(new_text)
        && range_point("/textEdit/range/start/line") == Some(u64::from(line))
        && range_point("/textEdit/range/start/character") == Some(u64::from(prefix_start))
        && range_point("/textEdit/range/end/line") == Some(u64::from(line))
        && range_point("/textEdit/range/end/character") == Some(u64::from(cursor))
}

fn diagnostic_codes(diagnostics: &[Value]) -> Vec<String> {
    diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.get("code"))
        .map(|code| match code {
            Value::String(code) => code.clone(),
            other => other.to_string(),
        })
        .collect()
}

fn scenario_harness() -> Result<UxHarness> {
    let mut fixture_files = load_dancer2_fixture_files()?;
    fixture_files.extend(load_mojolicious_fixture_files()?);
    for stack in STACKS {
        fixture_files.push(ProjectFixtureFile::new(stack.path, stack.source));
    }
    let harness = create_fixture_harness(&fixture_files)?;
    for stack in STACKS {
        harness.open_file(stack.path, stack.source)?;
    }
    Ok(harness)
}

fn check_stack(recorder: &mut UxRunRecorder, harness: &UxHarness, stack: &Stack) -> Result<()> {
    let Stack { name, path, source } = stack;

    // --- Parser acceptance: the fixture is admitted Perl. ---
    let diagnostics = harness.wait_for_latest_diagnostics(path, Duration::from_secs(10));
    let codes = diagnostic_codes(&diagnostics);
    recorder.check(
        &format!("{name}: fixture opens without parse-error diagnostics"),
        !codes.iter().any(|code| PARSE_ERROR_CODES.contains(&code.as_str())),
    )?;
    recorder.check(
        &format!("{name}: no htmx diagnostic is published for the fixture"),
        !codes.iter().any(|code| code.to_ascii_lowercase().contains("htmx")),
    )?;

    // --- Request-header detection site completes the canonical request name. ---
    let (line, cursor) = cursor_after(source, REQUEST_SITE);
    let prefix_start = cursor - (REQUEST_SITE.len() as u32 - 1);
    recorder.mark_request_start("completion");
    let request_items = harness.completion(path, line, cursor)?;
    let request_labels = htmx_labels(&request_items);
    recorder.check(
        &format!("{name}: request-header site completes exactly HX-Request"),
        request_labels == ["HX-Request"],
    )?;
    recorder.mark_first_useful_result("completion");
    recorder.check(
        &format!("{name}: HX-Request carries the request-direction detail"),
        item_detail(&request_items, "HX-Request") == Some("htmx request header"),
    )?;
    recorder.check(
        &format!("{name}: HX-Request textEdit replaces exactly the typed prefix"),
        request_items.iter().any(|item| {
            item.get("label").and_then(Value::as_str) == Some("HX-Request")
                && text_edit_replaces_prefix(item, line, prefix_start, cursor, "HX-Request")
        }),
    )?;
    recorder.check(
        &format!("{name}: HX-Request carries the catalog documentation"),
        request_items.iter().any(|item| {
            item.get("label").and_then(Value::as_str) == Some("HX-Request")
                && item
                    .pointer("/documentation/value")
                    .and_then(Value::as_str)
                    .is_some_and(|documentation| !documentation.is_empty())
        }),
    )?;

    // --- Response-header emission site completes the HX-Trigger family. ---
    let (line, cursor) = cursor_after(source, RESPONSE_SITE);
    let prefix_start = cursor - (RESPONSE_SITE.len() as u32 - 1);
    let response_items = harness.completion(path, line, cursor)?;
    recorder.check(
        &format!("{name}: response-header site completes the HX-Trigger family in catalog order"),
        htmx_labels(&response_items)
            == [
                "HX-Trigger",
                "HX-Trigger-After-Settle",
                "HX-Trigger-After-Swap",
                "HX-Trigger-Name",
            ],
    )?;
    recorder.check(
        &format!("{name}: HX-Trigger family items carry their catalog direction details"),
        item_detail(&response_items, "HX-Trigger") == Some("htmx request and response header")
            && item_detail(&response_items, "HX-Trigger-After-Settle")
                == Some("htmx response header")
            && item_detail(&response_items, "HX-Trigger-After-Swap")
                == Some("htmx response header")
            && item_detail(&response_items, "HX-Trigger-Name") == Some("htmx request header"),
    )?;
    recorder.check(
        &format!("{name}: every HX-Trigger item textEdit replaces exactly the typed prefix"),
        response_items.iter().all(|item| {
            item.get("label").and_then(Value::as_str).is_some_and(|label| {
                text_edit_replaces_prefix(item, line, prefix_start, cursor, label)
            })
        }),
    )?;

    // --- Case-insensitive typing inserts the canonical spelling. ---
    let (line, cursor) = cursor_after(source, CASE_INSENSITIVE_SITE);
    let prefix_start = cursor - (CASE_INSENSITIVE_SITE.len() as u32 - 1);
    let redirect_items = harness.completion(path, line, cursor)?;
    recorder.check(
        &format!("{name}: lowercase hx-red completes exactly HX-Redirect"),
        htmx_labels(&redirect_items) == ["HX-Redirect"],
    )?;
    recorder.check(
        &format!(
            "{name}: HX-Redirect textEdit replaces the lowercase prefix with the canonical name"
        ),
        redirect_items
            .iter()
            .any(|item| text_edit_replaces_prefix(item, line, prefix_start, cursor, "HX-Redirect")),
    )?;

    // --- Dynamic value stays unrestricted: no htmx item at the value. ---
    let (line, cursor) = cursor_after(source, DYNAMIC_VALUE_SITE);
    let dynamic_items = harness.completion(path, line, cursor)?;
    recorder.check(
        &format!("{name}: a dynamic header value receives no htmx completion item"),
        htmx_labels(&dynamic_items).is_empty(),
    )?;

    // --- Ordinary string stays owned by ordinary string completion. ---
    let (line, cursor) = cursor_after(source, ORDINARY_STRING_SITE);
    let ordinary_items = harness.completion(path, line, cursor)?;
    recorder.check(
        &format!("{name}: an ordinary path string receives no htmx completion item"),
        htmx_labels(&ordinary_items).is_empty(),
    )?;

    Ok(())
}

#[test]
fn scenario_70_htmx_server_header_workflows_receipt() {
    run_ux_scenario(
        "htmx_server_header_workflows",
        SCENARIO_FILE,
        "scenario_70_htmx_server_header_workflows_receipt",
        UxCiTier::Pr,
        Some(UxComponent::Completion),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }
            let harness = scenario_harness()?;

            for stack in STACKS {
                check_stack(recorder, &harness, stack)?;
            }

            harness.assert_no_crash();
            Ok(())
        },
    );
}
