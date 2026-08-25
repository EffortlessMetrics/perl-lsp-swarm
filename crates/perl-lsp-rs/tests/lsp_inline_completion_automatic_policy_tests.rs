//! Automatic inline completion trigger policy.
//!
//! Automatic ghost text appears without the user asking for it, so it is
//! decided by the evidence behind a candidate rather than by the shape of its
//! text. These tests pin the user-visible consequences:
//!
//! - a continuation backed by a subroutine in the current package appears even
//!   though it contains ordinary Perl punctuation;
//! - scaffolds and placeholders stay behind an explicit invocation;
//! - an automatic request returns at most one item, and nothing at all when two
//!   candidates rest on the same class of evidence;
//! - explicit invocation keeps the full ranked list.

mod support;

use serde_json::{Value, json};
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const AUTOMATIC: u64 = 2;
const INVOKED: u64 = 1;

/// A package whose `run` body is about to call a method on `$self`, with one
/// subroutine — `save` — actually defined in the same package.
const SELF_RECEIVER_SOURCE: &str = "package My::App;\n\
     \n\
     sub save {\n\
     \x20   my $self = shift;\n\
     }\n\
     \n\
     sub run {\n\
     \x20   my $self = shift;\n\
     \x20   $self->\n\
     }\n";

fn open_harness(uri: &str, text: &str) -> Result<LspHarness, Box<dyn std::error::Error>> {
    let mut harness = LspHarness::new();
    harness.initialize(Some(json!({
        "textDocument": { "inlineCompletion": { "dynamicRegistration": true } }
    })))?;
    harness.open(uri, text)?;
    Ok(harness)
}

fn inline_completion_texts(
    harness: &mut LspHarness,
    uri: &str,
    line: u32,
    character: u32,
    trigger_kind: u64,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let result = harness.request(
        "textDocument/inlineCompletion",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "triggerKind": trigger_kind }
        }),
    )?;

    Ok(result
        .get("items")
        .and_then(Value::as_array)
        .ok_or("inline completion result must contain an items array")?
        .iter()
        .filter_map(|item| item.get("insertText").and_then(Value::as_str))
        .map(str::to_string)
        .collect())
}

/// A `$self->` continuation backed by a subroutine in the current package is
/// exactly the case automatic ghost text exists for. Its parentheses and sigil
/// are ordinary Perl, not a signal that the suggestion is weak.
#[test]
fn automatic_shows_source_backed_receiver_continuation() -> TestResult {
    let uri = "file:///automatic_receiver.pl";
    let mut harness = open_harness(uri, SELF_RECEIVER_SOURCE)?;

    let texts = inline_completion_texts(&mut harness, uri, 8, 11, AUTOMATIC)?;

    assert_eq!(texts, vec!["save()".to_string()]);
    Ok(())
}

/// Automatic display is zero-or-one by contract, so a request that would rank
/// several candidates still yields at most one item.
#[test]
fn automatic_returns_at_most_one_item() -> TestResult {
    let uri = "file:///automatic_single_item.pl";
    let mut harness = open_harness(uri, SELF_RECEIVER_SOURCE)?;

    let automatic = inline_completion_texts(&mut harness, uri, 8, 11, AUTOMATIC)?;
    assert!(automatic.len() <= 1, "automatic display must return zero or one item: {automatic:?}");
    Ok(())
}

/// A subroutine body scaffold is a template the user still has to fill in, so
/// it must not appear unbidden — but invoking completion explicitly still
/// offers it.
#[test]
fn automatic_withholds_subroutine_scaffold_that_invocation_still_offers() -> TestResult {
    let uri = "file:///automatic_scaffold.pl";
    let source = "package My::App;\n\nsub compute";
    let mut harness = open_harness(uri, source)?;

    let automatic = inline_completion_texts(&mut harness, uri, 2, 11, AUTOMATIC)?;
    assert!(
        automatic.is_empty(),
        "a subroutine body scaffold must not appear as automatic ghost text: {automatic:?}"
    );

    let invoked = inline_completion_texts(&mut harness, uri, 2, 11, INVOKED)?;
    assert!(
        invoked.iter().any(|text| text.contains("...")),
        "explicit invocation must still offer the scaffold: {invoked:?}"
    );
    Ok(())
}

/// Two subroutines in the current package are equally good `$self->`
/// continuations. Inserting one of them unbidden would be a guess, so automatic
/// display stays silent while explicit invocation offers both.
#[test]
fn automatic_is_silent_when_candidates_are_equally_supported() -> TestResult {
    let uri = "file:///automatic_ambiguous_methods.pl";
    let source = "package My::App;\n\
         \n\
         sub save {\n\
         \x20   my $self = shift;\n\
         }\n\
         \n\
         sub load {\n\
         \x20   my $self = shift;\n\
         }\n\
         \n\
         sub run {\n\
         \x20   my $self = shift;\n\
         \x20   $self->\n\
         }\n";
    let mut harness = open_harness(uri, source)?;

    let automatic = inline_completion_texts(&mut harness, uri, 12, 11, AUTOMATIC)?;
    assert!(automatic.is_empty(), "ambiguous automatic requests must be empty: {automatic:?}");

    let invoked = inline_completion_texts(&mut harness, uri, 12, 11, INVOKED)?;
    assert!(
        invoked.len() > 1,
        "explicit invocation must keep every equally-supported method: {invoked:?}"
    );
    Ok(())
}

/// The provider ranks `use strict;` ahead of the other pragmas on purpose, so
/// a bare `use ` is not ambiguous: that preference survives into automatic
/// display, while invocation still offers the full list.
#[test]
fn automatic_keeps_the_preferred_pragma_for_a_bare_use_statement() -> TestResult {
    let uri = "file:///automatic_bare_use.pl";
    let mut harness = open_harness(uri, "use ")?;

    let automatic = inline_completion_texts(&mut harness, uri, 0, 4, AUTOMATIC)?;
    assert_eq!(automatic, vec!["strict;".to_string()]);

    let invoked = inline_completion_texts(&mut harness, uri, 0, 4, INVOKED)?;
    assert!(invoked.len() > 1, "explicit invocation must keep every pragma: {invoked:?}");
    Ok(())
}

/// Once the typed prefix singles out one pragma the request is no longer
/// ambiguous, so automatic ghost text appears again.
#[test]
fn automatic_shows_disambiguated_use_statement() -> TestResult {
    let uri = "file:///automatic_disambiguated_use.pl";
    let mut harness = open_harness(uri, "use str")?;

    let texts = inline_completion_texts(&mut harness, uri, 0, 7, AUTOMATIC)?;

    assert_eq!(texts, vec!["strict;".to_string()]);
    Ok(())
}

/// A request with no declared trigger kind cannot be attributed to a keystroke,
/// so it is treated as an explicit invocation and keeps the full ranked list.
#[test]
fn request_without_context_keeps_the_full_list() -> TestResult {
    let uri = "file:///automatic_no_context.pl";
    let mut harness = open_harness(uri, "use ")?;

    let result = harness.request(
        "textDocument/inlineCompletion",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 4 }
        }),
    )?;
    let items = result
        .get("items")
        .and_then(Value::as_array)
        .ok_or("inline completion result must contain an items array")?;

    assert!(items.len() > 1, "a context-free request must not be narrowed to one item");
    Ok(())
}
