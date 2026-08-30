use perl_parser_core::{SourceRegionIndex, SourceRegionKind};

#[test]
fn bare_heredoc_body_classified() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = <<EOF;\nbody line\nEOF\n";
    let index = SourceRegionIndex::build(source);
    let body_offset = source.find("body").ok_or("missing heredoc body")?;
    assert_eq!(index.kind_at_offset(body_offset), SourceRegionKind::Heredoc);
    Ok(())
}

#[test]
fn quoted_heredoc_body_classified() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = <<'EOF';\nbody line\nEOF\n";
    let index = SourceRegionIndex::build(source);
    let body_offset = source.find("body").ok_or("missing heredoc body")?;
    assert_eq!(index.kind_at_offset(body_offset), SourceRegionKind::Heredoc);
    Ok(())
}

#[test]
fn indented_heredoc_body_classified() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = <<~EOF;\n    body line\n    EOF\nmy $y = 1;\n";
    let index = SourceRegionIndex::build(source);
    let body_offset = source.find("body").ok_or("missing heredoc body")?;
    assert_eq!(index.kind_at_offset(body_offset), SourceRegionKind::Heredoc);
    let code_offset = source.find("my $y").ok_or("missing trailing code")?;
    assert_eq!(index.kind_at_offset(code_offset), SourceRegionKind::Code);
    Ok(())
}

#[test]
fn left_shift_expression_is_not_a_heredoc() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = 8;\nmy $y = $x << 2;\nmy $z = 3;\n";
    let index = SourceRegionIndex::build(source);
    let tail_offset = source.find("my $z").ok_or("missing trailing code")?;
    assert_eq!(
        index.kind_at_offset(tail_offset),
        SourceRegionKind::Code,
        "`$x << 2` must not open a heredoc, regions: {:?}",
        index.regions()
    );
    Ok(())
}

#[test]
fn numeric_label_does_not_open_heredoc() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $y = 1 << 32;\nmy $z = 3;\n";
    let index = SourceRegionIndex::build(source);
    let tail_offset = source.find("my $z").ok_or("missing trailing code")?;
    assert_eq!(index.kind_at_offset(tail_offset), SourceRegionKind::Code);
    Ok(())
}

/// `PerlLexer` closes a heredoc on a delimiter line carrying trailing spaces or
/// tabs. The collector compared the untrimmed line, so it never closed, scanned
/// to EOF, and reclassified every following statement as `Heredoc` — the same
/// "silently swallow real code" class as the left-shift defect above.
#[test]
fn terminator_with_trailing_whitespace_closes_region() -> Result<(), Box<dyn std::error::Error>> {
    for source in [
        "my $t = <<EOF;\nbody\nEOF  \nmy $after = 1;\n",
        "my $t = <<EOF;\nbody\nEOF\t\nmy $after = 1;\n",
    ] {
        let index = SourceRegionIndex::build(source);
        let body_offset = source.find("body").ok_or("missing body")?;
        assert_eq!(
            index.kind_at_offset(body_offset),
            SourceRegionKind::Heredoc,
            "body must stay heredoc in {source:?}"
        );
        let after_offset = source.find("my $after").ok_or("missing trailing code")?;
        assert_eq!(
            index.kind_at_offset(after_offset),
            SourceRegionKind::Code,
            "code after a whitespace-padded terminator must stay code, regions: {:?}",
            index.regions()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// #5456 / #12934 — the phantom-heredoc negatives through the *production* path.
//
// The unit tests in `collector::literal_scan` exercise `scan_heredoc_regions`
// directly. `collect_regions` runs two further scanners alongside it and then
// coalesces, so a correct marker scan is necessary but not sufficient: these
// assert the classification a real consumer actually sees, via
// `SourceRegionIndex::build`. Every fixture is `syntax OK` under perl 5.38.2
// with no heredoc, so the trailing statement is live code.

#[test]
fn quoted_marker_before_a_construct_leaves_following_code_as_code()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "my $s = \"a <<FAKE\"; my $r = qr/x/;\nmy $tail = 1;\n";
    let index = SourceRegionIndex::build(source);
    let tail = source.find("my $tail").ok_or("missing trailing code")?;
    assert_eq!(
        index.kind_at_offset(tail),
        SourceRegionKind::Code,
        "a quoted marker must not swallow following code, regions: {:?}",
        index.regions()
    );
    Ok(())
}

#[test]
fn marker_in_a_multiline_quote_like_leaves_following_code_as_code()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "my $s = q{\n<<EOF\n};\nmy $tail = 1;\n";
    let index = SourceRegionIndex::build(source);
    let tail = source.find("my $tail").ok_or("missing trailing code")?;
    assert_eq!(
        index.kind_at_offset(tail),
        SourceRegionKind::Code,
        "a marker inside a multi-line q{{}} must not swallow following code, regions: {:?}",
        index.regions()
    );
    Ok(())
}

#[test]
fn marker_in_a_closed_quote_like_leaves_following_code_as_code()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "my $s = q{a <<FAKE}; print 1;\nmy $tail = 1;\n";
    let index = SourceRegionIndex::build(source);
    let tail = source.find("my $tail").ok_or("missing trailing code")?;
    assert_eq!(
        index.kind_at_offset(tail),
        SourceRegionKind::Code,
        "a marker inside a closed literal must not swallow following code, regions: {:?}",
        index.regions()
    );
    Ok(())
}

#[test]
fn marker_on_a_string_continuation_line_leaves_following_code_as_code()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "my $s = \"start\nstill string <<EOF\n\";\nmy $tail = 1;\n";
    let index = SourceRegionIndex::build(source);
    let tail = source.find("my $tail").ok_or("missing trailing code")?;
    assert_eq!(
        index.kind_at_offset(tail),
        SourceRegionKind::Code,
        "a marker on a continuation line must not swallow following code, regions: {:?}",
        index.regions()
    );
    Ok(())
}

// Positive controls for the other two quoted label forms. `<<'EOF'` is already
// covered by `quoted_heredoc_body_classified` above; these pin `<<"EOF"` and
// the backtick command form, so the negatives above cannot be satisfied by a
// scan that simply stopped recognizing quoted labels.

#[test]
fn double_quoted_label_opens_a_heredoc() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = <<\"EOF\";\nbody line\nEOF\nmy $tail = 1;\n";
    let index = SourceRegionIndex::build(source);
    let body = source.find("body").ok_or("missing heredoc body")?;
    assert_eq!(index.kind_at_offset(body), SourceRegionKind::Heredoc);
    let tail = source.find("my $tail").ok_or("missing trailing code")?;
    assert_eq!(
        index.kind_at_offset(tail),
        SourceRegionKind::Code,
        "the body must close at EOF, regions: {:?}",
        index.regions()
    );
    Ok(())
}

#[test]
fn backtick_label_opens_a_heredoc() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = <<`EOF`;\nbody line\nEOF\nmy $tail = 1;\n";
    let index = SourceRegionIndex::build(source);
    let body = source.find("body").ok_or("missing heredoc body")?;
    assert_eq!(index.kind_at_offset(body), SourceRegionKind::Heredoc);
    let tail = source.find("my $tail").ok_or("missing trailing code")?;
    assert_eq!(
        index.kind_at_offset(tail),
        SourceRegionKind::Code,
        "the body must close at EOF, regions: {:?}",
        index.regions()
    );
    Ok(())
}
