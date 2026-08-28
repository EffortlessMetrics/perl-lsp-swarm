use perl_parser_core::{SourceRegionIndex, SourceRegionKind};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn ordinary_quoted_markers_do_not_open_phantom_heredocs() -> TestResult {
    for source in [
        "my $doc = \"use <<EOF here\";\nmy $tail = 1;\n",
        "my $doc = 'use <<EOF here';\nmy $tail = 1;\n",
    ] {
        let index = SourceRegionIndex::build(source);
        let marker_offset = source.find("<<EOF").ok_or("missing quoted marker")?;
        assert_eq!(
            index.kind_at_offset(marker_offset),
            SourceRegionKind::StringLiteral,
            "the marker itself must remain string content in {source:?}: {:?}",
            index.regions()
        );

        let tail_offset = source.find("my $tail").ok_or("missing trailing code")?;
        assert_eq!(
            index.kind_at_offset(tail_offset),
            SourceRegionKind::Code,
            "a quoted marker must not swallow following code in {source:?}: {:?}",
            index.regions()
        );
        assert!(
            index.regions().iter().all(|region| region.kind != SourceRegionKind::Heredoc),
            "a source containing only a quoted marker must have no heredoc region: {:?}",
            index.regions()
        );
    }
    Ok(())
}

#[test]
fn quoted_false_candidate_does_not_hide_later_real_opener() -> TestResult {
    for source in [
        "my $s = \"<<NOPE\"; print <<REAL;\nbody\nREAL\nmy $tail = 1;\n",
        "my $s = '<<NOPE'; print <<REAL;\nbody\nREAL\nmy $tail = 1;\n",
    ] {
        let index = SourceRegionIndex::build(source);

        let false_marker = source.find("<<NOPE").ok_or("missing false marker")?;
        assert_eq!(
            index.kind_at_offset(false_marker),
            SourceRegionKind::StringLiteral,
            "the first marker must stay inside the string in {source:?}: {:?}",
            index.regions()
        );

        let real_marker = source.find("<<REAL").ok_or("missing real opener")?;
        assert_eq!(
            index.kind_at_offset(real_marker),
            SourceRegionKind::Heredoc,
            "the later code marker must remain a real opener in {source:?}: {:?}",
            index.regions()
        );

        let body_offset = source.find("body").ok_or("missing heredoc body")?;
        assert_eq!(
            index.kind_at_offset(body_offset),
            SourceRegionKind::Heredoc,
            "the real opener must classify its body in {source:?}: {:?}",
            index.regions()
        );

        let tail_offset = source.find("my $tail").ok_or("missing trailing code")?;
        assert_eq!(
            index.kind_at_offset(tail_offset),
            SourceRegionKind::Code,
            "the terminator must resume code in {source:?}: {:?}",
            index.regions()
        );
    }
    Ok(())
}
