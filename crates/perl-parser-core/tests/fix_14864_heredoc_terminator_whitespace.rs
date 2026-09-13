//! Edge-case proof for #14864: a heredoc terminator with trailing horizontal
//! whitespace must close the SourceRegion heredoc so following source stays
//! `Code`.
//!
//! The load-bearing production assertion remains
//! `source_context_heredoc::terminator_with_trailing_whitespace_closes_region`.
//! These cases pin CRLF, mixed padding, `<<~`, and the opposite-direction
//! near-misses that must not close.

use perl_parser_core::{SourceRegionIndex, SourceRegionKind};

fn require_trailing_code(source: &str, marker: &str) -> Result<(), Box<dyn std::error::Error>> {
    let index = SourceRegionIndex::build(source);
    let body = source.find("body").ok_or("missing heredoc body")?;
    assert_eq!(
        index.kind_at_offset(body),
        SourceRegionKind::Heredoc,
        "body must stay heredoc in {source:?}"
    );
    let after = source.find(marker).ok_or("missing trailing code")?;
    assert_eq!(
        index.kind_at_offset(after),
        SourceRegionKind::Code,
        "code after a whitespace-padded terminator must stay code, regions: {:?}",
        index.regions()
    );
    Ok(())
}

fn require_unterminated_following(
    source: &str,
    marker: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let index = SourceRegionIndex::build(source);
    let after = source.find(marker).ok_or("missing following text")?;
    assert_ne!(
        index.kind_at_offset(after),
        SourceRegionKind::Code,
        "a near-miss terminator must not classify following text as code, regions: {:?}",
        index.regions()
    );
    Ok(())
}

#[test]
fn crlf_padded_terminator_closes_region() -> Result<(), Box<dyn std::error::Error>> {
    require_trailing_code("my $t = <<EOF;\r\nbody\r\nEOF  \r\nmy $after = 1;\r\n", "my $after")
}

#[test]
fn mixed_spaces_and_tabs_after_terminator_close_region() -> Result<(), Box<dyn std::error::Error>> {
    require_trailing_code("my $t = <<EOF;\nbody\nEOF \t \nmy $after = 1;\n", "my $after")
}

#[test]
fn empty_body_padded_terminator_closes_region() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $t = <<EOF;\nEOF  \nmy $after = 1;\n";
    let index = SourceRegionIndex::build(source);
    let after = source.find("my $after").ok_or("missing trailing code")?;
    assert_eq!(
        index.kind_at_offset(after),
        SourceRegionKind::Code,
        "code after an empty padded terminator must stay code, regions: {:?}",
        index.regions()
    );
    Ok(())
}

#[test]
fn indented_heredoc_padded_terminator_closes_region() -> Result<(), Box<dyn std::error::Error>> {
    require_trailing_code("my $t = <<~EOF;\n    body\n    EOF  \nmy $after = 1;\n", "my $after")
}

#[test]
fn second_of_two_heredocs_padded_terminator_closes_region() -> Result<(), Box<dyn std::error::Error>>
{
    let source = "my $a = <<A;\na-body\nA\nmy $t = <<EOF;\nbody\nEOF  \nmy $after = 1;\n";
    require_trailing_code(source, "my $after")
}

#[test]
fn both_of_two_heredocs_padded_terminators_close_regions() -> Result<(), Box<dyn std::error::Error>>
{
    let source = "my $a = <<A;\na-body\nA  \nmy $t = <<EOF;\nbody\nEOF  \nmy $after = 1;\n";
    require_trailing_code(source, "my $after")?;
    let index = SourceRegionIndex::build(source);
    let mid = source.find("my $t").ok_or("missing second opener")?;
    assert_eq!(
        index.kind_at_offset(mid),
        SourceRegionKind::Code,
        "code between two padded closes must stay code, regions: {:?}",
        index.regions()
    );
    Ok(())
}

/// Opposite direction: trailing non-whitespace is not consumed, so the body
/// stays open and following text must not become `Code`.
#[test]
fn trailing_non_whitespace_does_not_close_region() -> Result<(), Box<dyn std::error::Error>> {
    require_unterminated_following("my $t = <<EOF;\nbody\nEOF;\nmy $after = 1;\n", "my $after")
}

/// Opposite direction: ordinary heredocs do not accept a leading-indented
/// terminator. That remains a near-miss, not a close.
#[test]
fn leading_whitespace_on_plain_terminator_does_not_close_region()
-> Result<(), Box<dyn std::error::Error>> {
    require_unterminated_following("my $t = <<EOF;\nbody\n    EOF\nmy $after = 1;\n", "my $after")
}

/// Opposite direction: vertical tab is not horizontal whitespace and must not
/// close. The trim is spaces and tabs only.
#[test]
fn vertical_tab_after_terminator_does_not_close_region() -> Result<(), Box<dyn std::error::Error>> {
    require_unterminated_following(
        "my $t = <<EOF;\nbody\nEOF\u{000b}\nmy $after = 1;\n",
        "my $after",
    )
}

/// `<<~` indent-prefix mismatch is a lexer near-miss, not trailing horizontal
/// whitespace. The suppressor must not turn the following source into `Code`.
#[test]
fn over_indented_tilde_terminator_does_not_close_as_padding()
-> Result<(), Box<dyn std::error::Error>> {
    require_unterminated_following(
        "my $t = <<~END;\n  body\n    END\nmy $after = 1;\n",
        "my $after",
    )
}

/// An unclosed quote after a successful padded close is independent recovery
/// and must not be swallowed by the terminator clip. The line scanner anchors
/// that span at the final character, not the whole quote body.
#[test]
fn unclosed_quote_after_padded_close_stays_recovery() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $t = <<EOF;\nbody\nEOF  \nmy $x = \"open\n";
    let index = SourceRegionIndex::build(source);
    let last =
        source.char_indices().next_back().map(|(offset, _)| offset).ok_or("empty fixture")?;
    assert_eq!(
        index.kind_at_offset(last),
        SourceRegionKind::RecoveryAmbiguous,
        "unclosed quote after a padded close must keep trailing recovery, regions: {:?}",
        index.regions()
    );
    Ok(())
}
