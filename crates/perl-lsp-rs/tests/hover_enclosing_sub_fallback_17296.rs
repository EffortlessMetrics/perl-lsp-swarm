//! `textDocument/hover` must not answer with the enclosing subroutine's card
//! for tokens it does not resolve (#17296).
//!
//! `find_definition`'s containment fallback returns the smallest declaration
//! *containing* the offset, so every identifier inside a sub body used to
//! resolve to the enclosing sub: hovering `keys`, a call-site package name, or
//! the `my` keyword produced a confident `**Subroutine**` card whose hover
//! range covered the hovered token. The card must name the hovered token —
//! builtins via the builtin-documentation fallback, package names via the
//! `::`-qualified token path, keywords via the keyword card — or fail closed.
//!
//! End-to-end through the real LSP request path (pattern of
//! `autoload_hover_confidence_14256.rs`).

// Integration tests print diagnostic output for CI troubleshooting; this is
// not the LSP server's stdio transport, so print_stdout doesn't apply the
// way it does to production code.
#![allow(clippy::print_stdout)]

mod common;

#[cfg(test)]
mod hover_enclosing_sub_fallback {
    use crate::common::test_utils::TestServerBuilder;
    use serde_json::Value;

    fn hover_content(resp: &Value) -> Option<String> {
        let result = resp.get("result")?;
        if result.is_null() {
            return None;
        }
        Some(result.get("contents")?.get("value")?.as_str()?.to_string())
    }

    /// (line, character) of `needle`'s first occurrence in `code`, 0-based.
    fn pos_of(code: &str, needle: &str) -> Result<(u32, u32), Box<dyn std::error::Error>> {
        code.lines()
            .enumerate()
            .find_map(|(line, l)| l.find(needle).map(|col| (line as u32, col as u32)))
            .ok_or_else(|| format!("could not find `{needle}` in test code").into())
    }

    const FIXTURE: &str = r#"package WidgetGarden;

use strict;
use warnings;

sub new {
    my ($class, %opts) = @_;
    return bless { name => $opts{name} }, $class;
}

sub run {
    my $garden = WidgetGarden->new(name => 'demo');
    my %counts = (a => 1, b => 2);
    my $kcount = keys %counts;
    push my @names, 'd';
    $garden->new(name => 'again');
    return $kcount;
}

1;
"#;

    fn open_fixture_and_hover(
        uri: &str,
        needle: &str,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let server = TestServerBuilder::new().build();
        server.open_document(uri, FIXTURE);
        let (line, character) = pos_of(FIXTURE, needle)?;
        let response = server.get_hover(uri, line, character);
        println!("HOVER `{needle}` RESPONSE: {response:#}");
        Ok(hover_content(&response))
    }

    /// Hovering the builtin `keys` inside `sub run` must answer with the
    /// builtin card, never the enclosing `sub run` card.
    #[test]
    fn builtin_inside_sub_is_not_the_enclosing_sub() -> Result<(), Box<dyn std::error::Error>> {
        let uri = "file:///hover_builtin_17296.pm";
        let content = open_fixture_and_hover(uri, "keys %counts")?
            .ok_or("expected hover content for `keys`")?;

        assert!(
            content.contains("Built-in Function"),
            "hover on `keys` must render the builtin card, got: {content}"
        );
        assert!(
            !content.contains("**Subroutine**"),
            "hover on `keys` must not claim a subroutine, got: {content}"
        );
        Ok(())
    }

    /// Hovering the class name at a `->new` call site inside `sub run` must
    /// not answer with the enclosing `sub run` card. The token fallback may
    /// answer with a neutral card or fail closed; neither may name `sub run`.
    #[test]
    fn call_site_package_name_is_not_the_enclosing_sub() -> Result<(), Box<dyn std::error::Error>> {
        let uri = "file:///hover_pkgname_17296.pm";
        let content = open_fixture_and_hover(uri, "WidgetGarden->new(name => 'demo')")?
            .ok_or("expected hover content for the class name")?;

        assert!(
            !content.contains("**Subroutine**"),
            "hover on the class name must not claim a subroutine, got: {content}"
        );
        assert!(
            !content.contains("`sub run`"),
            "hover on the class name must not name the enclosing sub, got: {content}"
        );
        Ok(())
    }

    /// Hovering the `my` keyword inside `sub run` must not answer with the
    /// enclosing sub's card.
    #[test]
    fn keyword_inside_sub_is_not_the_enclosing_sub() -> Result<(), Box<dyn std::error::Error>> {
        let uri = "file:///hover_keyword_17296.pm";
        let content =
            open_fixture_and_hover(uri, "my $kcount")?.ok_or("expected hover content for `my`")?;

        assert!(
            !content.contains("**Subroutine**") && !content.contains("`sub run`"),
            "hover on `my` must not claim the enclosing sub, got: {content}"
        );
        Ok(())
    }

    /// Control: hovering the sub's own name still returns that sub's card.
    /// (The needle targets the `run` name token, not the `sub` keyword — the
    /// keyword honestly answers with its own keyword card.)
    #[test]
    fn hover_on_the_sub_name_keeps_the_sub_card() -> Result<(), Box<dyn std::error::Error>> {
        let uri = "file:///hover_subname_17296.pm";
        let content = open_fixture_and_hover(uri, "run {")?
            .ok_or("expected hover content for the `run` name token")?;

        assert!(
            content.contains("`sub run`"),
            "hover on the sub name must keep the sub card, got: {content}"
        );
        Ok(())
    }

    /// Control: a real lexical in the body still resolves to its variable card.
    #[test]
    fn variable_in_sub_body_still_resolves() -> Result<(), Box<dyn std::error::Error>> {
        let uri = "file:///hover_variable_17296.pm";
        let content = open_fixture_and_hover(uri, "$kcount = keys")?
            .ok_or("expected hover content for `$kcount`")?;

        assert!(
            content.contains("Scalar Variable"),
            "hover on `$kcount` must resolve to its variable card, got: {content}"
        );
        Ok(())
    }

    /// Control: the method name after `->` on a `$self`-style receiver still
    /// reaches the method card rather than failing.
    #[test]
    fn method_name_after_arrow_still_resolves() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"package Plain;

sub new { my $c = shift; return bless {}, $c; }

sub greet { return "hi"; }

sub caller_method {
    my $self = shift;
    return $self->greet();
}
"#;
        let uri = "file:///hover_method_17296.pm";
        let server = TestServerBuilder::new().build();
        server.open_document(uri, code);
        let (line, character) = pos_of(code, "->greet()")?;
        // Cursor on the method name itself, just past `->`.
        let character = character + 2;
        let response = server.get_hover(uri, line, character);
        println!("HOVER `->greet` RESPONSE: {response:#}");
        let content = hover_content(&response).ok_or("expected hover content for `->greet`")?;

        assert!(
            content.contains("greet"),
            "hover on the method name must still describe `greet`, got: {content}"
        );
        assert!(
            !content.contains("caller_method"),
            "hover on the method name must not claim the enclosing sub, got: {content}"
        );
        Ok(())
    }
}
