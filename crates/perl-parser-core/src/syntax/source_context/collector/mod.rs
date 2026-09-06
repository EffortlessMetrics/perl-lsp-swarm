//! Region collection for [`super::index::SourceRegionIndex`].

mod literal_scan;

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use perl_lexer::tokenizer::util::find_data_marker_byte_lexed;
use perl_lexer::{PerlLexer, TokenType};

use super::kind::SourceRegionKind;
use super::region::SourceRegion;

/// Collect non-code regions from `source` using lexer spans plus a lifted line scanner.
pub(crate) fn collect_regions(source: &str) -> Vec<SourceRegion> {
    let mut regions = Vec::new();
    regions.extend(literal_scan::scan_line_comments_and_open_literals(source));
    let heredoc_regions = literal_scan::scan_heredoc_regions(source);
    regions.extend(heredoc_regions.iter().copied());
    let mut lexer_regions = collect_lexer_literal_regions(source);
    suppress_padded_terminator_recovery(&mut lexer_regions, &heredoc_regions, source.len());
    regions.extend(lexer_regions);
    if let Some(marker_start) = find_data_marker_byte_lexed(source)
        && let Some(region) =
            SourceRegion::new(marker_start, source.len(), SourceRegionKind::DataSection)
    {
        regions.push(region);
    }
    coalesce_regions(regions, source.len())
}

/// Honor a scanner-closed heredoc when composing lexer recovery.
///
/// `scan_heredoc_regions` already treats trailing spaces/tabs after the label
/// as closing the body. `PerlLexer` does not: Perl 5.38.2 rejects that line
/// (`Can't find string terminator`), and the lexer emits `UnknownRest` through
/// EOF. Mapping that token to [`SourceRegionKind::RecoveryAmbiguous`] then
/// fills the gap after the closed body, so following statements stop being
/// `Code` (#14864).
///
/// Clip or drop only EOF-reaching recovery that starts inside a *closed*
/// heredoc body or exactly at its terminator line. Later independent recovery
/// (an unclosed quote after the padded close) keeps its own span.
fn suppress_padded_terminator_recovery(
    lexer_regions: &mut Vec<SourceRegion>,
    heredoc_regions: &[SourceRegion],
    source_len: usize,
) {
    let mut kept = Vec::with_capacity(lexer_regions.len());
    for mut region in lexer_regions.drain(..) {
        if region.kind == SourceRegionKind::RecoveryAmbiguous && region.end == source_len {
            for heredoc in heredoc_regions {
                if heredoc.kind != SourceRegionKind::Heredoc || heredoc.end >= source_len {
                    continue;
                }
                if region.start >= heredoc.start && region.start < heredoc.end {
                    region.end = heredoc.end;
                    break;
                }
                if region.start == heredoc.end {
                    region.end = region.start;
                    break;
                }
            }
        }
        if region.start < region.end {
            kept.push(region);
        }
    }
    *lexer_regions = kept;
}

fn collect_lexer_literal_regions(source: &str) -> Vec<SourceRegion> {
    let mut regions = Vec::new();
    let mut lexer = PerlLexer::with_body_tokens(source);
    while let Some(token) = lexer.next_token() {
        let kind = match token.token_type {
            // `PerlLexer` emits `InterpolatedString` for *every* double-quoted
            // string, interpolating or not — `StringLiteral` only covers the
            // single-quoted form. Dropping the interpolated variant left every
            // `"…"` span uncovered, so `kind_at_offset` reported `Code` for the
            // most common Perl string literal while `'…'` reported
            // `StringLiteral`.
            TokenType::StringLiteral | TokenType::InterpolatedString(_) => {
                SourceRegionKind::StringLiteral
            }
            // Lexer recovery spans must stay ambiguous rather than decay to
            // `Code`: an unterminated literal lexes as `Error`, and dropping it
            // here contradicted the fail-closed contract documented on
            // `SourceRegionKind::RecoveryAmbiguous`.
            TokenType::Error(_) | TokenType::UnknownRest => SourceRegionKind::RecoveryAmbiguous,
            TokenType::QuoteSingle
            | TokenType::QuoteDouble(_)
            | TokenType::QuoteWords
            | TokenType::QuoteCommand => SourceRegionKind::QuoteLike,
            TokenType::RegexMatch
            | TokenType::Substitution
            | TokenType::Transliteration
            | TokenType::QuoteRegex => SourceRegionKind::RegexLike,
            TokenType::HeredocStart
            | TokenType::HeredocBody(_)
            | TokenType::InterpolatedHeredocBody(_) => SourceRegionKind::Heredoc,
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

/// Resolve overlapping regions into one sorted, non-overlapping cover.
///
/// Overlaps are **split**, not replaced: every byte takes the kind of the
/// highest-precedence region covering it, so an enclosing region keeps both the
/// prefix before and the suffix after a nested higher-precedence region. The
/// previous replace-or-skip rule silently reclassified those parts as `Code`,
/// so heredoc, POD, and `__DATA__` payload became apparent executable source
/// whenever a higher-precedence span nested inside them.
///
/// Adjacent runs of the same kind are merged, so a same-kind split is invisible
/// to callers.
pub(super) fn coalesce_regions(
    mut regions: Vec<SourceRegion>,
    source_len: usize,
) -> Vec<SourceRegion> {
    regions.retain(|region| region.start < region.end && region.end <= source_len);
    if regions.is_empty() {
        return regions;
    }
    regions.sort_by_key(|region| region.start);

    let mut boundaries: Vec<usize> = Vec::with_capacity(regions.len() * 2);
    for region in &regions {
        boundaries.push(region.start);
        boundaries.push(region.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    // Sweep the boundary points. `active` is a max-heap keyed on precedence, so
    // its top is the winning kind for the current slice. Expired entries are
    // dropped lazily from the top, which is sound because equal precedences are
    // ordered by furthest `end`: if the top has expired, so has every peer.
    let mut active: BinaryHeap<ActiveRegion> = BinaryHeap::new();
    let mut next_region = 0usize;
    let mut merged: Vec<SourceRegion> = Vec::new();

    for window in boundaries.windows(2) {
        let (start, end) = (window[0], window[1]);
        while let Some(region) =
            regions.get(next_region).copied().filter(|region| region.start <= start)
        {
            active.push(ActiveRegion {
                precedence: region_precedence(region.kind),
                end: region.end,
                kind: region.kind,
            });
            next_region += 1;
        }
        while active.peek().is_some_and(|top| top.end <= start) {
            active.pop();
        }
        let Some(kind) = active.peek().map(|top| top.kind) else {
            continue;
        };
        if let Some(last) = merged.last_mut()
            && last.kind == kind
            && last.end == start
        {
            last.end = end;
            continue;
        }
        if let Some(region) = SourceRegion::new(start, end, kind) {
            merged.push(region);
        }
    }
    merged
}

/// A region covering the current sweep position, ordered so that the highest
/// precedence — and, for ties, the furthest `end` — sorts to the heap top.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ActiveRegion {
    precedence: u8,
    end: usize,
    kind: SourceRegionKind,
}

impl Ord for ActiveRegion {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.precedence, self.end).cmp(&(other.precedence, other.end))
    }
}

impl PartialOrd for ActiveRegion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
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
    fn heredoc_body_region_present() -> Result<(), Box<dyn std::error::Error>> {
        let source = "my $x = <<EOF;\nbody line\nEOF\n";
        let regions = collect_regions(source);
        let body_offset = source.find("body").ok_or("missing heredoc body")?;
        assert!(
            regions
                .iter()
                .any(|r| r.kind == SourceRegionKind::Heredoc && r.contains_offset(body_offset)),
            "expected heredoc region covering body, got: {regions:?}"
        );
        Ok(())
    }

    #[test]
    fn nested_higher_precedence_region_splits_instead_of_replacing() {
        let regions = coalesce_regions(
            vec![
                SourceRegion { start: 0, end: 20, kind: SourceRegionKind::Pod },
                SourceRegion { start: 5, end: 10, kind: SourceRegionKind::Heredoc },
            ],
            20,
        );
        assert_eq!(
            regions,
            vec![
                SourceRegion { start: 0, end: 5, kind: SourceRegionKind::Pod },
                SourceRegion { start: 5, end: 10, kind: SourceRegionKind::Heredoc },
                SourceRegion { start: 10, end: 20, kind: SourceRegionKind::Pod },
            ],
            "the enclosing region must keep its prefix and suffix"
        );
    }

    #[test]
    fn nested_lower_precedence_region_leaves_enclosing_region_intact() {
        let regions = coalesce_regions(
            vec![
                SourceRegion { start: 0, end: 20, kind: SourceRegionKind::DataSection },
                SourceRegion { start: 5, end: 10, kind: SourceRegionKind::LineComment },
            ],
            20,
        );
        assert_eq!(
            regions,
            vec![SourceRegion { start: 0, end: 20, kind: SourceRegionKind::DataSection }],
            "a lower-precedence nested region must not fragment its enclosing region"
        );
    }

    // ---- collect_lexer_literal_regions token mapping -------------------------

    /// Names the token-mapping seam directly. `PerlLexer` emits
    /// `InterpolatedString` for *every* double-quoted string — interpolating or
    /// not — and `StringLiteral` only for the single-quoted form, so mapping
    /// only the latter left every `"…"` span uncovered.
    #[test]
    fn both_quote_forms_map_to_string_literal() {
        for source in
            ["my $x = 'plain';\n", "my $x = \"plain\";\n", "my $x = \"interp $y here\";\n"]
        {
            let regions = collect_lexer_literal_regions(source);
            assert!(
                regions.iter().any(|region| region.kind == SourceRegionKind::StringLiteral),
                "expected a StringLiteral region for {source:?}, got: {regions:?}"
            );
        }
    }

    /// Lexer recovery spans must stay ambiguous. An unterminated literal lexes
    /// as `Error`; discarding it let partial input read as executable `Code`,
    /// contradicting the contract on `SourceRegionKind::RecoveryAmbiguous`.
    #[test]
    fn lexer_error_span_maps_to_recovery_ambiguous() {
        let source = "my $x = \"open\n";
        let regions = collect_lexer_literal_regions(source);
        assert!(
            regions.iter().any(|region| region.kind == SourceRegionKind::RecoveryAmbiguous),
            "an unterminated literal must produce a recovery region, got: {regions:?}"
        );
        assert!(
            regions.iter().all(|region| region.kind != SourceRegionKind::StringLiteral),
            "an unterminated literal is not a proven string, got: {regions:?}"
        );
    }

    /// The negative half: ordinary code produces no literal region at all, so
    /// the two arms above are not classifying everything they see.
    #[test]
    fn plain_code_produces_no_literal_region() {
        let regions = collect_lexer_literal_regions("my $x = 1 + 2;\n");
        assert!(regions.is_empty(), "plain code must produce no literal region, got: {regions:?}");
    }

    /// Names the composition seam: the scanner closes on a padded terminator,
    /// the lexer still emits EOF-reaching `UnknownRest`, and `collect_regions`
    /// must not leave that suffix classified as recovery.
    #[test]
    fn padded_terminator_does_not_leave_recovery_over_following_code() {
        let source = "my $t = <<EOF;\nbody\nEOF  \nmy $after = 1;\n";
        let regions = collect_regions(source);
        assert!(
            regions.iter().all(|region| region.kind != SourceRegionKind::RecoveryAmbiguous),
            "a whitespace-padded terminator must not leave recovery, got: {regions:?}"
        );
        let after = source.find("my $after").expect("fixture must contain trailing code");
        assert!(
            regions.iter().all(|region| !region.contains_offset(after)),
            "trailing code must be uncovered (Code), got: {regions:?}"
        );
    }

    /// Retention: a truly unclosed quote after a padded close must still be
    /// recovery. The clip is not a blanket drop of every EOF-reaching span.
    #[test]
    fn unclosed_quote_after_padded_terminator_stays_recovery() {
        let source = "my $t = <<EOF;\nbody\nEOF  \nmy $x = \"open\n";
        let regions = collect_regions(source);
        assert!(
            regions.iter().any(|region| region.kind == SourceRegionKind::RecoveryAmbiguous),
            "an unclosed quote after the padded close must stay recovery, got: {regions:?}"
        );
    }
}
