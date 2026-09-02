use super::MAX_INCREMENTAL_EDIT_BATCH;
use perl_lexer::{PerlLexer, TokenType};
use perl_parser_core::{
    SourceRangeClassification, SourceRegionIndex, SourceRegionKind,
    ast::{Node, SourceLocation},
    edit::EditSet,
};
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
struct NormalizedEdit {
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
    byte_shift: isize,
}

/// Whether `[start, end)` is provably executable code.
///
/// Non-empty ranges delegate to [`SourceRegionIndex::range_fully_within`].
/// An insertion has an empty range, and #14007 made empty-boundary queries
/// explicit rather than guessing: the boundary's region is code only when
/// both neighbors are code, with a missing neighbor at either end of the
/// source counting as code. Without this, every zero-width insertion fails
/// the region proof and the reuse path dead-falls back to parsing.
fn range_is_code(regions: &SourceRegionIndex, start: usize, end: usize) -> bool {
    if start < end {
        return regions.range_fully_within(start, end, &[SourceRegionKind::Code]);
    }
    match regions.classify_range_checked(start, end) {
        SourceRangeClassification::Proven { kind } => kind == SourceRegionKind::Code,
        SourceRangeClassification::EmptyBoundary { left, right } => {
            let left_is_code = left.is_none_or(|kind| kind == SourceRegionKind::Code);
            let right_is_code = right.is_none_or(|kind| kind == SourceRegionKind::Code);
            left_is_code && right_is_code
        }
        SourceRangeClassification::Ambiguous
        | SourceRangeClassification::InvalidUtf8Boundary
        | SourceRangeClassification::OutOfBounds => false,
    }
}

#[derive(Debug)]
pub(super) struct WhitespaceEditMap {
    edits: Vec<NormalizedEdit>,
}

impl WhitespaceEditMap {
    /// Admit an edit batch only when the declared edits exactly explain the
    /// old/new sources, every replacement is whitespace-only and provably in
    /// code regions, and lexing the two complete sources yields the same
    /// non-whitespace token fingerprint.
    ///
    /// The full-source coherence check is deliberate. Incremental-v2's legacy
    /// `Edit` values carry positions but not replacement text, and stale or
    /// over-wide ranges previously made whitespace tests exercise structural
    /// bytes such as `$` and `=`. Reconstructing the unchanged segments keeps
    /// malformed edit authority out of the reuse path.
    ///
    /// The shared lexer drops comment tokens and does not surface heredoc
    /// bodies without `with_body_tokens`, so its token fingerprint cannot prove
    /// that non-code content is unchanged. The source region index supplies
    /// that proof; anything not provably `Code` falls back conservatively.
    pub(super) fn try_new(old_source: &str, new_source: &str, edits: &EditSet) -> Option<Self> {
        if edits.is_empty() || edits.len() > MAX_INCREMENTAL_EDIT_BATCH {
            return None;
        }

        let old_regions = SourceRegionIndex::build(old_source);
        let new_regions = SourceRegionIndex::build(new_source);
        let mut normalized: Vec<NormalizedEdit> = Vec::with_capacity(edits.len());
        let mut old_cursor = 0usize;
        let mut new_cursor = 0usize;
        let mut cumulative_shift = 0isize;

        for edit in edits.edits() {
            let old_start = original_coordinate(edit.start_byte, cumulative_shift)?;
            let old_end = original_coordinate(edit.old_end_byte, cumulative_shift)?;
            let new_start = edit.start_byte;
            let new_end = edit.new_end_byte;

            if old_start < old_cursor
                || old_end < old_start
                || new_start < new_cursor
                || new_end < new_start
            {
                return None;
            }

            // Consecutive insertions at one original boundary are coherent and
            // can be accumulated. An insertion followed by a non-empty edit at
            // that same original boundary is ambiguous for left-biased spans:
            // the progressive new coordinate already includes the insertion.
            // Keep that mixed overlap on the conservative fallback path.
            if let Some(previous) = normalized.last()
                && previous.old_start == old_start
                && previous.old_start == previous.old_end
                && old_start != old_end
            {
                return None;
            }

            if old_source.get(old_cursor..old_start)? != new_source.get(new_cursor..new_start)? {
                return None;
            }

            let removed = old_source.get(old_start..old_end)?;
            let inserted = new_source.get(new_start..new_end)?;
            if !removed.chars().all(char::is_whitespace)
                || !inserted.chars().all(char::is_whitespace)
            {
                return None;
            }

            if !range_is_code(&old_regions, old_start, old_end)
                || !range_is_code(&new_regions, new_start, new_end)
            {
                return None;
            }

            let byte_shift = edit.byte_shift();
            normalized.push(NormalizedEdit { old_start, old_end, new_start, new_end, byte_shift });
            old_cursor = old_end;
            new_cursor = new_end;
            cumulative_shift = cumulative_shift.checked_add(byte_shift)?;
        }

        if old_source.get(old_cursor..)? != new_source.get(new_cursor..)?
            || shifted_position(old_source.len(), cumulative_shift) != new_source.len()
            || !structural_tokens_match(old_source, new_source)
        {
            return None;
        }

        Some(Self { edits: normalized })
    }

    pub(super) fn clone_tree(&self, root: &Node) -> Option<Node> {
        let mut cloned =
            root.clone_with_mapped_locations(|location| self.map_location(location))?;
        // Parser::parse anchors the Program at the source origin: leading
        // trivia moves its first statement, not the Program anchor.
        // `leading_trivia_insertion_matches_a_fresh_parse` (incremental_v2)
        // pins start, end, and full-tree equality against `Parser::parse`.
        cloned.location = SourceLocation::new(root.location.start(), cloned.location.end());
        Some(cloned)
    }

    fn map_location(&self, location: SourceLocation) -> SourceLocation {
        if location.start() == location.end() {
            let anchor = self.map_position(location.start(), BoundaryBias::Right);
            return SourceLocation::new(anchor, anchor);
        }

        let start = self.map_position(location.start(), BoundaryBias::Right);
        let end = self.map_position(location.end(), BoundaryBias::Left).max(start);
        SourceLocation::new(start, end)
    }

    fn map_position(&self, position: usize, bias: BoundaryBias) -> usize {
        let mut cumulative_shift = 0isize;

        for edit in &self.edits {
            if position < edit.old_start {
                break;
            }

            if edit.old_start == edit.old_end && position == edit.old_start {
                if bias == BoundaryBias::Right {
                    cumulative_shift = cumulative_shift.saturating_add(edit.byte_shift);
                }
                // More than one progressive insertion may map to this same
                // original boundary. Consume all of them before returning the
                // mapped position so the following node receives the full shift.
                continue;
            }

            if position < edit.old_end {
                return match bias {
                    BoundaryBias::Left => edit.new_start,
                    BoundaryBias::Right => edit.new_end,
                };
            }

            cumulative_shift = cumulative_shift.saturating_add(edit.byte_shift);
        }

        shifted_position(position, cumulative_shift)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundaryBias {
    Left,
    Right,
}

fn original_coordinate(coordinate: usize, cumulative_shift: isize) -> Option<usize> {
    let coordinate = isize::try_from(coordinate).ok()?;
    usize::try_from(coordinate.checked_sub(cumulative_shift)?).ok()
}

fn shifted_position(position: usize, shift: isize) -> usize {
    if shift >= 0 {
        position.saturating_add(shift as usize)
    } else {
        position.saturating_sub(shift.unsigned_abs())
    }
}

enum StructuralLexItem {
    Token { kind: TokenType, text: Arc<str>, start: usize, end: usize },
    End,
    Rejected,
}

fn next_structural_token(lexer: &mut PerlLexer<'_>) -> StructuralLexItem {
    loop {
        let Some(token) = lexer.next_token() else {
            return StructuralLexItem::Rejected;
        };

        if token.token_type.is_recovery_token() {
            return StructuralLexItem::Rejected;
        }

        match token.token_type {
            TokenType::Whitespace | TokenType::Newline => continue,
            TokenType::EOF => return StructuralLexItem::End,
            kind => {
                return StructuralLexItem::Token {
                    kind,
                    text: token.text,
                    start: token.start,
                    end: token.end,
                };
            }
        }
    }
}

fn gap_class(source: &str, previous_end: usize, current_start: usize) -> Option<(bool, bool)> {
    let gap = source.get(previous_end..current_start)?;
    Some((gap.is_empty(), gap.contains('\n')))
}

fn adjacency_sensitive_pair(previous_text: &str, current_text: &str) -> bool {
    current_text == "++"
        || current_text == "--"
        || previous_text.contains('$')
        || current_text.contains('$')
}

fn structural_tokens_match(old_source: &str, new_source: &str) -> bool {
    let mut old_lexer = PerlLexer::new(old_source);
    let mut new_lexer = PerlLexer::new(new_source);
    let mut old_previous: Option<(usize, Arc<str>)> = None;
    let mut new_previous_end: Option<usize> = None;

    loop {
        match (next_structural_token(&mut old_lexer), next_structural_token(&mut new_lexer)) {
            (StructuralLexItem::End, StructuralLexItem::End) => return true,
            (
                StructuralLexItem::Token {
                    kind: old_kind,
                    text: old_text,
                    start: old_start,
                    end: old_end,
                },
                StructuralLexItem::Token {
                    kind: new_kind,
                    text: new_text,
                    start: new_start,
                    end: new_end,
                },
            ) if old_kind == new_kind && old_text == new_text => {
                let gaps_match = match (&old_previous, new_previous_end) {
                    (Some((old_end, old_previous_text)), Some(new_end)) => {
                        let Some((old_empty, old_newline)) =
                            gap_class(old_source, *old_end, old_start)
                        else {
                            return false;
                        };
                        let Some((new_empty, new_newline)) =
                            gap_class(new_source, new_end, new_start)
                        else {
                            return false;
                        };
                        // Empty-gap changes matter for postfix incdec block-list and $$ dereference
                        // predicates.
                        old_newline == new_newline
                            && (!adjacency_sensitive_pair(old_previous_text, old_text.as_ref())
                                || old_empty == new_empty)
                    }
                    (None, None) => true,
                    _ => false,
                };
                if !gaps_match {
                    return false;
                }
                old_previous = Some((old_end, old_text));
                new_previous_end = Some(new_end);
            }
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::{ast::NodeKind, edit::Edit, position::Position};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn edit(start: usize, old_end: usize, new_end: usize) -> Edit {
        Edit::new(
            start,
            old_end,
            new_end,
            Position::new(start, 0, start as u32),
            Position::new(old_end, 0, old_end as u32),
            Position::new(new_end, 0, new_end as u32),
        )
    }

    fn edit_set(edits: impl IntoIterator<Item = Edit>) -> EditSet {
        let mut set = EditSet::new();
        for edit in edits {
            set.add(edit);
        }
        set
    }

    fn loc(start: usize, end: usize) -> SourceLocation {
        SourceLocation::new(start, end)
    }

    fn leaf(name: &str, start: usize, end: usize) -> Node {
        Node::new(NodeKind::Identifier { name: name.to_string() }, loc(start, end))
    }

    #[test]
    fn maps_mid_file_insertion_selectively() -> TestResult {
        let edits = edit_set([edit(2, 2, 3)]);
        let map = WhitespaceEditMap::try_new("a b", "a  b", &edits)
            .ok_or("exact whitespace insertion should be admitted")?;
        let root = Node::new(
            NodeKind::Program { statements: vec![leaf("a", 0, 1), leaf("b", 2, 3)] },
            loc(0, 3),
        );

        let mapped = map.clone_tree(&root).ok_or("location mapping unexpectedly failed")?;
        let statements = match &mapped.kind {
            NodeKind::Program { statements } => statements,
            other => return Err(format!("expected Program, got {}", other.kind_name()).into()),
        };
        assert_eq!(mapped.location, loc(0, 4));
        assert_eq!(statements[0].location, loc(0, 1));
        assert_eq!(statements[1].location, loc(3, 4));
        Ok(())
    }

    #[test]
    fn maps_whitespace_deletion_selectively() -> TestResult {
        let edits = edit_set([edit(2, 3, 2)]);
        let map = WhitespaceEditMap::try_new("a  b", "a b", &edits)
            .ok_or("exact whitespace deletion should be admitted")?;
        let root = Node::new(
            NodeKind::Program { statements: vec![leaf("a", 0, 1), leaf("b", 3, 4)] },
            loc(0, 4),
        );

        let mapped = map.clone_tree(&root).ok_or("location mapping unexpectedly failed")?;
        let statements = match &mapped.kind {
            NodeKind::Program { statements } => statements,
            other => return Err(format!("expected Program, got {}", other.kind_name()).into()),
        };
        assert_eq!(mapped.location, loc(0, 3));
        assert_eq!(statements[0].location, loc(0, 1));
        assert_eq!(statements[1].location, loc(2, 3));
        Ok(())
    }

    #[test]
    fn maps_progressive_multi_edit_coordinates() -> TestResult {
        let edits = edit_set([edit(0, 0, 1), edit(3, 3, 4), edit(5, 5, 6)]);
        let map = WhitespaceEditMap::try_new("a b", " a  b ", &edits)
            .ok_or("coherent progressive whitespace edits should be admitted")?;
        let root = Node::new(
            NodeKind::Program { statements: vec![leaf("a", 0, 1), leaf("b", 2, 3)] },
            loc(0, 3),
        );

        let mapped = map.clone_tree(&root).ok_or("location mapping unexpectedly failed")?;
        let statements = match &mapped.kind {
            NodeKind::Program { statements } => statements,
            other => return Err(format!("expected Program, got {}", other.kind_name()).into()),
        };
        // The program anchor stays at the source origin while its end maps
        // with the boundary biases (the fresh parse of " a  b " keeps the
        // program anchored at byte zero as well).
        assert_eq!(mapped.location, loc(0, 5));
        assert_eq!(statements[0].location, loc(1, 2));
        assert_eq!(statements[1].location, loc(4, 5));
        Ok(())
    }

    #[test]
    fn accumulates_adjacent_insertions_at_one_boundary() -> TestResult {
        let edits = edit_set([edit(2, 2, 3), edit(3, 3, 4)]);
        let map = WhitespaceEditMap::try_new("a b", "a   b", &edits)
            .ok_or("adjacent progressive insertions should be admitted")?;
        let root = Node::new(
            NodeKind::Program { statements: vec![leaf("a", 0, 1), leaf("b", 2, 3)] },
            loc(0, 3),
        );

        let mapped = map.clone_tree(&root).ok_or("location mapping unexpectedly failed")?;
        let statements = match &mapped.kind {
            NodeKind::Program { statements } => statements,
            other => return Err(format!("expected Program, got {}", other.kind_name()).into()),
        };
        assert_eq!(mapped.location, loc(0, 5));
        assert_eq!(statements[0].location, loc(0, 1));
        assert_eq!(statements[1].location, loc(4, 5));
        Ok(())
    }

    #[test]
    fn admits_adjacent_progressive_deletions() -> TestResult {
        let edits = edit_set([edit(1, 2, 1), edit(1, 2, 1)]);
        let map = WhitespaceEditMap::try_new("a   b", "a b", &edits)
            .ok_or("adjacent progressive deletions should be admitted")?;
        let root = Node::new(
            NodeKind::Program { statements: vec![leaf("a", 0, 1), leaf("b", 4, 5)] },
            loc(0, 5),
        );

        let mapped = map.clone_tree(&root).ok_or("location mapping unexpectedly failed")?;
        let statements = match &mapped.kind {
            NodeKind::Program { statements } => statements,
            other => return Err(format!("expected Program, got {}", other.kind_name()).into()),
        };
        assert_eq!(mapped.location, loc(0, 3));
        assert_eq!(statements[0].location, loc(0, 1));
        assert_eq!(statements[1].location, loc(2, 3));
        Ok(())
    }

    #[test]
    fn admits_utf8_source_when_edit_offsets_are_boundaries() {
        let old = "print 'é';my $x = 1;";
        let insertion = old.find("my $x").unwrap_or(old.len());
        let new = "print 'é'; my $x = 1;";
        let edits = edit_set([edit(insertion, insertion, insertion + 1)]);
        assert!(WhitespaceEditMap::try_new(old, new, &edits).is_some());
    }

    #[test]
    fn admits_crlf_normalization_without_changing_structural_tokens() -> TestResult {
        let old = "my $x = 1;\nmy $y = 2;";
        let newline = old.find('\n').ok_or("expected newline")?;
        let new = "my $x = 1;\r\nmy $y = 2;";
        let edits = edit_set([edit(newline, newline, newline + 1)]);
        let map = WhitespaceEditMap::try_new(old, new, &edits)
            .ok_or("CRLF insertion should be admitted")?;
        let second_start = newline + 1;
        let root = Node::new(
            NodeKind::Program {
                statements: vec![
                    leaf("first", 0, newline),
                    leaf("second", second_start, old.len()),
                ],
            },
            loc(0, old.len()),
        );

        let mapped = map.clone_tree(&root).ok_or("location mapping unexpectedly failed")?;
        let statements = match &mapped.kind {
            NodeKind::Program { statements } => statements,
            other => return Err(format!("expected Program, got {}", other.kind_name()).into()),
        };
        assert_eq!(statements[0].location, loc(0, newline));
        assert_eq!(statements[1].location, loc(second_start + 1, new.len()));
        Ok(())
    }

    #[test]
    fn rejects_overwide_edit_that_claims_the_operator_as_whitespace() {
        let edits = edit_set([edit(6, 6, 9)]);
        assert!(WhitespaceEditMap::try_new("my $x = 42;", "my $x   = 42;", &edits).is_none());
    }

    #[test]
    fn rejects_structural_replacement_even_when_surrounded_by_whitespace() {
        let edits = edit_set([edit(6, 7, 8)]);
        assert!(WhitespaceEditMap::try_new("my $x = 42;", "my $x += 42;", &edits).is_none());
    }

    #[test]
    fn rejects_newline_that_terminates_a_comment_and_exposes_code() {
        let old = "# hidden print 1;";
        let new = "# hidden\n print 1;";
        let edits = edit_set([edit(8, 8, 9)]);
        assert!(WhitespaceEditMap::try_new(old, new, &edits).is_none());
    }

    #[test]
    fn rejects_whitespace_changes_inside_a_comment_token() {
        let edits = edit_set([edit(3, 4, 5)]);
        assert!(WhitespaceEditMap::try_new("# a b\n1;", "# a  b\n1;", &edits).is_none());
    }

    #[test]
    fn rejects_whitespace_changes_inside_a_string_token() {
        let old = "my $s = \"a b\";";
        let insertion = old.rfind(" b").map_or(old.len(), |index| index + 1);
        let new = "my $s = \"a  b\";";
        let edits = edit_set([edit(insertion, insertion, insertion + 1)]);
        assert!(WhitespaceEditMap::try_new(old, new, &edits).is_none());
    }

    #[test]
    fn rejects_whitespace_changes_inside_a_heredoc_body() {
        let old = "my $s = <<'EOF';\na b\nEOF\n";
        let insertion = old.find("b\nEOF").unwrap_or(old.len());
        let new = "my $s = <<'EOF';\na  b\nEOF\n";
        let edits = edit_set([edit(insertion, insertion, insertion + 1)]);
        assert!(WhitespaceEditMap::try_new(old, new, &edits).is_none());
    }

    #[test]
    fn rejects_whitespace_changes_inside_pod() {
        let old = "=pod\nbody text\n=cut\nmy $x = 1;\n";
        let insertion = old.find("body text").map_or(old.len(), |index| index + 4);
        let new = "=pod\nbody  text\n=cut\nmy $x = 1;\n";
        let edits = edit_set([edit(insertion, insertion, insertion + 1)]);
        assert!(WhitespaceEditMap::try_new(old, new, &edits).is_none());
    }

    #[test]
    fn rejects_insertion_at_a_comment_boundary() {
        let old = "my $x = 1;# note\n";
        let boundary = old.find('#').map_or(old.len(), |index| index);
        let new = "my $x = 1; # note\n";
        let edits = edit_set([edit(boundary, boundary, boundary + 1)]);
        // This conservative boundary causes a correct-but-slower fallback.
        assert!(WhitespaceEditMap::try_new(old, new, &edits).is_none());
    }

    #[test]
    fn rejects_stale_edit_coordinates() {
        let edits = edit_set([edit(4, 4, 5)]);
        assert!(WhitespaceEditMap::try_new("my $x = 1;", "my  $x = 1;", &edits).is_none());
    }

    #[test]
    fn rejects_temporally_reordered_progressive_edits() {
        let edits = edit_set([edit(5, 5, 6), edit(0, 0, 1)]);
        assert!(WhitespaceEditMap::try_new("a b c", " a b c ", &edits).is_none());
    }

    #[test]
    fn rejects_overlapping_declared_ranges() {
        let edits = edit_set([edit(1, 3, 1), edit(2, 3, 2)]);
        assert!(WhitespaceEditMap::try_new("a   b", "a b", &edits).is_none());
    }

    #[test]
    fn rejects_insert_then_delete_at_same_original_boundary() {
        let edits = edit_set([edit(1, 1, 2), edit(2, 3, 2)]);
        assert!(WhitespaceEditMap::try_new("a  b", "a  b", &edits).is_none());
    }

    #[test]
    fn rejects_offsets_inside_utf8_code_points() {
        let old = "print 'é';";
        let accent = old.find('é').unwrap_or(old.len());
        let new = "print ' é';";
        let edits = edit_set([edit(accent + 1, accent + 1, accent + 2)]);
        assert!(WhitespaceEditMap::try_new(old, new, &edits).is_none());
    }
}
