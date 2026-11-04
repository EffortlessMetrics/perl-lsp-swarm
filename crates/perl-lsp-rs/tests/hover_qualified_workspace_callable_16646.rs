//! Hover must not treat a workspace-qualified `Pkg::sub` as a missing CPAN module (#16646).
//!
//! Production seam: `textDocument/hover` on an open caller after the defining
//! file is also open and indexed. Same-file qualified-call tests are not this
//! seam: they never reach `PossiblePackage`, and they accept a `Perl` card that
//! can still contain `cpanm`.
//!
//! Non-goals: #8670 POD rendering, #16647 never-opened defining files.

mod support;

use std::time::Duration;

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
    // Missing-module hover walks configured include roots; 600ms (the two-thread
    // default) is too tight for that retention case on this proof file.
    Ok(harness.request_with_timeout(
        "textDocument/hover",
        json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        }),
        Duration::from_secs(3),
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

fn assert_proven_callable(value: &str, package: &str, qualified: &str) {
    assert_not_missing_module_card(value, qualified);
    assert!(
        value.contains(&format!("Defined in `{package}`"))
            || value.contains(&format!("sub {qualified}")),
        "hover should present the indexed callable `{qualified}` in `{package}`, got: {value}"
    );
}

fn assert_module_receiver_card(value: &str, package: &str) {
    assert!(
        !value.starts_with("**Perl**: `"),
        "package receiver `{package}` must not collapse to the generic callable card, got: {value}"
    );
    assert!(
        !value.contains("**Subroutine**"),
        "package receiver `{package}` must not use the subroutine card, got: {value}"
    );
    assert!(
        !value.contains(&format!("Defined in `{package}`")),
        "package receiver `{package}` must not be presented as a proven sub, got: {value}"
    );
    assert!(
        !value.contains(&format!("sub {package}")),
        "package receiver `{package}` must not render as a subroutine, got: {value}"
    );
    assert!(
        value.contains(package),
        "package-receiver hover should mention `{package}`, got: {value}"
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

    assert_proven_callable(&value, "PodHeavy", "PodHeavy::documented_sub");
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

    assert_proven_callable(&value, "PodHeavy", "PodHeavy::documented_sub");
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

    assert_proven_callable(&value, "Foo::Bar", "Foo::Bar::helper");
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
    assert!(
        hover_markdown(&result).is_none(),
        "qualified name in a comment must fail closed, got: {result:?}"
    );
    Ok(())
}

/// Nested `Foo::Bar::run` must not prove a parent `Foo::run()` callable.
#[test]
fn hover_on_parent_call_is_not_proven_by_nested_package_sub() -> TestResult {
    const NESTED: &str = r#"package Foo::Bar;
sub run { return 1; }
1;
"#;
    const SCRIPT: &str = "print Foo::run();\n";

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///lib/Foo/Bar.pm", NESTED)?;
    harness.open("file:///app.pl", SCRIPT)?;
    harness.barrier();

    let (line, character) = pos_on_line(SCRIPT, 0, "run")?;
    let result = hover_at(&mut harness, "file:///app.pl", line, character)?;
    let value = hover_markdown(&result).ok_or("expected hover content on Foo::run")?;

    assert_not_missing_module_card(&value, "Foo::run");
    assert!(
        !value.contains("Defined in `Foo`"),
        "nested Foo::Bar::run must not prove Foo::run, got: {value}"
    );
    assert!(
        !value.contains("sub Foo::run"),
        "nested Foo::Bar::run must not render as sub Foo::run, got: {value}"
    );
    assert!(
        !value.contains("**Subroutine**"),
        "unresolved parent call must not get a subroutine card from a nested member, got: {value}"
    );
    Ok(())
}

/// Direct `Foo::run` still wins when a nested `Foo::Bar::run` also exists.
#[test]
fn hover_on_parent_call_uses_direct_member_not_nested_namesake() -> TestResult {
    const PARENT: &str = r#"package Foo;
sub run { return 1; }
1;
"#;
    const NESTED: &str = r#"package Foo::Bar;
sub run { return 1; }
1;
"#;
    const SCRIPT: &str = "print Foo::run();\n";

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///lib/Foo.pm", PARENT)?;
    harness.open("file:///lib/Foo/Bar.pm", NESTED)?;
    harness.open("file:///app.pl", SCRIPT)?;
    harness.barrier();

    let (line, character) = pos_on_line(SCRIPT, 0, "run")?;
    let result = hover_at(&mut harness, "file:///app.pl", line, character)?;
    let value = hover_markdown(&result).ok_or("expected hover content on Foo::run")?;

    assert_proven_callable(&value, "Foo", "Foo::run");
    assert!(
        !value.contains("Defined in `Foo::Bar`"),
        "direct Foo::run must not be attributed to Foo::Bar, got: {value}"
    );
    Ok(())
}

/// Lowercase `pkg::name` before a wrapped `->` is a package receiver, not a
/// callable. Same-line `pkg::name->method` and a real `pkg::name::sub()` call
/// stay on their existing paths.
#[test]
fn hover_on_lowercase_multiline_arrow_receiver_is_module_not_callable() -> TestResult {
    const MODULE: &str = r#"package foo::bar;
sub make { return 1; }
1;
"#;
    const SCRIPT: &str = "foo::bar->make();\nfoo::bar\n  ->make();\nprint foo::bar::make();\n";

    let workspace = support::lsp_harness::TempWorkspace::new()?;
    workspace.write("lib/foo/bar.pm", MODULE)?;
    workspace.write("caller.pl", SCRIPT)?;

    let mut harness = LspHarness::new();
    harness.initialize_with_root(&workspace.root_uri, None)?;
    harness.open(&workspace.uri("lib/foo/bar.pm"), MODULE)?;
    harness.open(&workspace.uri("caller.pl"), SCRIPT)?;
    harness.barrier();

    let caller = workspace.uri("caller.pl");
    let (same_line, same_col) = pos_on_line(SCRIPT, 0, "bar")?;
    let same = hover_markdown(&hover_at(&mut harness, &caller, same_line, same_col)?)
        .ok_or("expected hover on same-line foo::bar->make()")?;
    assert_module_receiver_card(&same, "foo::bar");

    let (nl_line, nl_col) = pos_on_line(SCRIPT, 1, "bar")?;
    let wrapped = hover_markdown(&hover_at(&mut harness, &caller, nl_line, nl_col)?)
        .ok_or("expected hover on lowercase foo::bar before newline+arrow")?;
    assert_module_receiver_card(&wrapped, "foo::bar");

    let (call_line, call_col) = pos_on_line(SCRIPT, 3, "make")?;
    let call = hover_markdown(&hover_at(&mut harness, &caller, call_line, call_col)?)
        .ok_or("expected hover on foo::bar::make()")?;
    assert_proven_callable(&call, "foo::bar", "foo::bar::make");
    Ok(())
}

/// `$Pkg::sub` is a package variable, not the same-named subroutine.
#[test]
fn hover_on_sigiled_qualified_name_is_not_subroutine_card() -> TestResult {
    let mut harness = LspHarness::new();
    open_podheavy_workspace(&mut harness)?;

    const SCRIPT: &str = "print $PodHeavy::documented_sub;\n";
    harness.open("file:///script/sigil_caller.pl", SCRIPT)?;
    harness.barrier();

    let (line, character) = pos_on_line(SCRIPT, 0, "documented_sub")?;
    let result = hover_at(&mut harness, "file:///script/sigil_caller.pl", line, character)?;
    if let Some(value) = hover_markdown(&result) {
        assert!(
            !value.contains("**Subroutine**"),
            "sigiled $PodHeavy::documented_sub must not use the subroutine card, got: {value}"
        );
        assert!(
            !value.contains("Defined in `PodHeavy`"),
            "sigiled $PodHeavy::documented_sub must not be a proven sub, got: {value}"
        );
        assert_not_missing_module_card(&value, "PodHeavy::documented_sub");
    }
    Ok(())
}
