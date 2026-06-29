//! Literal-eval sub extractor for dynamic boundary evidence.
//!
//! Recognizes `eval "sub NAME { ... }"` patterns in an AST and emits an
//! [`OccurrenceFact`] with `kind = OccurrenceKind::DynamicBoundary` keyed to
//! the sub name `NAME`.
//!
//! # Scope
//!
//! Only literal string evals whose string value textually contains `sub NAME`
//! are recognized. Non-literal evals (e.g. `eval $code`) are out of scope —
//! the module name is not statically known and no evidence is emitted.
//!
//! Package declarations inside the eval string are tracked left-to-right so
//! that `eval "package Foo; sub bar { }"` emits the canonical name `Foo::bar`.
//! Multiple package switches in a single eval string are handled correctly:
//! `eval "package A; sub x { } package B; sub y { }"` emits `A::x` and `B::y`.
//!
//! # Placement note — circular dependency debt
//!
//! This extractor lives in `perl-workspace` rather than `perl-semantic-analyzer`
//! because of a circular dependency: `perl-semantic-analyzer/Cargo.toml` declares
//! `perl-workspace` as a dependency (for workspace indexing), so moving any
//! producer into `perl-semantic-analyzer` would create a cycle.
//!
//! This is **temporary architectural debt**. The correct long-term placement is
//! `perl-semantic-analyzer`, which owns the semantic production layer.
//! The blocker is the current `perl-semantic-analyzer → perl-workspace` dep arc.
//!
//! **Follow-up**: invert or remove the `perl-semantic-analyzer → perl-workspace`
//! dependency (possibly by introducing a `perl-workspace-types` leaf crate for
//! the fact types), then move this extractor to `perl-semantic-analyzer`.
//! Track this as a follow-up issue after the dynamic-boundary suppression PRs merge.
//!
//! # Requirements
//!
//! - **Req 7.5a**: Emit `DynamicBoundary` evidence for `eval "sub NAME { ... }"`
//!   so that `dynamic_callable_may_be_visible_at` can suppress the
//!   `UnquotedBareword` diagnostic for `NAME` at later call sites in the
//!   same file.
//! - **Req 7.5b**: When a `package NAME;` declaration precedes a `sub` inside
//!   the eval string, qualify the emitted canonical name as `Package::sub_name`.

use crate::ast::{Node, NodeKind};
use perl_semantic_facts::{
    AnchorFact, AnchorId, Confidence, EntityFact, EntityId, EntityKind, FileId, OccurrenceFact,
    OccurrenceId, OccurrenceKind, Provenance,
};

/// Walk an AST and return `(EntityFact, AnchorFact, OccurrenceFact)` triples
/// for each `eval "sub NAME { ... }"` pattern found.
///
/// The returned facts should be merged into the file's [`FileFactShard`] by
/// the caller so that `dynamic_callable_may_be_visible_at` can find them.
///
/// # Algorithm
///
/// 1. Recursively walk every node.
/// 2. For each `NodeKind::Eval { block }` where `block` is a
///    `NodeKind::String { value, .. }` (a literal string eval), extract
///    all sub names that appear as `sub NAME` in `value`.
/// 3. For each name found, emit a triple with `Confidence::Low` and
///    `Provenance::DynamicBoundary`.
///
/// # ID generation
///
/// IDs are derived from a stable hash of `(file_id, node_start_byte, name)`
/// to avoid collisions across multiple eval strings in the same file.
pub fn extract_eval_sub_boundaries(
    ast: &Node,
    file_id: FileId,
) -> Vec<(EntityFact, AnchorFact, OccurrenceFact)> {
    let mut out = Vec::new();
    walk(ast, file_id, &mut out);
    out
}

fn walk(node: &Node, file_id: FileId, out: &mut Vec<(EntityFact, AnchorFact, OccurrenceFact)>) {
    if let NodeKind::Eval { block } = &node.kind {
        // Only literal string evals produce evidence.
        if let NodeKind::String { value, .. } = &block.kind {
            extract_from_eval_string(value, node.location.start, node.location.end, file_id, out);
        }
        // Recurse into the block for nested evals.
        walk(block, file_id, out);
        return;
    }

    for child in node.children() {
        walk(child, file_id, out);
    }
}

/// Parse `eval_string` for `sub NAME` and `package NAME;` patterns and emit triples.
///
/// Handles plausible Perl sub declarations of the form:
/// - `sub NAME {`   — named sub with body
/// - `sub NAME ;`   — forward declaration
/// - `sub NAME (`   — named sub with prototype/signature
///
/// Tracks `package NAME;` declarations left-to-right so that a sub declared
/// after a package statement is emitted as `Package::sub_name`.  Multiple
/// package switches in one eval string are handled:
/// `"package A; sub x { } package B; sub y { }"` emits `A::x` and `B::y`.
///
/// Does NOT match:
/// - `sub { ... }` — anonymous sub (no name)
/// - `sub $name { ... }` — interpolated name (sigil-prefixed)
/// - `sub NAME` followed by arbitrary text (conservative: reject if no
///   plausible Perl delimiter follows)
///
/// This conservative approach avoids false positives from strings that
/// contain the word `sub` in prose (e.g. `"no sub here really"`).
fn extract_from_eval_string(
    eval_string: &str,
    node_start_byte: usize,
    node_end_byte: usize,
    file_id: FileId,
    out: &mut Vec<(EntityFact, AnchorFact, OccurrenceFact)>,
) {
    // Strip surrounding quotes if present (the parser may or may not include them).
    let content = eval_string
        .trim_start_matches('"')
        .trim_end_matches('"')
        .trim_start_matches('\'')
        .trim_end_matches('\'');

    // Track the current active package as we scan left-to-right.
    let mut current_package: Option<String> = None;

    let mut search = content;
    while !search.is_empty() {
        let sub_pos = find_sub_keyword(search);
        let pkg_pos = find_package_keyword(search);

        // Determine which keyword comes first; process it.
        let process_package = match (sub_pos, pkg_pos) {
            (None, None) => break,
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (Some(sp), Some(pp)) => pp < sp,
        };

        if process_package {
            let pp = pkg_pos.unwrap_or(0);
            let after_pkg = &search[pp + 7..]; // skip "package"

            // Skip whitespace between `package` and the name.
            let ws_len = after_pkg.len()
                - after_pkg.trim_start_matches(|c: char| c.is_ascii_whitespace()).len();
            let after_ws = &after_pkg[ws_len..];

            // Extract the package name, including `::` separators (e.g. `Foo::Bar`).
            let name_len = after_ws
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != ':')
                .unwrap_or(after_ws.len());

            if name_len > 0 {
                let pkg_name = &after_ws[..name_len];
                if pkg_name
                    .as_bytes()
                    .first()
                    .is_some_and(|&b| b.is_ascii_alphabetic() || b == b'_')
                {
                    // Accept only `package NAME;` or `package NAME {` patterns.
                    let after_name = after_ws[name_len..].trim_start();
                    if after_name.starts_with(';') || after_name.starts_with('{') {
                        current_package = Some(pkg_name.trim_end_matches(':').to_string());
                    }
                }
            }

            let advance = pp + 7 + ws_len + name_len.max(1);
            if advance >= search.len() {
                break;
            }
            search = &search[advance..];
            continue;
        }

        // Process a `sub` keyword.
        let sp = sub_pos.unwrap_or(0);
        let after_sub = &search[sp + 3..]; // skip "sub"

        // Skip whitespace between `sub` and the name.
        let ws_len =
            after_sub.len() - after_sub.trim_start_matches(|c: char| c.is_ascii_whitespace()).len();
        let after_ws = &after_sub[ws_len..];

        // Reject: anonymous sub (`sub {`) or sigil-prefixed (`sub $name`).
        if after_ws.starts_with('{') || after_ws.starts_with(['$', '@', '%', '&', '*']) {
            let advance = sp + 3 + ws_len.max(1);
            if advance >= search.len() {
                break;
            }
            search = &search[advance..];
            continue;
        }

        // Extract the identifier name (bare, no `::` — qualified names in sub
        // declarations are a separate future feature).
        let name_len = after_ws
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(after_ws.len());

        if name_len > 0 {
            let name = &after_ws[..name_len];
            // Validate: must start with a letter or underscore (not a digit).
            if name.as_bytes().first().is_some_and(|&b| b.is_ascii_alphabetic() || b == b'_') {
                // Validate: what follows the name must look like a Perl sub declaration.
                // Accept: `{`, `;`, `(` (optionally preceded by whitespace).
                // - `sub NAME {`   — named sub with body
                // - `sub NAME ;`   — forward declaration
                // - `sub NAME (`   — named sub with prototype or signature
                // Reject everything else, including bare `sub NAME` at end-of-string
                // (ambiguous — could be prose containing the word "sub").
                let after_name = after_ws[name_len..].trim_start();
                let plausible = after_name.starts_with('{')
                    || after_name.starts_with(';')
                    || after_name.starts_with('(');
                if plausible {
                    emit_triple(
                        name,
                        current_package.as_deref(),
                        node_start_byte,
                        node_end_byte,
                        file_id,
                        out,
                    );
                }
            }
        }

        // Advance past the name to continue scanning.
        let advance = sp + 3 + ws_len + name_len.max(1);
        if advance >= search.len() {
            break;
        }
        search = &search[advance..];
    }
}

/// Find the byte offset of the next `sub` keyword in `text` that is preceded
/// by a word boundary (not part of a longer identifier like `suburb`).
fn find_sub_keyword(text: &str) -> Option<usize> {
    let mut start = 0;
    while start < text.len() {
        let pos = text[start..].find("sub")?;
        let abs_pos = start + pos;

        // Check left boundary: must be at start or preceded by non-word char.
        let left_ok = abs_pos == 0
            || !text.as_bytes()[abs_pos - 1].is_ascii_alphanumeric()
                && text.as_bytes()[abs_pos - 1] != b'_';

        // Check right boundary: must be followed by whitespace or end.
        let right_byte = text.as_bytes().get(abs_pos + 3).copied();
        let right_ok = right_byte.map(|b| b.is_ascii_whitespace()).unwrap_or(true);

        if left_ok && right_ok {
            return Some(abs_pos);
        }

        start = abs_pos + 3;
    }
    None
}

/// Find the byte offset of the next `package` keyword in `text` that is at a
/// word boundary (not part of a longer identifier like `repackage`).
fn find_package_keyword(text: &str) -> Option<usize> {
    let mut start = 0;
    while start < text.len() {
        let pos = text[start..].find("package")?;
        let abs_pos = start + pos;

        // Check left boundary: must be at start or preceded by non-word char.
        let left_ok = abs_pos == 0
            || !text.as_bytes()[abs_pos - 1].is_ascii_alphanumeric()
                && text.as_bytes()[abs_pos - 1] != b'_';

        // Check right boundary: must be followed by whitespace or end.
        let right_byte = text.as_bytes().get(abs_pos + 7).copied();
        let right_ok = right_byte.map(|b| b.is_ascii_whitespace()).unwrap_or(true);

        if left_ok && right_ok {
            return Some(abs_pos);
        }

        start = abs_pos + 7;
    }
    None
}

/// Emit a `(EntityFact, AnchorFact, OccurrenceFact)` triple for a named sub
/// found in an eval string.
///
/// `node_start_byte` and `node_end_byte` are from the enclosing `Eval` AST
/// node's `location.start` and `location.end` — these are the real source
/// positions of the eval expression, used directly as the anchor span.
///
/// When `package` is `Some("Foo")`, the emitted `canonical_name` is `Foo::sub_name`
/// and confidence is upgraded to `Medium` (package-scoped facts are more reliable
/// than bare-name facts).  When `package` is `None`, the name is bare and
/// confidence stays at `Low`.
fn emit_triple(
    name: &str,
    package: Option<&str>,
    node_start_byte: usize,
    node_end_byte: usize,
    file_id: FileId,
    out: &mut Vec<(EntityFact, AnchorFact, OccurrenceFact)>,
) {
    let canonical_name = match package {
        Some(pkg) => format!("{pkg}::{name}"),
        None => name.to_string(),
    };
    let confidence = if package.is_some() { Confidence::Medium } else { Confidence::Low };

    // Stable ID derivation: hash (file_id, node_start_byte, canonical_name).
    // Including the package in the hash ensures `Foo::bar` and `bar` get
    // different IDs even when they appear in the same eval at the same offset.
    let base_id = stable_id(file_id.0, node_start_byte as u64, &canonical_name);

    let entity_id = EntityId(base_id);
    let anchor_id = AnchorId(base_id + 1);
    let occurrence_id = OccurrenceId(base_id + 2);

    let entity = EntityFact {
        id: entity_id,
        canonical_name,
        kind: EntityKind::Subroutine,
        anchor_id: Some(anchor_id),
        scope_id: None,
        provenance: Provenance::DynamicBoundary,
        confidence,
    };

    // Use the real AST span from the enclosing eval node.
    // node_end_byte comes from node.location.end, which is the source position
    // of the end of the entire eval expression (including closing quote/paren).
    let span_end =
        if node_end_byte > node_start_byte { node_end_byte } else { node_start_byte + 1 };
    let anchor = AnchorFact {
        id: anchor_id,
        file_id,
        span_start_byte: node_start_byte as u32,
        span_end_byte: span_end as u32,
        scope_id: None,
        provenance: Provenance::DynamicBoundary,
        confidence,
    };

    let occurrence = OccurrenceFact {
        id: occurrence_id,
        kind: OccurrenceKind::DynamicBoundary,
        entity_id: Some(entity_id),
        anchor_id,
        scope_id: None,
        provenance: Provenance::DynamicBoundary,
        confidence,
    };

    out.push((entity, anchor, occurrence));
}

/// Compute a stable u64 ID from (file_id, node_start, name) using FNV-1a.
fn stable_id(file_id: u64, node_start: u64, name: &str) -> u64 {
    // FNV-1a 64-bit hash.
    const FNV_OFFSET: u64 = 14_695_981_039_346_656_037;
    const FNV_PRIME: u64 = 1_099_511_628_211;

    let mut hash = FNV_OFFSET;
    for &byte in &file_id.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    for &byte in &node_start.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    for &byte in name.as_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    // Reserve 3 IDs per triple (entity, anchor, occurrence).
    // Shift left by 3 bits so base_id, base_id+1, base_id+2 are in a cluster.
    // Use a high-base offset (0xE_0000_0000) to avoid collisions with symbol
    // adapter IDs which start from lower values.
    0xE_0000_0000_u64.wrapping_add(hash.wrapping_shl(3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_semantic_facts::FileId;

    // ── Unit tests for find_sub_keyword ──

    #[test]
    fn find_sub_keyword_basic() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(find_sub_keyword("sub foo { 1 }"), Some(0));
        assert_eq!(find_sub_keyword("  sub bar { }"), Some(2));
        // The FIRST `sub` in the string is at position 3 ("no sub here").
        assert_eq!(find_sub_keyword("no sub here really sub baz"), Some(3));
        Ok(())
    }

    #[test]
    fn find_sub_keyword_rejects_suburb() -> Result<(), Box<dyn std::error::Error>> {
        // "suburb" contains "sub" but as part of a word — must not match.
        assert_eq!(find_sub_keyword("suburb"), None);
        // "subsub" also should not match as a keyword.
        // Note: "sub sub" should match the second one.
        assert_eq!(find_sub_keyword("sub sub foo"), Some(0));
        Ok(())
    }

    #[test]
    fn find_sub_keyword_none_when_absent() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(find_sub_keyword("hello world"), None);
        assert_eq!(find_sub_keyword(""), None);
        Ok(())
    }

    // ── Unit tests for extract_eval_sub_boundaries ──

    fn parse_and_extract(
        code: &str,
        file_id: FileId,
    ) -> Vec<(EntityFact, AnchorFact, OccurrenceFact)> {
        let mut parser = crate::Parser::new(code);
        let ast = match parser.parse() {
            Ok(a) => a,
            Err(_) => return vec![],
        };
        extract_eval_sub_boundaries(&ast, file_id)
    }

    #[test]
    fn extracts_single_sub_from_eval_string() -> Result<(), Box<dyn std::error::Error>> {
        let file_id = FileId(1);
        let triples = parse_and_extract(r#"eval "sub generated_from_string { 1 }";"#, file_id);

        assert_eq!(triples.len(), 1, "should extract exactly one sub");
        let (entity, _anchor, occurrence) = &triples[0];
        assert_eq!(entity.canonical_name, "generated_from_string");
        assert_eq!(entity.kind, EntityKind::Subroutine);
        assert_eq!(entity.provenance, Provenance::DynamicBoundary);
        assert_eq!(entity.confidence, Confidence::Low);
        assert_eq!(occurrence.kind, OccurrenceKind::DynamicBoundary);
        assert_eq!(occurrence.entity_id, Some(entity.id));
        Ok(())
    }

    #[test]
    fn extracts_multiple_subs_from_eval_string() -> Result<(), Box<dyn std::error::Error>> {
        let file_id = FileId(2);
        let triples = parse_and_extract(r#"eval "sub foo { 1 } sub bar { 2 }";"#, file_id);

        assert_eq!(triples.len(), 2, "should extract two subs");
        let names: Vec<&str> = triples.iter().map(|(e, _, _)| e.canonical_name.as_str()).collect();
        assert!(names.contains(&"foo"), "should include 'foo'");
        assert!(names.contains(&"bar"), "should include 'bar'");
        Ok(())
    }

    #[test]
    fn non_literal_eval_does_not_produce_evidence() -> Result<(), Box<dyn std::error::Error>> {
        // `eval $code` — non-literal, must not emit evidence.
        let file_id = FileId(3);
        let triples = parse_and_extract(r#"eval $code;"#, file_id);
        assert!(triples.is_empty(), "non-literal eval must not produce evidence");
        Ok(())
    }

    #[test]
    fn eval_block_does_not_produce_evidence() -> Result<(), Box<dyn std::error::Error>> {
        // `eval { ... }` — block eval, must not emit evidence.
        let file_id = FileId(4);
        let triples = parse_and_extract(r#"eval { die "oops" };"#, file_id);
        assert!(triples.is_empty(), "block eval must not produce evidence");
        Ok(())
    }

    #[test]
    fn anonymous_sub_in_eval_does_not_produce_named_evidence()
    -> Result<(), Box<dyn std::error::Error>> {
        // `eval "sub { 1 }"` — anonymous sub, no name to extract.
        let file_id = FileId(5);
        let triples = parse_and_extract(r#"eval "sub { 1 }";"#, file_id);
        assert!(triples.is_empty(), "anonymous sub in eval must not produce named evidence");
        Ok(())
    }

    #[test]
    fn prose_sub_in_eval_does_not_produce_evidence() -> Result<(), Box<dyn std::error::Error>> {
        // A string that contains the word "sub" in prose should not produce evidence.
        // "no sub here really sub baz" has no Perl declaration delimiters after the name.
        let file_id = FileId(6);
        // Parse as a Perl string literal rather than through the parser to test
        // the extractor function directly.
        let triples = {
            let mut out = Vec::new();
            extract_from_eval_string("no sub here really sub baz", 0, 26, file_id, &mut out);
            out
        };
        assert!(
            triples.is_empty(),
            "prose containing 'sub' without delimiters must not produce evidence, got: {:?}",
            triples.iter().map(|(e, _, _)| e.canonical_name.as_str()).collect::<Vec<_>>()
        );
        Ok(())
    }

    #[test]
    fn sub_with_semicolon_delimiter_is_accepted() -> Result<(), Box<dyn std::error::Error>> {
        // Forward declaration: `sub foo;`
        let file_id = FileId(7);
        let triples = {
            let mut out = Vec::new();
            extract_from_eval_string("sub forward_decl;", 0, 18, file_id, &mut out);
            out
        };
        assert_eq!(triples.len(), 1, "sub NAME; (forward decl) should produce evidence");
        assert_eq!(triples[0].0.canonical_name, "forward_decl");
        Ok(())
    }

    #[test]
    fn sub_with_prototype_is_accepted() -> Result<(), Box<dyn std::error::Error>> {
        // Named sub with prototype: `sub proto_sub ($$) { }`
        let file_id = FileId(8);
        let triples = {
            let mut out = Vec::new();
            extract_from_eval_string("sub proto_sub ($$) { 1 }", 0, 24, file_id, &mut out);
            out
        };
        assert_eq!(triples.len(), 1, "sub NAME (proto) should produce evidence");
        assert_eq!(triples[0].0.canonical_name, "proto_sub");
        Ok(())
    }

    #[test]
    fn interpolated_name_sub_does_not_produce_evidence() -> Result<(), Box<dyn std::error::Error>> {
        // `sub $name { ... }` — dynamic name, cannot be extracted.
        let file_id = FileId(9);
        let triples = {
            let mut out = Vec::new();
            extract_from_eval_string("sub $dynamic_name { 1 }", 0, 23, file_id, &mut out);
            out
        };
        assert!(triples.is_empty(), "sub with sigil-prefixed name must not produce evidence");
        Ok(())
    }

    #[test]
    fn whitespace_variants_between_keyword_and_name_are_accepted()
    -> Result<(), Box<dyn std::error::Error>> {
        let file_id = FileId(10);
        let triples = {
            let mut out = Vec::new();
            extract_from_eval_string(
                "sub\twith_tab { 1 } sub\nwith_newline; sub\r\nwith_crlf () { 1 }",
                4,
                67,
                file_id,
                &mut out,
            );
            out
        };

        let names: Vec<&str> =
            triples.iter().map(|(entity, _, _)| entity.canonical_name.as_str()).collect();
        assert_eq!(names, vec!["with_tab", "with_newline", "with_crlf"]);
        Ok(())
    }

    #[test]
    fn invalid_candidate_does_not_stop_later_valid_declaration()
    -> Result<(), Box<dyn std::error::Error>> {
        let file_id = FileId(11);
        let triples = {
            let mut out = Vec::new();
            extract_from_eval_string(
                "sub 123_not_a_name { 1 } sub $dynamic { 2 } sub valid_after_invalid { 3 }",
                0,
                73,
                file_id,
                &mut out,
            );
            out
        };

        assert_eq!(triples.len(), 1, "only the later static declaration should match");
        assert_eq!(triples[0].0.canonical_name, "valid_after_invalid");
        Ok(())
    }

    #[test]
    fn keyword_boundaries_reject_identifier_fragments() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(find_sub_keyword("gsub name { 1 }"), None);
        assert_eq!(find_sub_keyword("prefix_sub name { 1 }"), None);
        assert_eq!(find_sub_keyword("subroutine name { 1 }"), None);
        assert_eq!(find_sub_keyword("sub:name { 1 }"), None);
        Ok(())
    }

    #[test]
    fn unsupported_declaration_shapes_are_ignored() -> Result<(), Box<dyn std::error::Error>> {
        let file_id = FileId(12);
        let triples = {
            let mut out = Vec::new();
            extract_from_eval_string(
                "sub bare_name sub namespaced::method { 1 } sub hyphen-name { 2 } sub _ok { 3 }",
                0,
                78,
                file_id,
                &mut out,
            );
            out
        };

        assert_eq!(triples.len(), 1, "only the simple identifier declaration is supported");
        assert_eq!(triples[0].0.canonical_name, "_ok");
        Ok(())
    }

    // ── Package-scope attribution tests (Req 7.5b) ──

    #[test]
    fn package_declaration_qualifies_sub_name() -> Result<(), Box<dyn std::error::Error>> {
        let file_id = FileId(14);
        let triples = {
            let mut out = Vec::new();
            extract_from_eval_string(
                "package Dynamic; sub generated { 1 }",
                0,
                37,
                file_id,
                &mut out,
            );
            out
        };

        assert_eq!(triples.len(), 1, "should extract one sub with package qualification");
        let (entity, _anchor, _occurrence) = &triples[0];
        assert_eq!(
            entity.canonical_name, "Dynamic::generated",
            "canonical_name must be package-qualified"
        );
        assert_eq!(
            entity.confidence,
            Confidence::Medium,
            "package-scoped facts have Medium confidence"
        );
        assert_eq!(entity.kind, EntityKind::Subroutine);
        assert_eq!(entity.provenance, Provenance::DynamicBoundary);
        Ok(())
    }

    #[test]
    fn multiple_package_switches_qualify_each_sub_correctly()
    -> Result<(), Box<dyn std::error::Error>> {
        let file_id = FileId(15);
        let triples = {
            let mut out = Vec::new();
            extract_from_eval_string(
                "package A; sub x { 1 } package B; sub y { 2 }",
                0,
                47,
                file_id,
                &mut out,
            );
            out
        };

        assert_eq!(triples.len(), 2, "should extract two subs with their respective packages");
        let names: Vec<&str> = triples.iter().map(|(e, _, _)| e.canonical_name.as_str()).collect();
        assert!(names.contains(&"A::x"), "first sub should be A::x");
        assert!(names.contains(&"B::y"), "second sub should be B::y");
        // Both are package-scoped, so Medium confidence
        for (entity, _, _) in &triples {
            assert_eq!(entity.confidence, Confidence::Medium);
        }
        Ok(())
    }

    #[test]
    fn sub_before_package_declaration_is_unscoped() -> Result<(), Box<dyn std::error::Error>> {
        let file_id = FileId(16);
        let triples = {
            let mut out = Vec::new();
            extract_from_eval_string(
                "sub early { 1 } package Foo; sub late { 2 }",
                0,
                44,
                file_id,
                &mut out,
            );
            out
        };

        assert_eq!(triples.len(), 2, "should extract both subs");
        let names: Vec<&str> = triples.iter().map(|(e, _, _)| e.canonical_name.as_str()).collect();
        assert!(names.contains(&"early"), "sub before package declaration is unscoped");
        assert!(names.contains(&"Foo::late"), "sub after package declaration is qualified");

        // Unscoped sub has Low confidence
        let early = triples.iter().find(|(e, _, _)| e.canonical_name == "early").unwrap();
        assert_eq!(early.0.confidence, Confidence::Low, "unscoped sub should be Low confidence");
        // Package-scoped sub has Medium confidence
        let late = triples.iter().find(|(e, _, _)| e.canonical_name == "Foo::late").unwrap();
        assert_eq!(
            late.0.confidence,
            Confidence::Medium,
            "package-scoped sub should be Medium confidence"
        );
        Ok(())
    }

    #[test]
    fn nested_package_name_with_double_colons() -> Result<(), Box<dyn std::error::Error>> {
        let file_id = FileId(17);
        let triples = {
            let mut out = Vec::new();
            extract_from_eval_string("package Foo::Bar; sub baz { 1 }", 0, 31, file_id, &mut out);
            out
        };

        assert_eq!(triples.len(), 1, "should extract one sub");
        assert_eq!(
            triples[0].0.canonical_name, "Foo::Bar::baz",
            "nested package names are preserved"
        );
        Ok(())
    }

    #[test]
    fn unscoped_eval_without_package_stays_low_confidence() -> Result<(), Box<dyn std::error::Error>>
    {
        let file_id = FileId(18);
        let triples = {
            let mut out = Vec::new();
            extract_from_eval_string("sub plain { 1 }", 0, 15, file_id, &mut out);
            out
        };

        assert_eq!(triples.len(), 1);
        assert_eq!(triples[0].0.canonical_name, "plain");
        assert_eq!(
            triples[0].0.confidence,
            Confidence::Low,
            "unscoped eval subs stay at Low confidence"
        );
        Ok(())
    }

    #[test]
    fn package_qualified_subs_produce_distinct_ids() -> Result<(), Box<dyn std::error::Error>> {
        let file_id = FileId(19);
        // Two different packages with a sub of the same bare name must get different IDs.
        let triples = {
            let mut out = Vec::new();
            extract_from_eval_string(
                "package A; sub foo { 1 } package B; sub foo { 2 }",
                0,
                50,
                file_id,
                &mut out,
            );
            out
        };

        assert_eq!(triples.len(), 2);
        let id_a = triples[0].0.id;
        let id_b = triples[1].0.id;
        assert_ne!(id_a, id_b, "A::foo and B::foo must have different entity IDs");
        Ok(())
    }

    #[test]
    fn find_package_keyword_basic() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(find_package_keyword("package Foo;"), Some(0));
        assert_eq!(find_package_keyword("  package Bar { }"), Some(2));
        // The FIRST `package` in the string is at position 3 ("no ").
        assert_eq!(find_package_keyword("no package here really package Baz;"), Some(3));
        Ok(())
    }

    #[test]
    fn find_package_keyword_rejects_non_keyword() -> Result<(), Box<dyn std::error::Error>> {
        // "repackage" contains "package" but as part of a longer word.
        assert_eq!(find_package_keyword("repackage"), None);
        // "mypackage" also must not match.
        assert_eq!(find_package_keyword("mypackage Foo;"), None);
        Ok(())
    }

    #[test]
    fn find_package_keyword_none_when_absent() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(find_package_keyword("sub foo { }"), None);
        assert_eq!(find_package_keyword(""), None);
        Ok(())
    }

    #[test]
    fn package_and_sub_in_full_eval_via_parser() -> Result<(), Box<dyn std::error::Error>> {
        let file_id = FileId(20);
        let triples =
            parse_and_extract(r#"eval "package Dynamic; sub generated_sub { 1 }";"#, file_id);

        assert_eq!(triples.len(), 1, "should extract one package-scoped sub from full eval");
        let (entity, anchor, occurrence) = &triples[0];
        assert_eq!(entity.canonical_name, "Dynamic::generated_sub");
        assert_eq!(entity.confidence, Confidence::Medium);
        assert_eq!(occurrence.kind, OccurrenceKind::DynamicBoundary);
        assert_eq!(occurrence.entity_id, Some(entity.id));
        assert_eq!(anchor.confidence, Confidence::Medium);
        Ok(())
    }

    #[test]
    fn emit_triple_normalizes_empty_or_reversed_spans() -> Result<(), Box<dyn std::error::Error>> {
        let mut out = Vec::new();
        emit_triple("span_edge", None, 99, 42, FileId(13), &mut out);

        assert_eq!(out.len(), 1);
        let (_entity, anchor, occurrence) = &out[0];
        assert_eq!(anchor.span_start_byte, 99);
        assert_eq!(anchor.span_end_byte, 100);
        assert_eq!(occurrence.anchor_id, anchor.id);
        Ok(())
    }

    #[test]
    fn stable_id_is_deterministic() -> Result<(), Box<dyn std::error::Error>> {
        let id1 = stable_id(1, 42, "foo");
        let id2 = stable_id(1, 42, "foo");
        assert_eq!(id1, id2, "stable_id must be deterministic");

        let id3 = stable_id(1, 42, "bar");
        assert_ne!(id1, id3, "different names must produce different IDs");
        Ok(())
    }
}
