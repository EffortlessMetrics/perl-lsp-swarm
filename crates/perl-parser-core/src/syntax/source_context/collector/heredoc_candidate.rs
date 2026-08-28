//! Heredoc candidate filtering backed by lexer-owned source regions.

use super::literal_scan;
use super::super::kind::SourceRegionKind;
use super::super::region::SourceRegion;

/// Scan heredoc bodies after hiding opener-shaped text already proven non-code.
///
/// The legacy line scanner selects the first textual `<<` on a line. The lexer
/// already owns string, quote-like, regex, comment, POD, data, and recovery
/// boundaries, so use those spans to keep non-code markers out of the candidate
/// set without adding a second quote parser.
pub(super) fn scan_heredoc_regions_in_code(
    source: &str,
    lexer_regions: &[SourceRegion],
) -> Vec<SourceRegion> {
    let Some(masked_source) = mask_non_code_heredoc_markers(source, lexer_regions) else {
        return literal_scan::scan_heredoc_regions(source);
    };
    literal_scan::scan_heredoc_regions(&masked_source)
}

/// Replace only lexer-proven non-code `<<` bytes, preserving source geometry.
fn mask_non_code_heredoc_markers(source: &str, lexer_regions: &[SourceRegion]) -> Option<String> {
    let mut masked: Option<String> = None;
    for (marker, _) in source.match_indices("<<") {
        let is_non_code = lexer_regions.iter().any(|region| {
            region.contains_offset(marker) && kind_masks_heredoc_candidate(region.kind)
        });
        if !is_non_code {
            continue;
        }
        masked
            .get_or_insert_with(|| source.to_owned())
            .replace_range(marker..marker + 2, "  ");
    }
    masked
}

fn kind_masks_heredoc_candidate(kind: SourceRegionKind) -> bool {
    matches!(
        kind,
        SourceRegionKind::RecoveryAmbiguous
            | SourceRegionKind::LineComment
            | SourceRegionKind::Pod
            | SourceRegionKind::DataSection
            | SourceRegionKind::StringLiteral
            | SourceRegionKind::QuoteLike
            | SourceRegionKind::RegexLike
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn missing(message: impl Into<String>) -> Box<dyn std::error::Error> {
        std::io::Error::other(message.into()).into()
    }

    fn offset(source: &str, needle: &str) -> Result<usize, Box<dyn std::error::Error>> {
        source
            .find(needle)
            .ok_or_else(|| missing(format!("missing {needle:?} in fixture")))
    }

    #[test]
    fn quoted_candidate_is_hidden_while_later_real_opener_stays_visible() -> TestResult {
        let source = "my $s = \"<<NOPE\"; print <<REAL;\nbody\nREAL\ntail();\n";
        let quote_start = offset(source, "\"<<NOPE\"")?;
        let lexer_regions = [SourceRegion {
            start: quote_start,
            end: quote_start + "\"<<NOPE\"".len(),
            kind: SourceRegionKind::StringLiteral,
        }];

        let regions = scan_heredoc_regions_in_code(source, &lexer_regions);
        let body_start = offset(source, "body\n")?;
        let terminator_start = offset(source, "REAL\ntail")?;
        assert_eq!(
            regions,
            vec![SourceRegion {
                start: body_start,
                end: terminator_start,
                kind: SourceRegionKind::Heredoc,
            }],
            "the quoted marker must be skipped and the later code marker selected"
        );
        Ok(())
    }

    #[test]
    fn every_lexer_proven_non_code_kind_hides_a_marker() {
        let source = "<<NOPE;\nbody\n";
        for kind in [
            SourceRegionKind::RecoveryAmbiguous,
            SourceRegionKind::LineComment,
            SourceRegionKind::Pod,
            SourceRegionKind::DataSection,
            SourceRegionKind::StringLiteral,
            SourceRegionKind::QuoteLike,
            SourceRegionKind::RegexLike,
        ] {
            let lexer_regions = [SourceRegion {
                start: 0,
                end: "<<NOPE".len(),
                kind,
            }];
            assert!(
                scan_heredoc_regions_in_code(source, &lexer_regions).is_empty(),
                "{kind:?} must keep its marker out of the heredoc candidate set"
            );
        }
    }

    #[test]
    fn lexer_proven_heredoc_marker_remains_scannable() -> TestResult {
        let source = "print <<REAL;\nbody\nREAL\ntail();\n";
        let marker = offset(source, "<<REAL")?;
        let lexer_regions = [SourceRegion {
            start: marker,
            end: marker + "<<REAL".len(),
            kind: SourceRegionKind::Heredoc,
        }];

        let regions = scan_heredoc_regions_in_code(source, &lexer_regions);
        let body_start = offset(source, "body\n")?;
        let terminator_start = offset(source, "REAL\ntail")?;
        assert_eq!(
            regions,
            vec![SourceRegion {
                start: body_start,
                end: terminator_start,
                kind: SourceRegionKind::Heredoc,
            }],
            "an authoritative heredoc token must not hide its real opener"
        );
        Ok(())
    }
}
