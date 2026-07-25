//! Region collection for [`super::index::SourceRegionIndex`].

mod literal_scan;

use perl_lexer::tokenizer::util::find_data_marker_byte_lexed;
use perl_lexer::{PerlLexer, TokenType};

use super::kind::SourceRegionKind;
use super::region::SourceRegion;

/// Collect non-code regions from `source` using lexer spans plus a lifted line scanner.
pub(crate) fn collect_regions(source: &str) -> Vec<SourceRegion> {
    let mut regions = Vec::new();
    regions.extend(literal_scan::scan_line_comments_and_open_literals(source));
    regions.extend(literal_scan::scan_heredoc_regions(source));
    regions.extend(collect_lexer_literal_regions(source));
    if let Some(marker_start) = find_data_marker_byte_lexed(source) {
        if let Some(region) =
            SourceRegion::new(marker_start, source.len(), SourceRegionKind::DataSection)
        {
            regions.push(region);
        }
    }
    coalesce_regions(regions, source.len())
}

fn collect_lexer_literal_regions(source: &str) -> Vec<SourceRegion> {
    let mut regions = Vec::new();
    let mut lexer = PerlLexer::with_body_tokens(source);
    while let Some(token) = lexer.next_token() {
        let kind = match token.token_type {
            TokenType::StringLiteral => SourceRegionKind::StringLiteral,
            TokenType::QuoteSingle
            | TokenType::QuoteDouble
            | TokenType::QuoteWords
            | TokenType::QuoteCommand => SourceRegionKind::QuoteLike,
            TokenType::RegexMatch
            | TokenType::Substitution
            | TokenType::Transliteration
            | TokenType::QuoteRegex => SourceRegionKind::RegexLike,
            TokenType::HeredocStart | TokenType::HeredocBody(_) => SourceRegionKind::Heredoc,
            TokenType::Pod => SourceRegionKind::Pod,
            TokenType::DataMarker(_) | TokenType::DataBody(_) => SourceRegionKind::DataSection,
            TokenType::Comment(_) => SourceRegionKind::LineComment,
            TokenType::EOF => break,
            _ => continue,
        };
        if let Some(region) = SourceRegion::new(token.start, token.end, kind) {
            regions.push(region);
        }
    }
    regions
}

fn coalesce_regions(mut regions: Vec<SourceRegion>, source_len: usize) -> Vec<SourceRegion> {
    regions.retain(|region| region.start < region.end && region.end <= source_len);
    regions.sort_by_key(|region| {
        (region.start, region_precedence(region.kind), usize::MAX - region.end)
    });
    let mut merged: Vec<SourceRegion> = Vec::new();
    for region in regions {
        if let Some(last) = merged.last_mut() {
            if last.kind == region.kind && region.start <= last.end {
                last.end = last.end.max(region.end);
                continue;
            }
            if region.start < last.end {
                if region_precedence(region.kind) > region_precedence(last.kind) {
                    *last = region;
                }
                continue;
            }
        }
        merged.push(region);
    }
    merged.sort_by_key(|region| region.start);
    merged
}

fn region_precedence(kind: SourceRegionKind) -> u8 {
    match kind {
        SourceRegionKind::RecoveryAmbiguous => 0,
        SourceRegionKind::LineComment => 1,
        SourceRegionKind::Pod => 2,
        SourceRegionKind::DataSection => 3,
        SourceRegionKind::Heredoc => 4,
        SourceRegionKind::StringLiteral => 5,
        SourceRegionKind::QuoteLike => 6,
        SourceRegionKind::RegexLike => 7,
        SourceRegionKind::Code => 8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heredoc_body_region_present() {
        let source = "my $x = <<EOF;\nbody line\nEOF\n";
        let regions = collect_regions(source);
        let body_offset = source.find("body").expect("body");
        assert!(
            regions
                .iter()
                .any(|r| r.kind == SourceRegionKind::Heredoc && r.contains_offset(body_offset)),
            "expected heredoc region covering body, got: {regions:?}"
        );
    }
}
