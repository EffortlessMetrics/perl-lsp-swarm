//! Scenario 70 — HTMX server-header authoring through the real completion path (#14948).
//!
//! Spawns `perllsp` over the committed Dancer2 and Mojolicious skeletons plus a
//! synthetic PSGI/Plack app. Assertions go through `textDocument/completion`
//! and `textDocument/publishDiagnostics`, not `providers::htmx` helpers.
//!
//! Explicitly unsupported (recorded, not asserted):
//! - htmx attribute completion inside `.html.ep` / Template Toolkit / inline templates
//! - editor/host smoke of the full request path
//! - hover, product diagnostics, and header-value completion
//!
//! Catalog honesty: header completion is prefix-shaped and call-site agnostic.
//! Request vs response sites are exercised as realistic authoring surfaces, not
//! as a claim that the catalog filters by AST role.

use anyhow::{Context, Result, anyhow, bail};
use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::missing_binary_skip;
use perl_lsp_ux_tests::{
    ProjectFixtureFile, UxCiTier, UxComponent, UxHarness, create_fixture_harness,
    load_dancer2_fixture_files, load_mojolicious_fixture_files, open_all_fixture_files,
    run_ux_scenario,
};
use serde_json::Value;
use std::time::Duration;

const SCENARIO_FILE: &str = "ux_scenario_70_htmx_server_header_workflows.rs";
const PARSE_ERROR_PATH: &str = "bin/htmx_header_parse_error.pl";
const DIAGNOSTIC_WAIT: Duration = Duration::from_secs(10);

const REQUEST_PREFIX: &str = "HX-Req";
const TRIGGER_PREFIX: &str = "HX-Tri";
const REDIRECT_PREFIX: &str = "hx-red";
const MIXED_REQUEST_PREFIX: &str = "Hx-Req";
const PATH_PREFIX: &str = "templates/";

/// Catalog labels for prefix `HX-Tri`, in catalog order. Exact-set is the
/// protocol receipt that the matcher is prefix-shaped and call-site agnostic:
/// `HX-Trigger-Name` (request) is offered at a response site, and the After-*
/// response headers are offered at a request site.
const TRIGGER_PREFIX_LABELS: [&str; 4] =
    ["HX-Trigger", "HX-Trigger-After-Settle", "HX-Trigger-After-Swap", "HX-Trigger-Name"];

const DANCER2_PROBE_PATH: &str = "bin/htmx_headers.pl";
const MOJO_PROBE_PATH: &str = "lib/HtmxHeaderController.pm";
const PLACK_PROBE_PATH: &str = "app.psgi";

const DANCER2_PROBE: &str = r#"#!/usr/bin/perl
use strict;
use warnings;
use lib 'lib';
use Dancer2;

get '/partial' => sub {
    my $is_htmx = request->header('HX-Req');
    my $quoted = request->header("HX-Req");
    my $mixed = request->header('Hx-Req');
    my $request_trigger = request->header('HX-Tri');
    my $event_name = 'reload';
    response->header('HX-Tri' => $event_name);
    response->header('hx-red' => '/next');
    my $template = 'templates/';
    return 'ok';
};
"#;

const MOJO_PROBE: &str = r#"package HtmxHeaderController;
use strict;
use warnings;
use Mojo::Base 'Mojolicious::Controller';

sub partial {
    my $c = shift;
    my $is_htmx = $c->req->headers->header('HX-Req');
    my $quoted = $c->req->headers->header("HX-Req");
    my $mixed = $c->req->headers->header('Hx-Req');
    my $request_trigger = $c->req->headers->header('HX-Tri');
    my $event_name = 'reload';
    $c->res->headers->header('HX-Tri' => $event_name);
    $c->res->headers->header('hx-red' => '/next');
    my $template = 'templates/';
    return $c->render(text => 'ok');
}

1;
"#;

const PLACK_PROBE: &str = r#"#!/usr/bin/perl
use strict;
use warnings;
use Plack::Request;

my $app = sub {
    my $env = shift;
    my $req = Plack::Request->new($env);
    my $is_htmx = $req->header('HX-Req');
    my $quoted = $req->header("HX-Req");
    my $mixed = $req->header('Hx-Req');
    my $request_trigger = $req->header('HX-Tri');
    my $event_name = 'reload';
    my $template = 'templates/';
    return [
        200,
        [
            'Content-Type' => 'text/html',
            'HX-Tri' => $event_name,
            'hx-red' => '/next',
        ],
        ['ok'],
    ];
};
"#;

const PARSE_ERROR_SOURCE: &str = r#"#!/usr/bin/perl
use strict;
use warnings;
my $broken = ;
"#;

#[derive(Clone, Copy)]
struct Stack {
    name: &'static str,
    probe_path: &'static str,
    probe_source: &'static str,
    request_needle: &'static str,
    quoted_needle: &'static str,
    mixed_needle: &'static str,
    request_trigger_needle: &'static str,
    response_trigger_needle: &'static str,
    redirect_needle: &'static str,
    path_needle: &'static str,
    value_line_needle: &'static str,
}

fn stacks() -> [Stack; 3] {
    [
        Stack {
            name: "Dancer2",
            probe_path: DANCER2_PROBE_PATH,
            probe_source: DANCER2_PROBE,
            request_needle: "request->header('HX-Req')",
            quoted_needle: "request->header(\"HX-Req\")",
            mixed_needle: "request->header('Hx-Req')",
            request_trigger_needle: "request->header('HX-Tri')",
            response_trigger_needle: "response->header('HX-Tri'",
            redirect_needle: "response->header('hx-red'",
            path_needle: "my $template = 'templates/'",
            value_line_needle: "response->header('HX-Tri' => $event_name)",
        },
        Stack {
            name: "Mojolicious",
            probe_path: MOJO_PROBE_PATH,
            probe_source: MOJO_PROBE,
            request_needle: "$c->req->headers->header('HX-Req')",
            quoted_needle: "$c->req->headers->header(\"HX-Req\")",
            mixed_needle: "$c->req->headers->header('Hx-Req')",
            request_trigger_needle: "$c->req->headers->header('HX-Tri')",
            response_trigger_needle: "$c->res->headers->header('HX-Tri'",
            redirect_needle: "$c->res->headers->header('hx-red'",
            path_needle: "my $template = 'templates/'",
            value_line_needle: "$c->res->headers->header('HX-Tri' => $event_name)",
        },
        Stack {
            name: "PSGI/Plack",
            probe_path: PLACK_PROBE_PATH,
            probe_source: PLACK_PROBE,
            request_needle: "$req->header('HX-Req')",
            quoted_needle: "$req->header(\"HX-Req\")",
            mixed_needle: "$req->header('Hx-Req')",
            request_trigger_needle: "$req->header('HX-Tri')",
            response_trigger_needle: "'HX-Tri' => $event_name",
            redirect_needle: "'hx-red' => '/next'",
            path_needle: "my $template = 'templates/'",
            value_line_needle: "'HX-Tri' => $event_name",
        },
    ]
}

fn load_stack_files(stack: Stack) -> Result<Vec<ProjectFixtureFile>> {
    let mut files = match stack.name {
        "Dancer2" => load_dancer2_fixture_files()?,
        "Mojolicious" => load_mojolicious_fixture_files()?,
        "PSGI/Plack" => Vec::new(),
        other => bail!("unknown stack {other}"),
    };
    files.push(ProjectFixtureFile::new(stack.probe_path, stack.probe_source));
    files.push(ProjectFixtureFile::new(PARSE_ERROR_PATH, PARSE_ERROR_SOURCE));
    Ok(files)
}

fn line_containing<'a>(source: &'a str, needle: &str) -> Result<(u32, &'a str)> {
    source
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(needle))
        .map(|(idx, line)| (u32::try_from(idx).unwrap_or(u32::MAX), line))
        .ok_or_else(|| anyhow!("missing line containing {needle}"))
}

fn cursor_after_prefix(source: &str, line_needle: &str, prefix: &str) -> Result<(u32, u32)> {
    let (line, text) = line_containing(source, line_needle)?;
    let start = text.find(prefix).ok_or_else(|| anyhow!("missing prefix {prefix} in {text}"))?;
    let character = u32::try_from(start + prefix.len())
        .with_context(|| format!("cursor overflow for {prefix}"))?;
    Ok((line, character))
}

fn prefix_range(source: &str, line_needle: &str, prefix: &str) -> Result<(u32, u32, u32)> {
    let (line, end) = cursor_after_prefix(source, line_needle, prefix)?;
    let start = end
        .checked_sub(u32::try_from(prefix.len())?)
        .ok_or_else(|| anyhow!("prefix longer than cursor for {prefix}"))?;
    Ok((line, start, end))
}

fn cursor_at_value(source: &str, line_needle: &str) -> Result<(u32, u32)> {
    let (line, text) = line_containing(source, line_needle)?;
    let start = text
        .rfind("$event_name")
        .ok_or_else(|| anyhow!("missing $event_name value on {line_needle}"))?;
    Ok((line, u32::try_from(start)?))
}

fn cursor_after_path(source: &str, line_needle: &str) -> Result<(u32, u32)> {
    cursor_after_prefix(source, line_needle, PATH_PREFIX)
}

fn item_label(item: &Value) -> Option<&str> {
    item.get("label").and_then(Value::as_str)
}

fn item_detail(item: &Value) -> Option<&str> {
    item.get("detail").and_then(Value::as_str)
}

fn is_hx_header_item(item: &Value) -> bool {
    item_label(item).is_some_and(|label| {
        label.get(..3).is_some_and(|head| head.eq_ignore_ascii_case("HX-"))
            || label.eq_ignore_ascii_case("HX")
    })
}

fn hx_items(items: &[Value]) -> Vec<&Value> {
    items.iter().filter(|item| is_hx_header_item(item)).collect()
}

fn text_edit(item: &Value) -> Option<&Value> {
    item.get("textEdit")
}

fn text_edit_replaces_only_prefix(
    item: &Value,
    source: &str,
    line_needle: &str,
    prefix: &str,
    new_text: &str,
) -> bool {
    let Ok((line, start, end)) = prefix_range(source, line_needle, prefix) else {
        return false;
    };
    let Some(edit) = text_edit(item) else {
        return false;
    };
    let Some(range) = edit.get("range") else {
        return false;
    };
    edit.get("newText").and_then(Value::as_str) == Some(new_text)
        && range.pointer("/start/line").and_then(Value::as_u64) == Some(u64::from(line))
        && range.pointer("/end/line").and_then(Value::as_u64) == Some(u64::from(line))
        && range.pointer("/start/character").and_then(Value::as_u64) == Some(u64::from(start))
        && range.pointer("/end/character").and_then(Value::as_u64) == Some(u64::from(end))
}

fn diagnostic_code(diag: &Value) -> Option<&str> {
    match diag.get("code") {
        Some(Value::String(code)) => Some(code.as_str()),
        Some(obj) if obj.is_object() => obj.get("value").and_then(Value::as_str),
        _ => None,
    }
}

fn is_parse_error_diagnostic(diag: &Value) -> bool {
    matches!(diagnostic_code(diag), Some("PL001" | "PL002" | "PL003"))
}

fn has_item_with_shape(
    items: &[Value],
    source: &str,
    line_needle: &str,
    prefix: &str,
    label: &str,
    detail: &str,
) -> bool {
    items.iter().any(|item| {
        item_label(item) == Some(label)
            && item_detail(item) == Some(detail)
            && item.get("kind").and_then(Value::as_u64) == Some(7)
            && text_edit_replaces_only_prefix(item, source, line_needle, prefix, label)
    })
}

fn trigger_prefix_labels(items: &[Value]) -> Vec<&str> {
    items.iter().filter_map(item_label).collect()
}

fn trigger_prefix_set_is_exact(items: &[Value]) -> bool {
    trigger_prefix_labels(items) == TRIGGER_PREFIX_LABELS
}

fn stack_harness(stack: Stack) -> Result<UxHarness> {
    let files = load_stack_files(stack)?;
    let harness = create_fixture_harness(&files)?;
    open_all_fixture_files(&harness, &files)?;
    Ok(harness)
}

fn complete(
    harness: &UxHarness,
    path: &str,
    source: &str,
    line_needle: &str,
    prefix: &str,
) -> Result<Vec<Value>> {
    let (line, character) = cursor_after_prefix(source, line_needle, prefix)?;
    harness.completion(path, line, character)
}

fn check_header(
    recorder: &mut perl_lsp_ux_tests::UxRunRecorder,
    description: String,
    items: &[Value],
    source: &str,
    line_needle: &str,
    prefix: &str,
    label: &str,
    detail: &str,
) -> Result<()> {
    recorder
        .check(&description, has_item_with_shape(items, source, line_needle, prefix, label, detail))
        .map_err(Into::into)
}

fn prove_stack(
    recorder: &mut perl_lsp_ux_tests::UxRunRecorder,
    stack: Stack,
    mark_first_useful: bool,
) -> Result<()> {
    let harness = stack_harness(stack)?;
    let path = stack.probe_path;
    let source = stack.probe_source;
    let name = stack.name;

    recorder.mark_request_start(&format!("{name} parse_error_channel"));
    let parse_error_diags = harness.wait_for_diagnostics(PARSE_ERROR_PATH, DIAGNOSTIC_WAIT);
    recorder.check(
        &format!(
            "{name} diagnostic channel publishes a parser-family diagnostic for broken source"
        ),
        parse_error_diags.iter().any(is_parse_error_diagnostic),
    )?;

    recorder.mark_request_start(&format!("{name} probe_diagnostics"));
    let probe_diags = harness.wait_for_latest_diagnostics(path, DIAGNOSTIC_WAIT);
    recorder.check(
        &format!("{name} probe opens without parse-error diagnostics (parser acceptance)"),
        !probe_diags.iter().any(is_parse_error_diagnostic),
    )?;

    recorder.mark_request_start(&format!("{name} request_header_completion"));
    let request_items = complete(&harness, path, source, stack.request_needle, REQUEST_PREFIX)?;
    check_header(
        recorder,
        format!(
            "{name} request site completes HX-Request with request-direction detail and prefix-only textEdit"
        ),
        &request_items,
        source,
        stack.request_needle,
        REQUEST_PREFIX,
        "HX-Request",
        "htmx request header",
    )?;
    if mark_first_useful {
        recorder.mark_first_useful_result(&format!("{name} request_header_completion"));
    }

    let quoted_items = complete(&harness, path, source, stack.quoted_needle, REQUEST_PREFIX)?;
    check_header(
        recorder,
        format!(
            "{name} double-quoted request site still serializes HX-Request with prefix-only textEdit"
        ),
        &quoted_items,
        source,
        stack.quoted_needle,
        REQUEST_PREFIX,
        "HX-Request",
        "htmx request header",
    )?;

    let mixed_items = complete(&harness, path, source, stack.mixed_needle, MIXED_REQUEST_PREFIX)?;
    check_header(
        recorder,
        format!("{name} mixed-case Hx-Req inserts canonical HX-Request over the typed prefix"),
        &mixed_items,
        source,
        stack.mixed_needle,
        MIXED_REQUEST_PREFIX,
        "HX-Request",
        "htmx request header",
    )?;

    recorder.mark_request_start(&format!("{name} response_header_completion"));
    let response_items =
        complete(&harness, path, source, stack.response_trigger_needle, TRIGGER_PREFIX)?;
    recorder.check(
        &format!(
            "{name} response-site HX-Tri is the exact catalog set including request-direction HX-Trigger-Name"
        ),
        trigger_prefix_set_is_exact(&response_items),
    )?;
    check_header(
        recorder,
        format!("{name} HX-Trigger serializes as request-and-response with prefix-only textEdit"),
        &response_items,
        source,
        stack.response_trigger_needle,
        TRIGGER_PREFIX,
        "HX-Trigger",
        "htmx request and response header",
    )?;
    check_header(
        recorder,
        format!(
            "{name} HX-Trigger-After-Settle serializes as a response header with prefix-only textEdit"
        ),
        &response_items,
        source,
        stack.response_trigger_needle,
        TRIGGER_PREFIX,
        "HX-Trigger-After-Settle",
        "htmx response header",
    )?;
    check_header(
        recorder,
        format!(
            "{name} HX-Trigger-After-Swap serializes as a response header with prefix-only textEdit"
        ),
        &response_items,
        source,
        stack.response_trigger_needle,
        TRIGGER_PREFIX,
        "HX-Trigger-After-Swap",
        "htmx response header",
    )?;
    check_header(
        recorder,
        format!(
            "{name} HX-Trigger-Name serializes as a request header with prefix-only textEdit at the response site"
        ),
        &response_items,
        source,
        stack.response_trigger_needle,
        TRIGGER_PREFIX,
        "HX-Trigger-Name",
        "htmx request header",
    )?;

    let request_trigger_items =
        complete(&harness, path, source, stack.request_trigger_needle, TRIGGER_PREFIX)?;
    recorder.check(
        &format!(
            "{name} request-site HX-Tri is the same exact catalog set (call-site agnostic, not site-filtered)"
        ),
        trigger_prefix_set_is_exact(&request_trigger_items),
    )?;

    let redirect_items = complete(&harness, path, source, stack.redirect_needle, REDIRECT_PREFIX)?;
    check_header(
        recorder,
        format!(
            "{name} case-insensitive hx-red inserts canonical HX-Redirect over the typed prefix"
        ),
        &redirect_items,
        source,
        stack.redirect_needle,
        REDIRECT_PREFIX,
        "HX-Redirect",
        "htmx response header",
    )?;

    let (value_line, value_char) = cursor_at_value(source, stack.value_line_needle)?;
    let value_items = harness.completion(path, value_line, value_char)?;
    recorder.check(
        &format!("{name} dynamic header value $event_name produces no HX-* completion item"),
        hx_items(&value_items).is_empty(),
    )?;
    recorder.check(
        &format!(
            "{name} dynamic header value $event_name publishes no diagnostic covering the value"
        ),
        !probe_diags.iter().any(|diag| {
            diag.get("range").and_then(|range| range.pointer("/start/line")).and_then(Value::as_u64)
                == Some(u64::from(value_line))
        }),
    )?;

    let (line, character) = cursor_after_path(source, stack.path_needle)?;
    let path_items = harness.completion(path, line, character)?;
    recorder.check(
        &format!("{name} ordinary string templates/ returns no HX-* item"),
        hx_items(&path_items).is_empty(),
    )?;
    Ok(())
}

#[test]
fn cursor_helpers_point_inside_the_quoted_prefix() -> Result<()> {
    let (line, character) =
        cursor_after_prefix(DANCER2_PROBE, "request->header('HX-Req')", REQUEST_PREFIX)?;
    let text = DANCER2_PROBE.lines().nth(line as usize).context("request line")?;
    anyhow::ensure!(text.contains("request->header('HX-Req')"));
    let last = usize::try_from(character.saturating_sub(1))?;
    anyhow::ensure!(text.as_bytes().get(last) == Some(&b'q'));
    anyhow::ensure!(text.as_bytes().get(usize::try_from(character)?) == Some(&b'\''));
    let (line, start, end) =
        prefix_range(DANCER2_PROBE, "request->header('HX-Req')", REQUEST_PREFIX)?;
    anyhow::ensure!(line > 0);
    anyhow::ensure!(&text[start as usize..end as usize] == REQUEST_PREFIX);
    Ok(())
}

#[test]
fn text_edit_oracle_rejects_quote_inclusive_and_suffix_only_edits() -> Result<()> {
    let (line, start, end) =
        prefix_range(DANCER2_PROBE, "request->header('HX-Req')", REQUEST_PREFIX)?;
    let good = serde_json::json!({
        "label": "HX-Request",
        "kind": 7,
        "detail": "htmx request header",
        "textEdit": {
            "newText": "HX-Request",
            "range": {
                "start": { "line": line, "character": start },
                "end": { "line": line, "character": end }
            }
        }
    });
    anyhow::ensure!(text_edit_replaces_only_prefix(
        &good,
        DANCER2_PROBE,
        "request->header('HX-Req')",
        REQUEST_PREFIX,
        "HX-Request",
    ));

    let mut including_quote = good.clone();
    including_quote["textEdit"]["range"]["start"]["character"] = serde_json::json!(start - 1);
    anyhow::ensure!(!text_edit_replaces_only_prefix(
        &including_quote,
        DANCER2_PROBE,
        "request->header('HX-Req')",
        REQUEST_PREFIX,
        "HX-Request",
    ));

    let mut suffix_only = good;
    suffix_only["textEdit"]["newText"] = serde_json::json!("uest");
    anyhow::ensure!(!text_edit_replaces_only_prefix(
        &suffix_only,
        DANCER2_PROBE,
        "request->header('HX-Req')",
        REQUEST_PREFIX,
        "HX-Request",
    ));
    Ok(())
}

#[test]
fn parse_error_oracle_accepts_parser_family_codes_only() {
    assert!(is_parse_error_diagnostic(&serde_json::json!({"code": "PL001"})));
    assert!(is_parse_error_diagnostic(&serde_json::json!({"code": {"value": "PL003"}})));
    assert!(!is_parse_error_diagnostic(&serde_json::json!({"code": "PL701"})));
    assert!(!is_hx_header_item(&serde_json::json!({"label": "templates/foo"})));
}

#[test]
fn trigger_prefix_oracle_requires_the_exact_catalog_set_in_order() {
    let exact: Vec<Value> =
        TRIGGER_PREFIX_LABELS.iter().map(|label| serde_json::json!({"label": label})).collect();
    assert!(trigger_prefix_set_is_exact(&exact));

    let missing_name: Vec<Value> = TRIGGER_PREFIX_LABELS[..3]
        .iter()
        .map(|label| serde_json::json!({"label": label}))
        .collect();
    assert!(!trigger_prefix_set_is_exact(&missing_name));

    let mut extra = exact.clone();
    extra.push(serde_json::json!({"label": "HX-Target"}));
    assert!(!trigger_prefix_set_is_exact(&extra));

    let mut reordered = exact;
    reordered.swap(2, 3);
    assert!(!trigger_prefix_set_is_exact(&reordered));
}

#[test]
fn scenario_70_htmx_server_header_workflows() {
    run_ux_scenario(
        "htmx_server_header_workflows",
        SCENARIO_FILE,
        "scenario_70_htmx_server_header_workflows",
        UxCiTier::Pr,
        Some(UxComponent::Completion),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            for (index, stack) in stacks().into_iter().enumerate() {
                prove_stack(recorder, stack, index == 0)?;
            }
            Ok(())
        },
    );
}
