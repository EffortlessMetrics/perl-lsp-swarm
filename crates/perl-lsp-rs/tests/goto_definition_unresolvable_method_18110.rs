//! Regression tests for #18110: goto-definition on an unresolvable
//! `$self->method` name must not return the enclosing sub's span.
//!
//! Adopted from the uncommitted probe `probe_gotodef_confirm.rs` (scenarios 5,
//! 6, and 7 of the #7451 probe family), converted from print-and-eyeball into
//! assertions. The same-file tier of `handle_definition` accepted
//! `SemanticModel::definition_at` unconditionally, and the semantic analyzer's
//! containing-symbol fallback answered an unknown method selector with the sub
//! already holding the cursor — a bogus jump target for UNIVERSAL builtins
//! (`isa`/`can`/`VERSION`), glob-aliased dispatch, and plain typos.

mod support;

use serde_json::{Value, json};
use support::lsp_harness::{LspHarness, TempWorkspace};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// (line, character) of the `occurrence`-th (0-based) occurrence of `needle`.
fn pos_of(source: &str, needle: &str, occurrence: usize) -> Result<(u32, u32), String> {
    let mut seen = 0;
    for (line_idx, line) in source.lines().enumerate() {
        let mut from = 0;
        while let Some(idx) = line[from..].find(needle) {
            let abs = from + idx;
            if seen == occurrence {
                return Ok((line_idx as u32, abs as u32));
            }
            seen += 1;
            from = abs + 1;
        }
    }
    Err(format!("needle {needle:?} occurrence {occurrence} not found in:\n{source}"))
}

fn describe(result: &Value) -> String {
    match result {
        Value::Null => "null".to_string(),
        Value::Array(items) if items.is_empty() => "[]".to_string(),
        Value::Array(items) => items
            .iter()
            .map(|item| {
                let uri = item.get("uri").and_then(Value::as_str).unwrap_or("<?>");
                let point = |side: &str| {
                    item.pointer(&format!("/range/{side}"))
                        .map(|p| {
                            format!(
                                "{}:{}",
                                p.get("line").and_then(Value::as_u64).unwrap_or(99_999),
                                p.get("character").and_then(Value::as_u64).unwrap_or(99_999)
                            )
                        })
                        .unwrap_or_else(|| "?".into())
                };
                format!("{uri} @{}-{}", point("start"), point("end"))
            })
            .collect::<Vec<_>>()
            .join("; "),
        other => format!("{other}"),
    }
}

fn goto_def(
    harness: &mut LspHarness,
    uri: &str,
    line: u32,
    character: u32,
) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(harness.request(
        "textDocument/definition",
        json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        }),
    )?)
}

/// #18110: "no definition" is a clean refusal — either `null` or an empty
/// location array — never a location pointing back into the caller's own sub.
fn assert_no_definition(result: &Value, label: &str) {
    let clean = result.is_null() || result.as_array().is_some_and(|items| items.is_empty());
    assert!(
        clean,
        "{label} must refuse without an enclosing-sub target, got: {}",
        describe(result)
    );
}

// ── Scenario: UNIVERSAL builtins isa / can / VERSION ────────────────────────

const WIDGET_PM: &str = r#"package Widget;
use strict;
use warnings;

sub new {
    my ($class) = @_;
    return bless {}, $class;
}

sub check {
    my ($self) = @_;
    return $self->isa('Widget')
        && $self->can('check')
        && defined $self->VERSION;
}

1;
"#;

#[test]
fn universal_builtin_methods_do_not_return_enclosing_sub() -> TestResult {
    let workspace = TempWorkspace::new()?;
    workspace.write("lib/Widget.pm", WIDGET_PM)?;

    let mut harness = LspHarness::new();
    harness.initialize_with_root(&workspace.root_uri, None)?;

    let widget_uri = workspace.uri("lib/Widget.pm");
    harness.open(&widget_uri, WIDGET_PM)?;
    harness.barrier();

    // `sub check` spans lines 9..14 (0-based); the junk answer named in the
    // issue is exactly that span. isa sits at 11:18, can at 12:18, VERSION at
    // 13:26 — on the method selector after the arrow.
    for (needle, occurrence) in [("->isa", 0), ("->can", 0), ("->VERSION", 0)] {
        let (line, character) = pos_of(WIDGET_PM, needle, occurrence)?;
        let result = goto_def(&mut harness, &widget_uri, line, character + 2)?;
        assert_no_definition(&result, &format!("goto-def on `{needle}` (a UNIVERSAL builtin)"));
    }
    Ok(())
}

// ── Scenario: glob-aliased dispatch *bar = &foo ─────────────────────────────

const UTIL_PM: &str = r#"package Util;
use strict;
use warnings;

sub new {
    my ($class) = @_;
    return bless {}, $class;
}

sub foo {
    return 'util-foo';
}

*bar = &foo;

sub user {
    my ($self) = @_;
    return $self->bar;
}

1;
"#;

#[test]
fn glob_aliased_method_does_not_return_enclosing_sub() -> TestResult {
    let workspace = TempWorkspace::new()?;
    workspace.write("lib/Util.pm", UTIL_PM)?;

    let mut harness = LspHarness::new();
    harness.initialize_with_root(&workspace.root_uri, None)?;

    let util_uri = workspace.uri("lib/Util.pm");
    harness.open(&util_uri, UTIL_PM)?;
    harness.barrier();

    // `sub user` spans lines 15..18 (0-based). The glob alias makes `bar`
    // undispatchable for the analyzer; the answer must stay a clean refusal,
    // not the enclosing `sub user`.
    let (line, character) = pos_of(UTIL_PM, "->bar", 0)?;
    let result = goto_def(&mut harness, &util_uri, line, character + 2)?;
    assert_no_definition(&result, "goto-def on glob-aliased `$self->bar`");
    Ok(())
}

// ── Scenario: unknown method + the two controls ─────────────────────────────

const PARENT_PM: &str = r#"package Parent;
use strict;
use warnings;

sub new {
    my ($class, %args) = @_;
    return bless \%args, $class;
}

sub foo {
    return 'parent-foo';
}

1;
"#;

const CHILD_PM: &str = r#"package Child;
use strict;
use warnings;
use parent 'Parent';

sub demo {
    my ($self) = @_;
    my $ok = $self->no_such_method_xyz;
    return $self->foo;
}

1;
"#;

const CALLER_PL: &str = r#"use strict;
use warnings;
use lib 'lib';
use Child;

my $c = Child->new;
$c->demo;
$c->no_such_method_xyz;
"#;

#[test]
fn unknown_self_method_refuses_while_known_method_and_cross_file_control_hold() -> TestResult {
    let workspace = TempWorkspace::new()?;
    workspace.write("lib/Parent.pm", PARENT_PM)?;
    workspace.write("lib/Child.pm", CHILD_PM)?;
    workspace.write("main.pl", CALLER_PL)?;

    let mut harness = LspHarness::new();
    harness.initialize_with_root(&workspace.root_uri, None)?;

    let parent_uri = workspace.uri("lib/Parent.pm");
    let child_uri = workspace.uri("lib/Child.pm");
    let main_uri = workspace.uri("main.pl");
    for (uri, content) in [(&parent_uri, PARENT_PM), (&child_uri, CHILD_PM), (&main_uri, CALLER_PL)]
    {
        harness.open(uri, content)?;
    }
    harness.barrier();

    // `sub demo` spans lines 5..9 (0-based). The typo'd method must not point
    // back into it.
    let (line, character) = pos_of(CHILD_PM, "->no_such_method_xyz", 0)?;
    let result = goto_def(&mut harness, &child_uri, line, character + 2)?;
    assert_no_definition(&result, "goto-def on unknown `$self->no_such_method_xyz`");

    // Control 1: the known inherited `$self->foo` still resolves to Parent.
    let (foo_line, foo_character) = pos_of(CHILD_PM, "->foo", 0)?;
    let result = goto_def(&mut harness, &child_uri, foo_line, foo_character + 2)?;
    let locations = result.as_array().filter(|items| !items.is_empty()).ok_or_else(|| {
        format!("known `$self->foo` must still resolve, got: {}", describe(&result))
    })?;
    assert_eq!(
        locations.len(),
        1,
        "known `$self->foo` resolves to exactly Parent::foo, got: {}",
        describe(&result)
    );
    let location = &locations[0];
    assert!(
        location.get("uri").and_then(Value::as_str).is_some_and(|uri| uri.ends_with("Parent.pm")),
        "`$self->foo` must land in Parent.pm, got: {}",
        describe(&result)
    );
    assert_eq!(
        location.pointer("/range/start/line").and_then(Value::as_u64),
        Some(9),
        "`$self->foo` must land on `sub foo` (0-based line 9), got: {}",
        describe(&result)
    );

    // Control 2: the SAME unknown method on `$c` in another file already
    // refused cleanly before the fix; it must keep doing so.
    let (line, character) = pos_of(CALLER_PL, "->no_such_method_xyz", 0)?;
    let result = goto_def(&mut harness, &main_uri, line, character + 2)?;
    assert_no_definition(&result, "goto-def on unknown `$c->no_such_method_xyz`");
    Ok(())
}
