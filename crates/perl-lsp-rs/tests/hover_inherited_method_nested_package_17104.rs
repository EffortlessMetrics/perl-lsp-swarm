//! Inherited-method hover must not credit a nested-package descendant as a
//! parent-package member (#17104 Finding A, hover slice).
//!
//! Production seam: `textDocument/hover` on a method call whose receiver is
//! package `Child`, after `Child`, `Child::Nested`, and `Base` are open and
//! indexed. `WorkspaceIndex::get_package_members("Child")` also returns
//! `Child::Nested::*`, so a bare-name match treats `sub Child::Nested::helper`
//! as `sub Child::helper`.
//!
//! Receiver form: hover resolves `$self`/`$this`/`$class` and uppercase bare
//! packages (`resolve_receiver_package_name`). An opaque `$obj = Child->new()`
//! receiver does not enter this path, so the fixtures use `$self` inside
//! `Child` (typed as Child) rather than an unresolved `$obj`.
//!
//! Non-goals: completion `packages.rs` / `workspace.rs` consumers, index-query
//! shape, BFS order, and card wording beyond ownership.

mod support;

use std::time::Duration;

use serde_json::{Value, json};
use support::lsp_harness::{LspHarness, TempWorkspace};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const BASE: &str = r#"package Base;

sub new {
    my ($class) = @_;
    return bless {}, $class;
}

sub helper {
    return 1;
}

1;
"#;

const CHILD_WITHOUT_HELPER: &str = r#"package Child;
use parent -norequire, 'Base';

sub new {
    my ($class) = @_;
    return bless {}, $class;
}

sub run {
    my $self = shift;
    $self->helper;
}

1;
"#;

const CHILD_WITH_HELPER: &str = r#"package Child;
use parent -norequire, 'Base';

sub helper {
    return 2;
}

sub run {
    my $self = shift;
    $self->helper;
}

1;
"#;

const NESTED_HELPER: &str = r#"package Child::Nested;

sub helper {
    return 3;
}

1;
"#;

const NESTED_AUTOLOAD: &str = r#"package Child::Nested;

sub AUTOLOAD {
    our $AUTOLOAD;
    return $AUTOLOAD;
}

1;
"#;

const CHILD_MISSING_METHOD: &str = r#"package Child;
use parent -norequire, 'Base';

sub run {
    my $self = shift;
    $self->ghost;
}

1;
"#;

const CHILD_WITH_AUTOLOAD: &str = r#"package Child;
use parent -norequire, 'Base';

sub AUTOLOAD {
    our $AUTOLOAD;
    return $AUTOLOAD;
}

sub run {
    my $self = shift;
    $self->ghost;
}

1;
"#;

const CALLER: &str = "Child->helper;\n";

fn hover_markdown(result: &Value) -> Option<String> {
    result
        .get("contents")
        .and_then(|contents| contents.get("value"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn pos_on_line(
    source: &str,
    line: usize,
    needle: &str,
) -> Result<(u32, u32), Box<dyn std::error::Error>> {
    let text = source.lines().nth(line).ok_or_else(|| format!("no line {line} in test source"))?;
    let col = text.find(needle).ok_or_else(|| format!("`{needle}` not found on line {line}"))?;
    Ok((line as u32, col as u32))
}

fn hover_at(
    harness: &mut LspHarness,
    uri: &str,
    line: u32,
    character: u32,
) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(harness.request_with_timeout(
        "textDocument/hover",
        json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        }),
        Duration::from_secs(3),
    )?)
}

fn open_workspace(
    harness: &mut LspHarness,
    files: &[(&str, &str)],
    wait_symbol: &str,
    wait_uri_suffix: &str,
) -> Result<TempWorkspace, Box<dyn std::error::Error>> {
    let workspace = TempWorkspace::new()?;
    for (relative, source) in files {
        workspace.write(relative, source)?;
    }

    harness.initialize_with_root(&workspace.root_uri, None)?;
    for (relative, source) in files {
        harness.open(&workspace.uri(relative), source)?;
    }
    harness.barrier();
    wait_indexed(harness, &workspace, wait_symbol, wait_uri_suffix)?;
    Ok(workspace)
}

fn wait_indexed(
    harness: &mut LspHarness,
    workspace: &TempWorkspace,
    symbol: &str,
    relative: &str,
) -> TestResult {
    harness.wait_for_symbol(symbol, Some(&workspace.uri(relative)), Duration::from_secs(5))?;
    Ok(())
}

fn line_of(source: &str, needle: &str) -> Result<usize, Box<dyn std::error::Error>> {
    source
        .lines()
        .position(|line| line.contains(needle))
        .ok_or_else(|| format!("`{needle}` not found in test source").into())
}

fn hover_self_call(
    harness: &mut LspHarness,
    workspace: &TempWorkspace,
    child_source: &str,
    method: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let child_uri = workspace.uri("lib/Child.pm");
    let call = format!("$self->{method}");
    let line = line_of(child_source, &call)?;
    let (line, character) = pos_on_line(child_source, line, method)?;
    hover_at(harness, &child_uri, line, character)
}

fn hover_child_arrow_call(
    harness: &mut LspHarness,
    workspace: &TempWorkspace,
    method: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let caller_uri = workspace.uri("lib/caller.pl");
    let line = line_of(CALLER, &format!("Child->{method}"))?;
    let (line, character) = pos_on_line(CALLER, line, method)?;
    hover_at(harness, &caller_uri, line, character)
}

/// Nested `Child::Nested::helper` must not be reported as `Child`'s own method
/// when the real definition lives on `Base`.
#[test]
fn inherited_hover_does_not_credit_nested_package_helper_to_child() -> TestResult {
    let mut harness = LspHarness::new();
    let workspace = open_workspace(
        &mut harness,
        &[
            ("lib/Base.pm", BASE),
            ("lib/Child.pm", CHILD_WITHOUT_HELPER),
            ("lib/Child/Nested.pm", NESTED_HELPER),
        ],
        "helper",
        "lib/Base.pm",
    )?;
    wait_indexed(&mut harness, &workspace, "helper", "lib/Child/Nested.pm")?;

    let result = hover_self_call(&mut harness, &workspace, CHILD_WITHOUT_HELPER, "helper")?;
    let value = hover_markdown(&result).ok_or("expected hover content on Child $self->helper")?;

    assert!(
        value.contains("Inherited from `Base`"),
        "Child::Nested::helper must not steal Child's inherited Base::helper, got: {value}"
    );
    assert!(
        value.contains("sub Base::helper"),
        "inherited hover must name `sub Base::helper`, got: {value}"
    );
    assert!(
        !value.contains("Defined in `Child`"),
        "nested Child::Nested::helper must not be reported as Defined in Child, got: {value}"
    );
    assert!(
        !value.contains("sub Child::helper"),
        "nested Child::Nested::helper must not render as sub Child::helper, got: {value}"
    );
    Ok(())
}

/// Nested `Child::Nested::AUTOLOAD` must not become Child's dynamic boundary.
#[test]
fn autoload_hover_does_not_credit_nested_package_autoload_to_child() -> TestResult {
    let mut harness = LspHarness::new();
    let workspace = open_workspace(
        &mut harness,
        &[
            ("lib/Base.pm", BASE),
            ("lib/Child.pm", CHILD_MISSING_METHOD),
            ("lib/Child/Nested.pm", NESTED_AUTOLOAD),
        ],
        "AUTOLOAD",
        "lib/Child/Nested.pm",
    )?;

    let result = hover_self_call(&mut harness, &workspace, CHILD_MISSING_METHOD, "ghost")?;
    let value = hover_markdown(&result).unwrap_or_default();
    assert!(
        !value.contains("Resolved via `AUTOLOAD` in `Child`"),
        "Child::Nested::AUTOLOAD must not resolve as Child's AUTOLOAD, got: {value}"
    );
    assert!(
        !value.contains("sub Child::AUTOLOAD"),
        "Child::Nested::AUTOLOAD must not render as sub Child::AUTOLOAD, got: {value}"
    );
    Ok(())
}

/// A real `Child::AUTOLOAD` still wins when `Child::Nested::AUTOLOAD` also exists.
#[test]
fn autoload_hover_keeps_real_child_autoload_when_nested_namesake_exists() -> TestResult {
    let mut harness = LspHarness::new();
    let workspace = open_workspace(
        &mut harness,
        &[
            ("lib/Base.pm", BASE),
            ("lib/Child.pm", CHILD_WITH_AUTOLOAD),
            ("lib/Child/Nested.pm", NESTED_AUTOLOAD),
        ],
        "AUTOLOAD",
        "lib/Child.pm",
    )?;
    wait_indexed(&mut harness, &workspace, "AUTOLOAD", "lib/Child/Nested.pm")?;

    let result = hover_self_call(&mut harness, &workspace, CHILD_WITH_AUTOLOAD, "ghost")?;
    let value = hover_markdown(&result)
        .ok_or("expected hover content for a real Child::AUTOLOAD handler")?;

    assert!(
        value.contains("Resolved via `AUTOLOAD` in `Child`"),
        "a real Child::AUTOLOAD must still be Child's dynamic boundary, got: {value}"
    );
    assert!(
        value.contains("sub Child::AUTOLOAD"),
        "a real Child::AUTOLOAD must render as sub Child::AUTOLOAD, got: {value}"
    );
    Ok(())
}

/// A real `Child::helper` still wins when `Child::Nested::helper` also exists.
///
/// Hover from a separate caller so the request reaches the workspace inherited
/// path (`Child->helper`) instead of the same-file subroutine card.
#[test]
fn inherited_hover_keeps_real_child_helper_when_nested_namesake_exists() -> TestResult {
    let mut harness = LspHarness::new();
    let workspace = open_workspace(
        &mut harness,
        &[
            ("lib/Base.pm", BASE),
            ("lib/Child.pm", CHILD_WITH_HELPER),
            ("lib/Child/Nested.pm", NESTED_HELPER),
            ("lib/caller.pl", CALLER),
        ],
        "helper",
        "lib/Child.pm",
    )?;

    let result = hover_child_arrow_call(&mut harness, &workspace, "helper")?;
    let value = hover_markdown(&result).ok_or("expected hover content on real Child::helper")?;

    assert!(
        value.contains("Defined in `Child`"),
        "a real Child::helper must stay Defined in Child, got: {value}"
    );
    assert!(
        value.contains("sub Child::helper"),
        "a real Child::helper must render as sub Child::helper, got: {value}"
    );
    assert!(
        !value.contains("Defined in `Child::Nested`"),
        "direct Child::helper must not be attributed to Child::Nested, got: {value}"
    );
    Ok(())
}
