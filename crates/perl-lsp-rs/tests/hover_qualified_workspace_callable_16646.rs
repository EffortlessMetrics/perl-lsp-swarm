//! Hover must not treat a workspace-qualified `Pkg::sub` as a missing CPAN module (#16646).
//!
//! Production seam: `textDocument/hover` on an open caller after the defining
//! file is also open and indexed. Same-file qualified-call tests are not this
//! seam: they never reach `PossiblePackage`, and they accept a `Perl` card that
//! can still contain `cpanm`.
//!
//! Non-goals: #8670 POD rendering, #16647 never-opened defining files.

mod support;

use serde_json::{Value, json};
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const PODHEAVY: &str = r#"package PodHeavy;
use strict;
use warnings;

sub documented_sub {
    return 1;
}

1;
"#;

const CALLER: &str = r#"use strict;
use warnings;

print PodHeavy::documented_sub(), "\n";
print &PodHeavy::documented_sub;
"#;

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
    Ok(harness.request(
        "textDocument/hover",
        json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        }),
    )?)
}

fn assert_not_missing_module_card(value: &str, qualified: &str) {
    assert!(
        !value.contains("cpanm"),
        "workspace-qualified `{qualified}` must not recommend cpanm, got: {value}"
    );
    assert!(
        !value.contains("Not found in workspace or configured include paths"),
        "workspace-qualified `{qualified}` must not use the missing-module card, got: {value}"
    );
    assert!(
        !value.contains(&format!("metacpan.org/pod/{qualified}")),
        "workspace-qualified `{qualified}` must not be advertised as a CPAN module, got: {value}"
    );
}

fn open_podheavy_workspace(harness: &mut LspHarness) -> TestResult {
    harness.initialize(None)?;
    harness.open("file:///lib/PodHeavy.pm", PODHEAVY)?;
    harness.open("file:///script/pod_caller.pl", CALLER)?;
    harness.barrier();
    Ok(())
}

/// The issue's production case: hover on the last component of an indexed
/// `Pkg::sub()` call must return callable information, not install advice.
#[test]
fn hover_on_indexed_qualified_workspace_sub_is_callable_not_cpanm() -> TestResult {
    let mut harness = LspHarness::new();
    open_podheavy_workspace(&mut harness)?;

    let (line, character) = pos_on_line(CALLER, 3, "documented_sub")?;
    let result = hover_at(&mut harness, "file:///script/pod_caller.pl", line, character)?;
    let value =
        hover_markdown(&result).ok_or("expected hover content on PodHeavy::documented_sub")?;

    assert_not_missing_module_card(&value, "PodHeavy::documented_sub");
    assert!(value.contains("documented_sub"), "hover should name the indexed sub, got: {value}");
    assert!(
        value.contains("Subroutine")
            || value.contains("Method")
            || value.contains("sub PodHeavy::documented_sub")
            || value.contains("`sub documented_sub"),
        "hover should present callable information, got: {value}"
    );
    Ok(())
}

/// Cursor on the package prefix of `Pkg::sub()` must not cpanm the whole
/// qualified name. Definition already refuses that prefix as the callable.
#[test]
fn hover_on_qualified_call_package_prefix_does_not_cpanm_the_sub() -> TestResult {
    let mut harness = LspHarness::new();
    open_podheavy_workspace(&mut harness)?;

    let (line, character) = pos_on_line(CALLER, 3, "PodHeavy::")?;
    let result = hover_at(&mut harness, "file:///script/pod_caller.pl", line, character)?;
    let value = hover_markdown(&result)
        .ok_or("expected hover on the PodHeavy prefix of PodHeavy::documented_sub")?;
    assert!(
        !value.contains("cpanm PodHeavy::documented_sub"),
        "prefix hover must not install the qualified sub name, got: {value}"
    );
    assert!(
        !value.contains("metacpan.org/pod/PodHeavy::documented_sub"),
        "prefix hover must not advertise the qualified sub as a CPAN module, got: {value}"
    );
    assert!(
        !value.contains("**PodHeavy::documented_sub**"),
        "prefix hover must not title the card as the qualified sub, got: {value}"
    );
    Ok(())
}

/// Ampersand-call form is still a qualified callable, not a module.
#[test]
fn hover_on_ampersand_qualified_workspace_sub_is_callable_not_cpanm() -> TestResult {
    let mut harness = LspHarness::new();
    open_podheavy_workspace(&mut harness)?;

    let (line, character) = pos_on_line(CALLER, 4, "documented_sub")?;
    let result = hover_at(&mut harness, "file:///script/pod_caller.pl", line, character)?;
    let value =
        hover_markdown(&result).ok_or("expected hover content on &PodHeavy::documented_sub")?;

    assert_not_missing_module_card(&value, "PodHeavy::documented_sub");
    assert!(
        value.contains("documented_sub"),
        "ampersand-call hover should name the indexed sub, got: {value}"
    );
    Ok(())
}

/// Three-component `Foo::Bar::helper()` must use the same callable path.
#[test]
fn hover_on_three_component_qualified_workspace_sub_is_callable_not_cpanm() -> TestResult {
    const MODULE: &str = r#"package Foo::Bar;
sub helper { return 1; }
1;
"#;
    const SCRIPT: &str = "print Foo::Bar::helper();\n";

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///lib/Foo/Bar.pm", MODULE)?;
    harness.open("file:///app.pl", SCRIPT)?;
    harness.barrier();

    let (line, character) = pos_on_line(SCRIPT, 0, "helper")?;
    let result = hover_at(&mut harness, "file:///app.pl", line, character)?;
    let value = hover_markdown(&result).ok_or("expected hover content on Foo::Bar::helper")?;

    assert_not_missing_module_card(&value, "Foo::Bar::helper");
    assert!(
        value.contains("helper"),
        "three-component hover should name the indexed sub, got: {value}"
    );
    Ok(())
}

/// Retention: a genuine missing `use` still gets the missing-module card.
#[test]
fn hover_on_missing_use_module_still_recommends_cpanm() -> TestResult {
    const SCRIPT: &str = "use Some::Missing::Module::Nope;\n";

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///missing_use.pl", SCRIPT)?;
    harness.barrier();

    let (line, character) = pos_on_line(SCRIPT, 0, "Some::Missing")?;
    let result = hover_at(&mut harness, "file:///missing_use.pl", line, character)?;
    let value = hover_markdown(&result)
        .ok_or("expected missing-module hover for use Some::Missing::Module::Nope")?;

    assert!(
        value.contains("cpanm Some::Missing::Module::Nope"),
        "genuine missing use must keep the cpanm card, got: {value}"
    );
    assert!(
        value.contains("Not found in workspace or configured include paths"),
        "genuine missing use must keep the not-found module card, got: {value}"
    );
    Ok(())
}

/// Retention: `require MyApp::Worker` is a module reference, not a qualified sub.
#[test]
fn hover_on_require_module_is_module_hover_not_sub_hover() -> TestResult {
    const MODULE: &str = r#"package MyApp::Worker;
sub run { return 1; }
1;
"#;
    const SCRIPT: &str = "require MyApp::Worker;\n";

    let workspace = support::lsp_harness::TempWorkspace::new()?;
    workspace.write("lib/MyApp/Worker.pm", MODULE)?;
    workspace.write("require_worker.pl", SCRIPT)?;

    let mut harness = LspHarness::new();
    harness.initialize_with_root(&workspace.root_uri, None)?;
    harness.open(&workspace.uri("lib/MyApp/Worker.pm"), MODULE)?;
    harness.open(&workspace.uri("require_worker.pl"), SCRIPT)?;
    harness.barrier();

    let (line, character) = pos_on_line(SCRIPT, 0, "MyApp::Worker")?;
    let result = hover_at(&mut harness, &workspace.uri("require_worker.pl"), line, character)?;
    let value = hover_markdown(&result).ok_or("expected module hover for require MyApp::Worker")?;

    assert!(value.contains("MyApp::Worker"), "require hover should name the module, got: {value}");
    assert!(
        !value.contains("cpanm MyApp::Worker"),
        "an indexed required module must not be reported missing, got: {value}"
    );
    assert!(
        !value.contains("sub MyApp::Worker"),
        "require MyApp::Worker must not be presented as a subroutine, got: {value}"
    );
    Ok(())
}

/// Unresolved `Package::call()` must not be sold as a proven workspace sub,
/// and must not reuse the missing-module install card.
#[test]
fn hover_on_unresolved_qualified_call_is_not_proven_sub_or_cpanm_module() -> TestResult {
    const SCRIPT: &str = "print Missing::definitely_not_a_workspace_sub();\n";

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///unresolved.pl", SCRIPT)?;
    harness.barrier();

    let (line, character) = pos_on_line(SCRIPT, 0, "definitely_not_a_workspace_sub")?;
    let result = hover_at(&mut harness, "file:///unresolved.pl", line, character)?;
    let value =
        hover_markdown(&result).ok_or("expected some hover on an unresolved qualified call")?;

    assert_not_missing_module_card(&value, "Missing::definitely_not_a_workspace_sub");
    assert!(
        !value.contains("Defined in `Missing`"),
        "unresolved qualified call must not be presented as a proven workspace sub, got: {value}"
    );
    assert!(
        !value.contains("**Subroutine**"),
        "unresolved qualified call must not get a subroutine card, got: {value}"
    );
    Ok(())
}

/// Retention: `File::Path->method` is still a package/module hover.
#[test]
fn hover_on_arrow_receiver_package_keeps_module_card() -> TestResult {
    const SCRIPT: &str = "File::Path->make_path('/tmp/test');\n";

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///pkg_hover_16646.pl", SCRIPT)?;
    harness.barrier();

    let result = hover_at(&mut harness, "file:///pkg_hover_16646.pl", 0, 0)?;
    let value = hover_markdown(&result).ok_or("expected hover content for File::Path")?;

    assert!(
        !value.starts_with("**Perl**: `"),
        "File::Path receiver should not collapse to the bare-token card, got: {value}"
    );
    assert!(
        value.contains("File::Path"),
        "arrow-receiver hover should mention the package, got: {value}"
    );
    assert!(
        value.contains("metacpan.org"),
        "arrow-receiver package hover should keep MetaCPAN, got: {value}"
    );
    Ok(())
}

/// Source-region gate: a qualified name inside a comment must not reach cpanm.
#[test]
fn hover_on_qualified_sub_in_comment_does_not_cpanm() -> TestResult {
    const SCRIPT: &str = "# print PodHeavy::documented_sub();\nprint 1;\n";

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///comment_qualified.pl", SCRIPT)?;
    harness.barrier();

    let (line, character) = pos_on_line(SCRIPT, 0, "documented_sub")?;
    let result = hover_at(&mut harness, "file:///comment_qualified.pl", line, character)?;
    if let Some(value) = hover_markdown(&result) {
        assert_not_missing_module_card(&value, "PodHeavy::documented_sub");
    }
    Ok(())
}
