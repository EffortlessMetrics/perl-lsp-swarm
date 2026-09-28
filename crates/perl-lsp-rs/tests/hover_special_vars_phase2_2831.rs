//! Integration tests for issue #2831 Phase 2 — special variable hover coverage.
//!
//! Phase 1 (#2839) added: $1–$9, $|, MetaCPAN links, builtin examples.
//! Phase 2 verifies hover docs for the high-value special variables that
//! were implemented but lacked dedicated integration tests:
//!   $_ (topic variable), @_ (subroutine arguments), $0 (program name),
//!   $! (errno), $@ (eval error), $/ (input record separator),
//!   $\ (output record separator), $, (output field separator),
//!   %ENV, %SIG, @INC, @ARGV, %INC, $^W (warnings flag), $^O (OS name).
//!
//! Cursor tests cover the sigil and following character for `@_` and `$|`.
//! Other cases target a character within the variable name. Declaration
//! contexts need explicit coverage because a containing subroutine can mask
//! the special-variable card when no variable declaration is found.

mod support;

use serde_json::json;
use support::lsp_harness::{LspHarness, TempWorkspace};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn hover_value(result: &serde_json::Value) -> Option<String> {
    result
        .get("contents")
        .and_then(|c| c.get("value"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// $_ — the topic / default variable
// ---------------------------------------------------------------------------

/// Hovering on $_ should return documentation about the default topic variable.
#[test]
fn test_hover_topic_variable_p2() -> TestResult {
    // "print $_ if $x;\n"
    //  0         1
    //  0123456789012345
    // $_ at offset 6; cursor on '_' at offset 7 to get the token "$_"
    let doc = "print $_ if $x;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///topic_var_p2.pl", doc)?;
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///topic_var_p2.pl"},
                "position": {"line": 0, "character": 7}
            }),
        )
        .unwrap_or(json!(null));
    let val = hover_value(&result).ok_or("Expected hover content for $_")?;
    let lower = val.to_lowercase();
    assert!(
        lower.contains("default") || lower.contains("topic") || lower.contains("$_"),
        "$_ hover should describe the topic/default variable, got: {val}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// @_ — subroutine arguments array
// ---------------------------------------------------------------------------

/// Hovering on @_ should explain subroutine argument passing.
#[test]
fn test_hover_subroutine_args_array_p2() -> TestResult {
    // "sub f { return @_; }\n"
    //  0         1         2
    //  01234567890123456789012
    // @_ at offset 15; cursor on '_' at offset 16 to get the token "@_"
    let doc = "sub f { return @_; }\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///subroutine_args_p2.pl", doc)?;
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///subroutine_args_p2.pl"},
                "position": {"line": 0, "character": 16}
            }),
        )
        .unwrap_or(json!(null));
    let val = hover_value(&result).ok_or("Expected hover content for @_")?;
    if !val.contains("**`@_` — Subroutine Arguments**") || val.contains("**Subroutine**") {
        return Err(format!("@_ must use the special-variable card, got: {val}").into());
    }
    Ok(())
}

/// Special variables inside a sub must retain their own identity for hover and
/// must not navigate to the containing sub's declaration.
#[test]
fn test_special_variables_in_sub_do_not_resolve_to_containing_sub() -> TestResult {
    let uri = "file:///special_variable_identity.pl";
    let doc = "sub demo {\n    my $a = 1;\n    my ($x) = @_;\n    local $| = 1;\n    # $| @_\n    my $literal = '$| @_';\n    my $quoted_lexical = '$a';\n    return $a;\n}\ndemo();\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document(uri, doc)?;

    for (line, character, heading) in [
        (2, 14, "**`@_` — Subroutine Arguments**"),
        (2, 15, "**`@_` — Subroutine Arguments**"),
        (3, 10, "**`$|` — Output Autoflush**"),
        (3, 11, "**`$|` — Output Autoflush**"),
    ] {
        let params = json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        });
        let hover = harness.request("textDocument/hover", params.clone())?;
        let value = hover_value(&hover).ok_or("Expected special-variable hover")?;
        if !value.contains(heading) || value.contains("**Subroutine**") {
            return Err(
                format!("expected {heading} instead of enclosing-sub card, got: {value}").into()
            );
        }

        let definition = harness.request("textDocument/definition", params)?;
        if !definition.is_null()
            && !definition.as_array().is_some_and(|locations| locations.is_empty())
        {
            return Err(format!(
                "special variable must not navigate to the containing sub: {definition}"
            )
            .into());
        }
    }

    for (line, character, expected_line) in [(7, 12, 1), (9, 2, 0)] {
        let definition = harness.request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character}
            }),
        )?;
        if definition.pointer("/0/range/start/line").and_then(|line| line.as_u64())
            != Some(expected_line)
        {
            return Err(
                format!("control should navigate to line {expected_line}: {definition}").into()
            );
        }
    }

    for (line, character) in [(4, 7), (4, 10), (5, 20), (5, 23), (6, 26), (6, 27)] {
        let params = json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        });
        let hover = harness.request("textDocument/hover", params.clone())?;
        if hover_value(&hover).is_some() {
            return Err(format!("comment/string gained hover: {hover}").into());
        }
        let definition = harness.request("textDocument/definition", params)?;
        if !definition.is_null()
            && !definition.as_array().is_some_and(|locations| locations.is_empty())
        {
            return Err(format!("inert source at ({line}, {character}) navigated to an unrelated declaration: {definition}").into());
        }
    }
    Ok(())
}

/// Quoted method names in modifiers are intentional navigation targets, unlike
/// the inert special-variable text in the same opened-document fixture above.
#[test]
fn test_quoted_method_modifier_keeps_definition_target() -> TestResult {
    let uri = "file:///quoted_modifier_definition.pl";
    let doc = "package MyApp::User;\nuse Moo;\nsub save { }\nbefore 'save' => sub { };\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document(uri, doc)?;

    let definition = harness.request(
        "textDocument/definition",
        json!({
            "textDocument": {"uri": uri},
            "position": {"line": 3, "character": 9}
        }),
    )?;
    if definition.pointer("/0/range/start/line").and_then(|line| line.as_u64()) != Some(2) {
        return Err(format!("quoted modifier should navigate to sub save: {definition}").into());
    }
    Ok(())
}

/// Raw-text goto detection must not turn quoted prose into a label reference.
#[test]
fn test_quoted_goto_does_not_navigate_to_real_label() -> TestResult {
    let uri = "file:///quoted_goto_definition.pl";
    let doc = "sub demo {\n    TARGET: return 1;\n    my $literal = 'goto TARGET';\n    goto TARGET;\n}\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document(uri, doc)?;

    let quoted = harness.request(
        "textDocument/definition",
        json!({
            "textDocument": {"uri": uri},
            "position": {"line": 2, "character": 24}
        }),
    )?;
    if !quoted.is_null() && !quoted.as_array().is_some_and(|locations| locations.is_empty()) {
        return Err(format!("quoted goto navigated to a real label: {quoted}").into());
    }

    let real = harness.request(
        "textDocument/definition",
        json!({
            "textDocument": {"uri": uri},
            "position": {"line": 3, "character": 9}
        }),
    )?;
    if real.pointer("/0/range/start/line").and_then(|line| line.as_u64()) != Some(1) {
        return Err(format!("real goto should navigate to TARGET label: {real}").into());
    }
    Ok(())
}

/// Quoted module-looking text must not enter raw use/package-arrow routing.
#[test]
fn test_quoted_module_text_does_not_navigate_to_real_module() -> TestResult {
    let workspace = TempWorkspace::new()?;
    let module = "package Foo;\nsub bar { 1 }\n1;\n";
    let doc = "use lib 'lib';\nuse Foo;\nuse parent 'Foo';\nmy $literal = 'use Foo';\nmy $arrow = 'Foo->bar';\nmy $framework = 'with \"Foo\"';\nwith \"Foo\";\nFoo->bar();\n";
    workspace.write("lib/Foo.pm", module)?;
    workspace.write("main.pl", doc)?;
    let module_uri = workspace.uri("lib/Foo.pm");
    let uri = workspace.uri("main.pl");

    let mut harness = LspHarness::new();
    harness.initialize_with_root(&workspace.root_uri, None)?;
    harness.open_document(&module_uri, module)?;
    harness.open_document(&uri, doc)?;
    harness.barrier();

    for (line, character) in [(3, 19), (4, 13), (5, 23)] {
        let definition = harness.request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character}
            }),
        )?;
        if !definition.is_null()
            && !definition.as_array().is_some_and(|locations| locations.is_empty())
        {
            return Err(format!(
                "quoted module text at ({line}, {character}) navigated: {definition}"
            )
            .into());
        }
    }

    for (line, character) in [(1, 5), (2, 12), (6, 6), (7, 1)] {
        let definition = harness.request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character}
            }),
        )?;
        if !definition
            .pointer("/0/uri")
            .and_then(|actual| actual.as_str())
            .is_some_and(|actual| actual.eq_ignore_ascii_case(&module_uri))
        {
            return Err(format!("real module reference should reach Foo.pm: {definition}").into());
        }
    }
    Ok(())
}

/// A quoted XS argument is a target only when the loader call itself is code.
#[test]
fn test_quoted_xs_argument_keeps_target_but_inert_loader_does_not() -> TestResult {
    let workspace = TempWorkspace::new()?;
    let doc = "package My::Module;\nuse XSLoader;\nXSLoader::load('My::Module');\nmy $literal = 'XSLoader::load My::Module';\n1;\n";
    let xs = "EXTERN_C void boot_My__Module(pTHX_ CV* cv) { }\n";
    workspace.write("lib/My/Module.pm", doc)?;
    workspace.write("Module.xs", xs)?;
    let uri = workspace.uri("lib/My/Module.pm");
    let mut harness = LspHarness::new();
    harness.initialize_with_root(&workspace.root_uri, None)?;
    harness.open_document(&uri, doc)?;
    harness.barrier();

    for character in [15, 30] {
        let definition = harness.request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": 3, "character": character}
            }),
        )?;
        if !definition.is_null()
            && !definition.as_array().is_some_and(|locations| locations.is_empty())
        {
            return Err(format!(
                "inert XS loader at character {character} navigated: {definition}"
            )
            .into());
        }
    }

    let definition = harness.request(
        "textDocument/definition",
        json!({
            "textDocument": {"uri": uri},
            "position": {"line": 2, "character": 16}
        }),
    )?;
    if !definition
        .pointer("/0/uri")
        .and_then(|uri| uri.as_str())
        .is_some_and(|uri| uri.ends_with("/Module.xs"))
    {
        return Err(format!("quoted XS argument should navigate to Module.xs: {definition}").into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// $0 — program name
// ---------------------------------------------------------------------------

/// Hovering on $0 should return the program name documentation, NOT capture
/// group docs (capture groups start at $1).
#[test]
fn test_hover_program_name_p2() -> TestResult {
    // "print $0;\n"
    //  0123456789
    // $0 at offset 6; '0' is at offset 7.
    // Cursor must be on '0' (offset 7) — the alphanumeric part — to get "$0" from tokenizer.
    let doc = "print $0;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///program_name_p2.pl", doc)?;
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///program_name_p2.pl"},
                "position": {"line": 0, "character": 7}
            }),
        )
        .unwrap_or(json!(null));
    let val = hover_value(&result).ok_or("Expected hover content for $0")?;
    let lower = val.to_lowercase();
    // $0 must describe the program name, not capture groups
    assert!(
        lower.contains("program") || lower.contains("script") || lower.contains("name"),
        "$0 hover should describe the program/script name, got: {val}"
    );
    assert!(
        !lower.contains("capture group"),
        "$0 hover must not claim it is a capture group, got: {val}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// $! — OS error / errno
// ---------------------------------------------------------------------------

/// Hovering on $! should explain errno and system error messages.
/// Uses expression context (not `local`) so the semantic analyzer does not
/// treat it as a user-declared scalar and shadow the special variable docs.
#[test]
fn test_hover_errno_p2() -> TestResult {
    // "die $! unless $ok;\n"
    //  0         1
    //  0123456789012345678
    // $! at offset 4; '!' is at offset 5 — either the sigil (4) or '!' (5) works
    // for extract_special_variable since it scans forward from sigil position.
    let doc = "die $! unless $ok;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///errno_p2.pl", doc)?;
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///errno_p2.pl"},
                "position": {"line": 0, "character": 4}
            }),
        )
        .unwrap_or(json!(null));
    let val = hover_value(&result).ok_or("Expected hover content for $!")?;
    let lower = val.to_lowercase();
    assert!(
        lower.contains("error") || lower.contains("errno") || lower.contains("system"),
        "$! hover should describe OS error / errno, got: {val}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// $@ — eval error
// ---------------------------------------------------------------------------

/// Hovering on $@ should explain that it holds the eval exception string.
#[test]
fn test_hover_eval_error_p2() -> TestResult {
    // "warn $@ if $@;\n"
    //  0         1
    //  01234567890123
    // First $@ at offset 5; '@' is at offset 6.
    let doc = "warn $@ if $@;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///eval_error_p2.pl", doc)?;
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///eval_error_p2.pl"},
                "position": {"line": 0, "character": 5}
            }),
        )
        .unwrap_or(json!(null));
    let val = hover_value(&result).ok_or("Expected hover content for $@")?;
    let lower = val.to_lowercase();
    assert!(
        lower.contains("eval") || lower.contains("error") || lower.contains("exception"),
        "$@ hover should describe the eval error variable, got: {val}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// $/ — input record separator
// ---------------------------------------------------------------------------

/// Hovering on $/ should explain line-ending / slurp mode behaviour.
/// Uses expression context to avoid semantic analyzer shadowing.
#[test]
fn test_hover_input_record_separator_p2() -> TestResult {
    // "undef $/;\n"  — $/ at offset 6, '/' at offset 7
    let doc = "undef $/;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///irs_p2.pl", doc)?;
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///irs_p2.pl"},
                "position": {"line": 0, "character": 7}
            }),
        )
        .unwrap_or(json!(null));
    let val = hover_value(&result).ok_or("Expected hover content for $/")?;
    let lower = val.to_lowercase();
    assert!(
        lower.contains("separator") || lower.contains("record") || lower.contains("slurp"),
        "$/ hover should describe input record separator / slurp mode, got: {val}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// $\ — output record separator
// ---------------------------------------------------------------------------

/// Hovering on $\ should explain that it is appended after every print.
/// Uses expression context to avoid semantic analyzer shadowing.
#[test]
fn test_hover_output_record_separator_p2() -> TestResult {
    // "print $\\;\n"  — $\ at offset 6, '\' at offset 7
    // In the actual file: "print $\;\n" (the backslash is a single char)
    let doc = "print $\\;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///ors_p2.pl", doc)?;
    // The file text is: p=0 r=1 i=2 n=3 t=4 ' '=5 $=6 \=7 ;=8 \n=9
    // Hover on '$' (offset 6) — extract_special_variable looks at next char '\' (offset 7)
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///ors_p2.pl"},
                "position": {"line": 0, "character": 6}
            }),
        )
        .unwrap_or(json!(null));
    let val = hover_value(&result).ok_or("Expected hover content for $\\")?;
    let lower = val.to_lowercase();
    assert!(
        lower.contains("separator") || lower.contains("output") || lower.contains("print"),
        "$\\ hover should describe the output record separator, got: {val}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// $, — output field separator
// ---------------------------------------------------------------------------

/// Hovering on $, should explain insertion between print list items.
/// Uses expression context to avoid semantic analyzer shadowing.
#[test]
fn test_hover_output_field_separator_p2() -> TestResult {
    // "print $,;\n"  — $, at offset 6, ',' at offset 7
    let doc = "print $,;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///ofs_p2.pl", doc)?;
    // Hover on '$' at offset 6; extract_special_variable sees ',' at offset 7
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///ofs_p2.pl"},
                "position": {"line": 0, "character": 6}
            }),
        )
        .unwrap_or(json!(null));
    let val = hover_value(&result).ok_or("Expected hover content for $,")?;
    let lower = val.to_lowercase();
    assert!(
        lower.contains("separator") || lower.contains("field") || lower.contains("output"),
        "$, hover should describe the output field separator, got: {val}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// %ENV — environment variables hash
// ---------------------------------------------------------------------------

/// Hovering on %ENV should explain environment variable inheritance.
#[test]
fn test_hover_env_hash_p2() -> TestResult {
    // "print keys %ENV;\n"
    //  0         1
    //  0123456789012345
    // %ENV at offset 11; 'E' at offset 12.
    // Cursor on 'E' (12) to get "%ENV" from tokenizer.
    let doc = "print keys %ENV;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///env_p2.pl", doc)?;
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///env_p2.pl"},
                "position": {"line": 0, "character": 12}
            }),
        )
        .unwrap_or(json!(null));
    let val = hover_value(&result).ok_or("Expected hover content for %ENV")?;
    let lower = val.to_lowercase();
    assert!(
        lower.contains("environment") || lower.contains("env"),
        "%ENV hover should describe environment variables, got: {val}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// %SIG — signal handlers hash
// ---------------------------------------------------------------------------

/// Hovering on %SIG should explain signal handler registration.
#[test]
fn test_hover_sig_hash_p2() -> TestResult {
    // "my %s = %SIG;\n"
    //  0         1
    //  01234567890123
    // %SIG at offset 8; 'S' at offset 9.
    let doc = "my %s = %SIG;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///sig_p2.pl", doc)?;
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///sig_p2.pl"},
                "position": {"line": 0, "character": 9}
            }),
        )
        .unwrap_or(json!(null));
    let val = hover_value(&result).ok_or("Expected hover content for %SIG")?;
    let lower = val.to_lowercase();
    assert!(
        lower.contains("signal") || lower.contains("handler") || lower.contains("sig"),
        "%SIG hover should describe signal handlers, got: {val}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// @INC — module search path
// ---------------------------------------------------------------------------

/// Hovering on @INC should explain module search paths and `use lib`.
#[test]
fn test_hover_inc_array_p2() -> TestResult {
    // "unshift @INC, '/my/lib';\n"
    //  0         1
    //  0123456789012
    // @INC at offset 8; 'I' at offset 9.
    let doc = "unshift @INC, '/my/lib';\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///inc_p2.pl", doc)?;
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///inc_p2.pl"},
                "position": {"line": 0, "character": 9}
            }),
        )
        .unwrap_or(json!(null));
    let val = hover_value(&result).ok_or("Expected hover content for @INC")?;
    let lower = val.to_lowercase();
    assert!(
        lower.contains("module") || lower.contains("search") || lower.contains("inc"),
        "@INC hover should describe module search paths, got: {val}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// @ARGV — command-line arguments
// ---------------------------------------------------------------------------

/// Hovering on @ARGV should explain command-line argument handling.
#[test]
fn test_hover_argv_array_p2() -> TestResult {
    // "print scalar @ARGV;\n"
    //  0         1         2
    //  01234567890123456789
    // @ARGV at offset 14; 'A' at offset 15.
    let doc = "print scalar @ARGV;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///argv_p2.pl", doc)?;
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///argv_p2.pl"},
                "position": {"line": 0, "character": 15}
            }),
        )
        .unwrap_or(json!(null));
    let val = hover_value(&result).ok_or("Expected hover content for @ARGV")?;
    let lower = val.to_lowercase();
    assert!(
        lower.contains("argument") || lower.contains("command") || lower.contains("argv"),
        "@ARGV hover should describe command-line arguments, got: {val}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// %INC — loaded modules registry
// ---------------------------------------------------------------------------

/// Hovering on %INC should explain the loaded-module registry.
#[test]
fn test_hover_inc_hash_p2() -> TestResult {
    // "my %m = %INC;\n"
    //  0         1
    //  01234567890123
    // %INC at offset 8; 'I' at offset 9.
    let doc = "my %m = %INC;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///inc_hash_p2.pl", doc)?;
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///inc_hash_p2.pl"},
                "position": {"line": 0, "character": 9}
            }),
        )
        .unwrap_or(json!(null));
    let val = hover_value(&result).ok_or("Expected hover content for %INC")?;
    let lower = val.to_lowercase();
    assert!(
        lower.contains("loaded") || lower.contains("module") || lower.contains("require"),
        "%INC hover should describe the loaded-module registry, got: {val}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// $^W — global warning flag
// ---------------------------------------------------------------------------

/// Hovering on $^W should explain the global warnings flag.
#[test]
fn test_hover_warning_flag_p2() -> TestResult {
    // "print $^W;\n"
    //  0         1
    //  0123456789
    // $^W at offset 6; '^' at offset 7, 'W' at offset 8.
    // extract_special_variable handles $^X when cursor is on '$' or '^'.
    let doc = "print $^W;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///warn_flag_p2.pl", doc)?;
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///warn_flag_p2.pl"},
                "position": {"line": 0, "character": 7}
            }),
        )
        .unwrap_or(json!(null));
    let val = hover_value(&result).ok_or("Expected hover content for $^W")?;
    let lower = val.to_lowercase();
    assert!(
        lower.contains("warn") || lower.contains("flag") || lower.contains("warning"),
        "$^W hover should describe the global warnings flag, got: {val}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// $^O — operating system name
// ---------------------------------------------------------------------------

/// Hovering on $^O should explain the OS name string.
#[test]
fn test_hover_os_name_p2() -> TestResult {
    // "print $^O;\n"
    //  0         1
    //  0123456789
    // $^O at offset 6; '^' at offset 7, 'O' at offset 8.
    let doc = "print $^O;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///os_name_p2.pl", doc)?;
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///os_name_p2.pl"},
                "position": {"line": 0, "character": 7}
            }),
        )
        .unwrap_or(json!(null));
    let val = hover_value(&result).ok_or("Expected hover content for $^O")?;
    let lower = val.to_lowercase();
    assert!(
        lower.contains("operating") || lower.contains("os") || lower.contains("platform"),
        "$^O hover should describe the operating system name, got: {val}"
    );
    Ok(())
}
