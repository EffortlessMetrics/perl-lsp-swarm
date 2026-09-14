//! Gate for the field-level source-geometry registry (#7015).
//!
//! The registry exists so a coordinate-mapping consumer (#13234) can enumerate
//! every payload field carrying byte offsets of its own. Two different things
//! have to be true for that to be trustworthy, and they are proven separately:
//!
//! 1. **Nothing is missing today.** The fully populated fixture bank is
//!    observed through `observe_geometry_fields` and reconciled against the
//!    registry, so a stale row or an unregistered field is red here.
//! 2. **Nothing can go missing later.** The reconciliation function is fed
//!    deliberately mutated inputs, proving it actually discriminates rather
//!    than returning `Ok` for everything.
//!
//! The second half needs care, because the obvious argument for it is wrong.
//! Exhaustive destructuring in `observe_geometry_fields` proves only that every
//! field *name* appears in a pattern. A new `Option<SourceLocation>` bound as
//! `field: _` satisfies the compiler, is never emitted by the observer, and so
//! reconciles clean; filing it under `untracked_fields` finishes hiding it, and
//! the no-`..` scan passes throughout. That escape was demonstrated, not
//! theorised, on this branch.
//!
//! `the_registry_covers_every_geometry_bearing_field_declared_in_the_enum` is
//! therefore the load-bearing guard: it reads the *declared field types* out of
//! `ast.rs`, so a field is geometry-bearing because of what it is, not because
//! an author remembered to say so. The pattern-exhaustiveness and rest-pattern
//! guards remain as earlier, cheaper tripwires.

use perl_ast::ast::{Token, TokenKind};
use perl_ast::{
    AST_GEOMETRY_SCHEMA_VERSION, AST_NODE_GEOMETRY_FIELDS, AstGeometryDisposition,
    AstGeometryDrift, AstGeometryField, AstGeometryMapping, AstGeometryShape,
    AstNodeClassification, NodeKind, ObservedGeometryField, SourceLocation, ast_node_policy,
    geometry_disposition_for_role, geometry_fields_for, geometry_shapes_in_use, node_kind_fixtures,
    observe_geometry_fields, reconcile_geometry_rows, reconcile_node_geometry,
};
use std::collections::{BTreeMap, BTreeSet};

/// Registry rows may only name kinds that exist, in canonical order.
#[test]
fn every_geometry_row_names_a_live_nodekind() {
    let canonical = NodeKind::ALL_KIND_NAMES.iter().copied().collect::<BTreeSet<_>>();
    for row in AST_NODE_GEOMETRY_FIELDS {
        assert!(
            canonical.contains(row.kind_name),
            "geometry row {}.{} names a NodeKind that does not exist",
            row.kind_name,
            row.field
        );
    }

    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    for row in AST_NODE_GEOMETRY_FIELDS {
        assert!(
            seen.insert((row.kind_name, row.field)),
            "geometry row {}.{} is registered more than once",
            row.kind_name,
            row.field
        );
    }

    let declaration_order: Vec<usize> = AST_NODE_GEOMETRY_FIELDS
        .iter()
        .filter_map(|row| NodeKind::ALL_KIND_NAMES.iter().position(|name| *name == row.kind_name))
        .collect();
    let mut sorted = declaration_order.clone();
    sorted.sort_unstable();
    assert_eq!(
        declaration_order, sorted,
        "geometry rows must follow NodeKind::ALL_KIND_NAMES declaration order so the table can be \
         read against the enum"
    );

    assert_eq!(AST_GEOMETRY_SCHEMA_VERSION, 1, "geometry schema version drifted");
}

/// The positive gate: every variant's real geometry matches its rows.
#[test]
fn every_fixture_reconciles_with_its_registered_geometry() -> Result<(), Box<dyn std::error::Error>>
{
    for fixture in node_kind_fixtures() {
        let kind_name = fixture.sample.kind.kind_name();
        reconcile_node_geometry(&fixture.sample)
            .map_err(|drift| format!("{kind_name}: {drift}"))?;
    }
    Ok(())
}

/// A fully populated sample must actually exercise every registered field.
///
/// Without this, a registry row could describe a field that the fixture leaves
/// absent, and the reconciliation above would still pass while proving nothing
/// about that field.
#[test]
fn the_populated_fixture_observes_every_registered_field() -> Result<(), Box<dyn std::error::Error>>
{
    for fixture in node_kind_fixtures() {
        let kind_name = fixture.sample.kind.kind_name();
        let observed = observe_geometry_fields(&fixture.sample.kind);

        for row in geometry_fields_for(kind_name) {
            let entry = observed
                .iter()
                .find(|entry| entry.field == row.field)
                .ok_or_else(|| format!("{kind_name}.{}: registered but not observed", row.field))?;

            match row.shape {
                AstGeometryShape::Direct | AstGeometryShape::Optional | AstGeometryShape::Token => {
                    assert_eq!(
                        entry.occurrences,
                        1,
                        "{kind_name}.{}: the fully populated fixture must carry exactly one span \
                         for a {} field, observed {}",
                        row.field,
                        row.shape.token(),
                        entry.occurrences
                    );
                }
                AstGeometryShape::Nested | AstGeometryShape::Repeated => {
                    assert!(
                        entry.occurrences >= 2,
                        "{kind_name}.{}: a {} field must observe more than one span on the fully \
                         populated fixture so repetition is provable, observed {}",
                        row.field,
                        row.shape.token(),
                        entry.occurrences
                    );
                }
            }
        }
    }
    Ok(())
}

/// Dispositions are derived from the field's role, floored by its owning node.
///
/// A row cannot choose its disposition, and it cannot name a role its owning
/// variant does not declare, so it cannot reach a friendlier disposition by
/// inventing a role.
#[test]
fn dispositions_are_derived_from_the_field_role_and_owning_classification()
-> Result<(), Box<dyn std::error::Error>> {
    for row in AST_NODE_GEOMETRY_FIELDS {
        let policy = ast_node_policy(row.kind_name)
            .ok_or_else(|| format!("{} has geometry rows but no policy row", row.kind_name))?;

        if let Some(role) = row.payload_role {
            assert!(
                policy.payload_policies.contains(&role),
                "{}.{}: claims payload role {role:?}, which {} does not declare",
                row.kind_name,
                row.field,
                row.kind_name
            );
        }

        let expected = geometry_disposition_for_role(row.payload_role, policy.classification);
        assert_eq!(
            row.disposition,
            expected,
            "{}.{}: role {:?} under classification {:?} requires disposition {} but the row \
             registers {}",
            row.kind_name,
            row.field,
            row.payload_role,
            policy.classification,
            expected.token(),
            row.disposition.token()
        );
    }
    Ok(())
}

/// A row may not drop its payload role where its variant declares one.
///
/// Raised in review, and it was a hole introduced by the role dimension itself.
/// `validate_geometry_registry` derived the required disposition from the role
/// with a classification fallback for `None` — so setting `Package.name_span` to
/// `payload_role: None` fell back to `ChildBearing` -> `SourceExact`, matched the
/// registered disposition, and validated clean. Reproduced before fixing: the
/// whole suite stayed green (29/29) with the role silently removed.
///
/// The coincidence is the point. For a declaration name on a child-bearing node
/// the fallback happens to agree, so nothing was red while the registry quietly
/// stopped recording *why* the disposition holds. On `Format` the same omission
/// would change the answer and be caught — which is exactly the kind of
/// inconsistent coverage that makes a guard untrustworthy.
///
/// `payload_role` is now `None` only for a variant declaring no payload policy.
#[test]
fn a_row_may_not_omit_a_role_its_variant_declares() -> Result<(), Box<dyn std::error::Error>> {
    for row in AST_NODE_GEOMETRY_FIELDS {
        let policy = ast_node_policy(row.kind_name)
            .ok_or_else(|| format!("{} has geometry rows but no policy row", row.kind_name))?;

        if policy.payload_policies.is_empty() {
            assert_eq!(
                row.payload_role, None,
                "{}.{}: {} declares no payload policy, so the row must not name a role",
                row.kind_name, row.field, row.kind_name
            );
        } else {
            assert!(
                row.payload_role.is_some(),
                "{}.{}: {} declares {:?}, so the row must name the role it realizes rather than \
                 falling back to classification-derived disposition",
                row.kind_name,
                row.field,
                row.kind_name,
                policy.payload_policies
            );
        }
    }

    // The coincidence that hid the defect: for Package the fallback agrees with
    // the role-derived answer, so disposition equality alone proves nothing here.
    let policy = ast_node_policy("Package").ok_or("Package policy must exist")?;
    assert_eq!(
        geometry_disposition_for_role(None, policy.classification),
        geometry_disposition_for_role(
            Some(perl_ast::AstPayloadPolicy::DeclarationNameAnchor),
            policy.classification
        ),
        "this control exists because these two agree for Package; if they ever diverge, the \
         omission would be caught by the disposition check and this test is no longer the guard"
    );
    Ok(())
}

/// A declaration name is exact source even on a boundary-classified node.
///
/// Raised in review. `Format` is classified `SourceBoundary` because its `body`
/// is an opaque source region, and deriving every field's disposition from the
/// classification alone recorded `Format.name_span` as boundary geometry. It is
/// a `DeclarationNameAnchor`, identical in kind to `Package.name_span`, and the
/// invariant policy already says so — the derivation was discarding a
/// distinction the registry had.
///
/// This pins the corrected rule in both directions on the same node, so a
/// future simplification back to classification-only derivation fails here.
#[test]
fn a_declaration_name_stays_exact_on_a_boundary_classified_node()
-> Result<(), Box<dyn std::error::Error>> {
    let policy = ast_node_policy("Format").ok_or("Format policy must exist")?;
    assert_eq!(
        policy.classification,
        AstNodeClassification::SourceBoundary,
        "this control is only meaningful while Format is boundary-classified"
    );

    let rows = geometry_fields_for("Format");
    let name_span = rows
        .iter()
        .find(|row| row.field == "name_span")
        .ok_or("Format.name_span must be registered")?;

    assert_eq!(
        name_span.payload_role,
        Some(perl_ast::AstPayloadPolicy::DeclarationNameAnchor),
        "Format.name_span realizes the declaration-name anchor Format declares"
    );
    assert_eq!(
        name_span.disposition,
        AstGeometryDisposition::SourceExact,
        "a declaration name anchors exact source text even when the node's body is opaque"
    );

    // The opposite direction on a node of the same classification: Heredoc's
    // body span really is boundary geometry and must not drift to exact.
    let heredoc = geometry_fields_for("Heredoc");
    let body_span = heredoc
        .iter()
        .find(|row| row.field == "body_span")
        .ok_or("Heredoc.body_span must be registered")?;
    assert_eq!(
        body_span.disposition,
        AstGeometryDisposition::SourceBoundary,
        "an opaque source region stays boundary geometry"
    );

    // And the classification floor still wins for recovery material.
    let error_rows = geometry_fields_for("Error");
    let found = error_rows.iter().find(|row| row.field == "found").ok_or("Error.found")?;
    assert_eq!(
        found.disposition,
        AstGeometryDisposition::Recovery,
        "a Recovery node's geometry stays recovery geometry whatever role the field plays"
    );
    Ok(())
}

/// Recovery geometry must be visible as recovery geometry.
#[test]
fn recovery_token_geometry_is_registered_as_recovery() -> Result<(), Box<dyn std::error::Error>> {
    let rows = geometry_fields_for("Error");
    let found = rows
        .iter()
        .find(|row| row.field == "found")
        .ok_or("Error.found must be registered as recovery-token geometry")?;

    assert_eq!(found.shape, AstGeometryShape::Token);
    assert_eq!(
        found.mapping,
        AstGeometryMapping::MapStartPreserveWidth,
        "a recovery token's width is established at construction; a remap moves the start and \
         carries the width across rather than recomputing it"
    );
    assert_eq!(found.disposition, AstGeometryDisposition::Recovery);

    let policy = ast_node_policy("Error").ok_or("Error policy must exist")?;
    assert_eq!(
        policy.classification,
        AstNodeClassification::Recovery,
        "Error must remain a recovery policy row"
    );
    Ok(())
}

/// The vocabulary is wider than current coverage; say so out loud.
///
/// `Repeated` is reserved. Registering the first row that uses it should be a
/// deliberate act that updates this denominator, not a silent widening.
#[test]
fn shape_coverage_is_an_explicit_denominator() {
    let in_use =
        geometry_shapes_in_use().iter().map(|shape| shape.token()).collect::<BTreeSet<_>>();
    let expected = ["direct", "nested", "optional", "token"].into_iter().collect::<BTreeSet<_>>();

    assert_eq!(
        in_use, expected,
        "the set of geometry shapes actually in use changed; update the denominator deliberately \
         rather than letting coverage drift"
    );
    assert!(
        !in_use.contains(AstGeometryShape::Repeated.token()),
        "AstGeometryShape::Repeated is documented as reserved vocabulary with no current row"
    );
}

/// Derive the geometry denominator from the enum's *declared field types*.
///
/// A field is geometry-bearing because of its type, not because someone
/// classified it. Reading `ast.rs` directly is what makes this independent of
/// the observer: a new span bound as `field: _` is invisible to every other
/// guard here, but not to this one.
/// Type identifiers that carry source offsets of their own.
const GEOMETRY_TYPES: &[&str] = &["SourceLocation", "Token"];

/// Type identifiers known to carry no source offsets.
///
/// `TokenKind` is here deliberately: it is a bare discriminant, unlike `Token`.
/// `Node` is here because child locations belong to structural traversal, not
/// to this registry.
const NEUTRAL_TYPES: &[&str] =
    &["Box", "GotoTargetForm", "Node", "Option", "String", "TokenKind", "Vec", "bool"];

/// Classify every type identifier in one field's declared type.
///
/// Returns whether the type carries source offsets, plus any identifier that is
/// on neither list. Classification is by allowlist, not by looking for known
/// geometry names: a denylist silently accepts what it does not recognise, so a
/// `type Span = SourceLocation` alias would read as neutral and its field would
/// escape the registry. An unrecognised identifier fails closed instead.
///
/// Lifetimes are stripped before tokenizing. `&'a SourceLocation` would
/// otherwise yield the word `a`, which is neither a geometry nor a neutral type
/// and would be reported as an unclassified *type* — misleading guidance for
/// something that is not a type at all. Stripping is deliberately narrow: it
/// removes `'name`, so a genuine one-letter type or generic parameter is still
/// reported.
/// Returns how many independent geometry members the type declares, not merely
/// whether it declares any. A nested record can carry more than one span inside
/// one declared field (`Vec<(Option<SourceLocation>, Option<SourceLocation>)>`),
/// and a boolean answer cannot tell that apart from a single span — which would
/// let a second nested span reach a coordinate remap unregistered.
fn classify_type_words(ty: &str) -> (usize, Vec<String>) {
    // Remove `'lifetime` sequences: an apostrophe followed by an identifier.
    let mut without_lifetimes = String::with_capacity(ty.len());
    let mut chars = ty.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\'' {
            while chars.peek().is_some_and(|c| c.is_alphanumeric() || *c == '_') {
                chars.next();
            }
            // Keep a separator so `&'a SourceLocation` does not fuse words.
            without_lifetimes.push(' ');
            continue;
        }
        without_lifetimes.push(ch);
    }

    let mut geometry_members = 0usize;
    let mut unclassified = Vec::new();
    for word in without_lifetimes.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if word.is_empty() {
            continue;
        }
        if GEOMETRY_TYPES.contains(&word) {
            geometry_members += 1;
        } else if !NEUTRAL_TYPES.contains(&word) {
            unclassified.push(word.to_string());
        }
    }
    (geometry_members, unclassified)
}

/// The lifetime carve-out must not weaken the allowlist.
#[test]
fn type_word_classification_ignores_lifetimes_but_not_types() {
    // A lifetime is not a type and must not be reported as an unclassified one.
    let (members, unknown) = classify_type_words("&'a SourceLocation");
    assert_eq!(members, 1, "a reference to a span still carries geometry");
    assert!(unknown.is_empty(), "a lifetime must not be reported as a type: {unknown:?}");

    let (_, unknown) = classify_type_words("Option<&'static Token>");
    assert!(unknown.is_empty(), "'static must not be reported as a type: {unknown:?}");

    // Stripping lifetimes must not start accepting unknown types.
    let (_, unknown) = classify_type_words("&'a Span");
    assert_eq!(unknown, vec!["Span".to_string()], "an unknown type must still fail closed");

    // A one-letter generic is a type, not a lifetime, and stays reported.
    let (_, unknown) = classify_type_words("Vec<T>");
    assert_eq!(unknown, vec!["T".to_string()], "a bare generic parameter must be classified");

    // The ordinary case is unaffected.
    let (members, unknown) = classify_type_words("Option<SourceLocation>");
    assert_eq!(members, 1);
    assert!(unknown.is_empty());

    let (members, unknown) = classify_type_words("Vec<TokenKind>");
    assert_eq!(members, 0, "TokenKind is a bare discriminant, not geometry");
    assert!(unknown.is_empty());

    // The count, not merely the presence, is what the nested guard relies on.
    let (members, unknown) =
        classify_type_words("Vec<(Option<(String, SourceLocation)>, Option<SourceLocation>)>");
    assert_eq!(members, 2, "two independent spans in one field must be counted as two");
    assert!(unknown.is_empty());
}

/// What the enum scan found: geometry fields, and any type it could not classify.
struct DeclaredFields {
    /// (variant, field, how many independent geometry members the field declares)
    geometry: Vec<(String, String, usize)>,
    /// (variant, field, unrecognised type identifier)
    unknown: Vec<(String, String, String)>,
}

fn declared_geometry_fields() -> DeclaredFields {
    scan_declared_fields(include_str!("../src/ast.rs"))
}

/// Remove Rust comments, leaving code and whitespace positions intact.
///
/// Handles line comments, **nested** block comments (Rust permits nesting), and
/// string literals including raw strings. Literal *contents* are neutralized to
/// spaces (newlines preserved) rather than copied through, which does two jobs
/// at once: a `"/*"` inside a literal cannot open a comment, and a `"}"` inside
/// one cannot count toward the enum's brace balance.
///
/// The second job was missing until human review found it. Preserving literal
/// text stopped the comment vector but kept the braces, and a multiline
/// attribute is enough to deliver one: the line filter drops only lines that
/// *start* with `#`, so the continuation of
///
/// ```text
/// #[cfg_attr(
///     feature = "x",
///     doc = "}"
/// )]
/// ```
///
/// survives with a live `}` in it. Reproduced against this scanner: the enum
/// body truncated and the declared fields were lost from the denominator
/// entirely.
///
/// Raised in review after the line-comment fix: stripping only `//` left `/* */`
/// braces counting toward the enum's brace balance. That is worse than the
/// line-comment case it replaced, because a block comment placed *after* the
/// ninth geometry field truncates the scan somewhere the `>= 9` floor cannot
/// see — the floor counts fields found, and nine were already found.
///
/// Char literals **are** tracked, and the earlier reasoning for skipping them
/// does not survive contact with the failure it allowed. That reasoning was: `'`
/// is ambiguous with a lifetime in type position, so guessing would corrupt the
/// scan; and the enum body carries no character literals anyway.
///
/// The second half was the load-bearing half, and it was too narrow — the scan
/// runs over the whole file, not the enum body, so a `const QUOTE: char = '"';`
/// anywhere above the enum was enough. Read naively, `'"'` opens a *string*, and
/// everything to the next `"` in the file is neutralized, taking real field
/// declarations with it. The `>= 9` floor below does catch that, so it was never
/// silent — but it reports a broken scanner rather than simply working.
///
/// The first half assumed the ambiguity is unresolvable. It is not, for the two
/// forms that occur: a char literal is `'` followed either by a backslash escape
/// or by exactly one character and then a closing `'`. A lifetime has no closing
/// quote, so looking ahead for one separates `'"'` and `'\''` from `'a` and
/// `'static` without guessing.
///
/// This is still a scanner sized for the job, not a Rust lexer. What it does not
/// handle — nested raw-string hash counts beyond the closing form, byte strings,
/// and any literal form Rust may add — remains a live maintenance edge, and the
/// floor remains the backstop for all of it.
fn strip_comments(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut index = 0usize;
    let mut block_depth = 0usize;

    while index < chars.len() {
        let ch = chars[index];
        let next = chars.get(index + 1).copied();

        if block_depth > 0 {
            if ch == '*' && next == Some('/') {
                block_depth -= 1;
                index += 2;
                continue;
            }
            if ch == '/' && next == Some('*') {
                block_depth += 1;
                index += 2;
                continue;
            }
            // Keep newlines so line-oriented filtering below stays aligned.
            if ch == '\n' {
                out.push('\n');
            }
            index += 1;
            continue;
        }

        if ch == '/' && next == Some('/') {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }

        if ch == '/' && next == Some('*') {
            block_depth = 1;
            index += 2;
            continue;
        }

        // Raw string: r"..." or r#"..."#, any number of hashes.
        if ch == 'r' {
            let mut hashes = 0usize;
            let mut probe = index + 1;
            while chars.get(probe) == Some(&'#') {
                hashes += 1;
                probe += 1;
            }
            if chars.get(probe) == Some(&'"') {
                out.push_str(&chars[index..=probe].iter().collect::<String>());
                index = probe + 1;
                loop {
                    match chars.get(index) {
                        None => break,
                        Some('"') => {
                            let closing = (1..=hashes).all(|n| chars.get(index + n) == Some(&'#'));
                            out.push('"');
                            index += 1;
                            if closing {
                                for _ in 0..hashes {
                                    out.push('#');
                                    index += 1;
                                }
                                break;
                            }
                        }
                        Some(&other) => {
                            // Neutralize, do not copy: a `}` in here would
                            // otherwise count toward the enum's brace balance.
                            out.push(if other == '\n' { '\n' } else { ' ' });
                            index += 1;
                        }
                    }
                }
                continue;
            }
        }

        // Char literal, e.g. `'"'` or `'\''`. This must be consumed before the
        // string-literal branch below, because a `'"'` would otherwise be read as
        // an *opening* double quote and neutralize everything up to the next `"`
        // in the file — the same desync class as findings 6 and 10, one quote
        // character over. A lifetime (`'a`, `'static`) is not a char literal and
        // must fall through untouched, so the two are told apart by looking for
        // the closing quote rather than by the opening one alone.
        if ch == '\'' {
            let is_escaped = next == Some('\\');
            let is_single = chars.get(index + 2) == Some(&'\'');
            if is_escaped || is_single {
                out.push('\'');
                index += 1;
                while index < chars.len() {
                    let inner = chars[index];
                    index += 1;
                    if inner == '\'' {
                        out.push('\'');
                        break;
                    }
                    // Neutralize, do not copy: a `}` or `"` inside the literal
                    // must not reach the brace balance or the string scanner.
                    out.push(' ');
                    if inner == '\\'
                        && let Some(&escaped) = chars.get(index)
                    {
                        out.push(if escaped == '\n' { '\n' } else { ' ' });
                        index += 1;
                    }
                }
                continue;
            }
        }

        if ch == '"' {
            out.push('"');
            index += 1;
            while index < chars.len() {
                let inner = chars[index];
                index += 1;
                if inner == '"' {
                    out.push('"');
                    break;
                }
                // Neutralize the contents rather than copying them. Preserving
                // the text kept a `"/*"` from opening a comment, but it also
                // kept the literal's braces, which then counted toward the enum
                // boundary. Newlines are kept so line structure is unchanged.
                out.push(if inner == '\n' { '\n' } else { ' ' });
                if inner == '\\'
                    && let Some(&escaped) = chars.get(index)
                {
                    out.push(if escaped == '\n' { '\n' } else { ' ' });
                    index += 1;
                }
            }
            continue;
        }

        out.push(ch);
        index += 1;
    }

    out
}

/// The scan itself, over arbitrary source.
///
/// Taking the source as a parameter is what lets the nested-escape control below
/// exercise this exact code against a synthetic enum. Testing it only against the
/// real `ast.rs` would mean the scanner's own blind spots could only be found by
/// mutating the production AST, which is expensive enough that it would not be
/// done routinely.
fn scan_declared_fields(ast_source: &str) -> DeclaredFields {
    // Strip comments and attributes *before* balancing braces, not after.
    //
    // Raised in review: the enum body used to be located by brace balance over
    // the raw source, and doc prose in this enum is full of braces —
    // `Variable { sigil: "@", .. }`, `@hash{qw(a b c)}`, `Block: { ... }`. They
    // happen to balance today (measured: net zero), so the scan was correct by
    // luck rather than by construction. One doc comment mentioning `${` or a
    // lone `}` would truncate the body early or run it past the enum's end,
    // silently changing what the load-bearing guard measures.
    //
    // Stripping first makes the brace balance see only code. The `>= 9` floor
    // below is the backstop for truncation, but it cannot catch over-extension,
    // nor truncation that happens *after* the ninth field — which is why the
    // stripping has to be right rather than merely backstopped.
    let stripped = strip_comments(ast_source);
    let source: String = stripped
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let source = source.as_str();

    // Isolate `pub enum NodeKind { .. }` by brace balance over comment-free code.
    let start = source.find("pub enum NodeKind {").unwrap_or(0);
    let body_start = source[start..].find('{').map_or(start, |i| start + i + 1);
    let mut depth = 1usize;
    let mut end = body_start;
    for (offset, ch) in source[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = body_start + offset;
                    break;
                }
            }
            _ => {}
        }
    }
    let cleaned = &source[body_start..end];

    let mut declared = Vec::new();
    let mut unknown = Vec::new();
    let chars: Vec<char> = cleaned.chars().collect();
    let mut index = 0usize;

    while index < chars.len() {
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }

        let mut name = String::new();
        while index < chars.len() && (chars[index].is_alphanumeric() || chars[index] == '_') {
            name.push(chars[index]);
            index += 1;
        }
        if name.is_empty() {
            index += 1;
            continue;
        }

        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        if index >= chars.len() || chars[index] != '{' {
            continue; // unit variant, or not a variant head
        }

        index += 1;
        let field_start = index;
        let mut variant_depth = 1usize;
        while index < chars.len() && variant_depth > 0 {
            match chars[index] {
                '{' => variant_depth += 1,
                '}' => variant_depth -= 1,
                _ => {}
            }
            index += 1;
        }
        let variant_body: String = chars[field_start..index.saturating_sub(1)].iter().collect();

        // Split fields on commas outside any generic/tuple nesting.
        let mut nesting = 0i32;
        let mut current = String::new();
        let mut chunks = Vec::new();
        for ch in variant_body.chars() {
            match ch {
                '<' | '(' | '[' | '{' => nesting += 1,
                '>' | ')' | ']' | '}' => nesting -= 1,
                ',' if nesting == 0 => {
                    chunks.push(std::mem::take(&mut current));
                    continue;
                }
                _ => {}
            }
            current.push(ch);
        }
        chunks.push(current);

        for chunk in chunks {
            let Some((field, ty)) = chunk.split_once(':') else { continue };
            let field = field.trim();
            if field.is_empty() {
                continue;
            }

            let (geometry_members, unclassified) = classify_type_words(ty);
            for word in unclassified {
                unknown.push((name.clone(), field.to_string(), word));
            }
            if geometry_members > 0 {
                declared.push((name.clone(), field.to_string(), geometry_members));
            }
        }
    }

    DeclaredFields { geometry: declared, unknown }
}

#[test]
fn the_registry_covers_every_geometry_bearing_field_declared_in_the_enum() {
    let scan = declared_geometry_fields();

    // Fail closed on any type the scan cannot classify. This is what closes the
    // alias vector: `type Span = SourceLocation` would arrive here as an
    // unrecognised identifier rather than being silently treated as neutral.
    assert!(
        scan.unknown.is_empty(),
        "these NodeKind field types use identifiers this scan cannot classify: {:?}\nAdd each to \
         GEOMETRY_TYPES (it carries source offsets) or NEUTRAL_TYPES (it does not). An alias for a \
         span type must go in GEOMETRY_TYPES, or geometry will escape the registry through it.",
        scan.unknown
    );

    let declared = scan.geometry;

    assert!(
        declared.len() >= 9,
        "the enum scan found only {} geometry-bearing fields; that means this scanner broke, not \
         that the enum shrank, and a broken scanner would silently stop guarding anything",
        declared.len()
    );

    compare_declared_against_registry(&declared, AST_NODE_GEOMETRY_FIELDS);
}

/// Compare a declared-field scan against a registry, by member count.
///
/// Counting rather than testing set membership is what closes the nested-escape
/// vector raised in review: registry identities are dotted for nested records
/// (`catch_blocks.variable`), so collapsing them to their outer field made one
/// span and two spans inside the same field indistinguishable. A second
/// `SourceLocation` added to an already-registered nested field changed neither
/// the declared set nor the registered set, and — because it introduces no new
/// field *name* — did not break exhaustive destructuring either. It could reach
/// a coordinate remap unregistered.
///
/// The number of rows whose base is a given field must now equal the number of
/// geometry members that field's declared type carries.
fn compare_declared_against_registry(
    declared: &[(String, String, usize)],
    registry: &[AstGeometryField],
) {
    let mut registered: BTreeMap<(String, String), usize> = BTreeMap::new();
    for row in registry {
        let base = row.field.split('.').next().unwrap_or(row.field);
        *registered.entry((row.kind_name.to_string(), base.to_string())).or_default() += 1;
    }

    let mut declared_counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (variant, field, members) in declared {
        *declared_counts.entry((variant.clone(), field.clone())).or_default() += members;
    }

    let unregistered: Vec<_> = declared_counts
        .iter()
        .filter(|(key, _)| !registered.contains_key(*key))
        .map(|(key, count)| (key.clone(), *count))
        .collect();
    assert!(
        unregistered.is_empty(),
        "these NodeKind fields are declared with span-bearing types but have no row in the \
         geometry registry: {unregistered:?}\nA coordinate remap would leave them at stale \
         offsets. Binding such a field as `_` in observe_geometry_fields does not exempt it."
    );

    let phantom: Vec<_> =
        registered.keys().filter(|key| !declared_counts.contains_key(*key)).cloned().collect();
    assert!(
        phantom.is_empty(),
        "these geometry rows name fields the enum no longer declares with a span-bearing type: \
         {phantom:?}"
    );

    let miscounted: Vec<_> = declared_counts
        .iter()
        .filter_map(|(key, declared_members)| {
            let rows = registered.get(key).copied().unwrap_or(0);
            (rows != *declared_members).then(|| (key.clone(), *declared_members, rows))
        })
        .collect();
    assert!(
        miscounted.is_empty(),
        "these fields declare a different number of geometry members than the registry has rows \
         for: {miscounted:?} (as (variant, field), declared members, registered rows)\nA nested \
         field that gains a second span needs a second dotted row; without one it reaches a \
         coordinate remap unregistered while the outer field still looks covered."
    );
}

/// A brace inside a string literal must not move the enum boundary.
///
/// Raised in human review, and the reported vector is a multiline attribute:
/// the line filter drops only lines *starting* with `#`, so the continuation
/// lines of
///
/// ```text
/// #[cfg_attr(
///     feature = "x",
///     doc = "}"
/// )]
/// ```
///
/// survive, and the `}` inside that string counted toward the enum's brace
/// balance. Reproduced: the body truncated at `doc = "` and `name_span` was
/// lost from the denominator entirely.
///
/// The earlier string handling preserved literal contents so a `"/*"` could not
/// open a comment. That was necessary but not sufficient — preserving the text
/// also preserved its braces. Contents are now neutralized instead: the quotes
/// and newlines stay so structure is unchanged, the characters between them do
/// not.
#[test]
fn a_brace_inside_a_string_literal_does_not_move_the_enum_boundary() {
    let synthetic = r####"
        pub enum NodeKind {
            #[cfg_attr(
                feature = "x",
                doc = "}"
            )]
            Package {
                name: String,
                name_span: SourceLocation,
            },
            #[cfg_attr(
                feature = "y",
                doc = r#"unbalanced } in a raw string"#
            )]
            Class {
                name: String,
                name_span: SourceLocation,
            },
            // An escaped quote must not terminate the literal early. If it did,
            // the `}` after it would leave the string and reach the balancer.
            #[cfg_attr(
                feature = "z",
                doc = "an escaped quote \" then a brace }"
            )]
            Method {
                name: String,
                name_span: SourceLocation,
            },
        }
    "####;

    let scan = scan_declared_fields(synthetic);
    assert!(scan.unknown.is_empty(), "synthetic source must classify cleanly: {:?}", scan.unknown);

    let found: BTreeSet<(String, String)> =
        scan.geometry.iter().map(|(v, f, _)| (v.clone(), f.clone())).collect();
    let expected: BTreeSet<(String, String)> = [
        ("Package".to_string(), "name_span".to_string()),
        ("Class".to_string(), "name_span".to_string()),
        ("Method".to_string(), "name_span".to_string()),
    ]
    .into_iter()
    .collect();

    assert_eq!(
        found, expected,
        "a brace inside an ordinary or raw string literal must not truncate the enum scan; \
         losing a field here silently shrinks the denominator the whole gate depends on"
    );
}

/// A `char` literal holding a quote must not open a string literal.
///
/// Same desync class as the two string-literal findings, one quote character
/// over: `'"'` read naively opens a double-quoted string, so everything up to
/// the next `"` anywhere in the file is neutralized — and in `ast.rs` that is a
/// long way, taking real fields with it. The sentinel on the total-loss case
/// does catch it, so this was never a silent hole, but it failed with a
/// scanner-is-broken message rather than simply working.
///
/// Lifetimes share the opening character and must survive untouched, so both
/// are asserted here: `'"'` is consumed as a literal, `'a` is not.
#[test]
fn a_char_literal_holding_a_quote_does_not_open_a_string() {
    let synthetic = r####"
        pub const QUOTE_CHAR: char = '"';
        pub const ESCAPED: char = '\'';
        pub const BRACE: char = '}';

        pub enum NodeKind<'a> {
            Package {
                name: String,
                name_span: SourceLocation,
            },
            Class {
                name: String,
                name_span: SourceLocation,
            },
        }
    "####;

    let scan = scan_declared_fields(synthetic);
    assert!(scan.unknown.is_empty(), "synthetic source must classify cleanly: {:?}", scan.unknown);

    let found: BTreeSet<(String, String)> =
        scan.geometry.iter().map(|(v, f, _)| (v.clone(), f.clone())).collect();
    let expected: BTreeSet<(String, String)> = [
        ("Package".to_string(), "name_span".to_string()),
        ("Class".to_string(), "name_span".to_string()),
    ]
    .into_iter()
    .collect();

    assert_eq!(
        found, expected,
        "a char literal carrying a quote or a brace must not desync the scanner, and a \
         lifetime must not be mistaken for one; either failure shrinks the denominator"
    );
}

/// A block comment must not truncate the scan behind the `>= 9` floor.
///
/// Raised in review as the sharper form of the doc-brace problem, and the
/// framing was the useful part: the floor counts fields *found*, so a truncation
/// occurring after the ninth field is invisible to it. This models exactly that
/// — nine geometry fields, then a block comment carrying an unbalanced `}`, then
/// a tenth field that must still be seen.
///
/// Block comments also nest in Rust, so the stripper tracks depth rather than
/// scanning for the first `*/`.
#[test]
fn a_block_comment_cannot_truncate_the_scan_after_the_ninth_field() {
    let mut synthetic = String::from("pub enum NodeKind {\n");
    for n in 0..9 {
        synthetic.push_str(&format!("    Variant{n} {{ span{n}: SourceLocation }},\n"));
    }
    synthetic.push_str(
        "    /** Prose with an unbalanced close brace } and a nested /* inner } */ part */\n",
    );
    synthetic.push_str("    Tenth { late_span: SourceLocation },\n");
    synthetic.push_str("}\n");

    let scan = scan_declared_fields(&synthetic);
    assert!(scan.unknown.is_empty(), "synthetic source must classify cleanly: {:?}", scan.unknown);

    let names: BTreeSet<String> = scan.geometry.iter().map(|(v, _, _)| v.clone()).collect();
    assert!(
        names.contains("Tenth"),
        "a block comment must not truncate the scan; the field after it was lost. Found: {names:?}"
    );
    assert_eq!(
        scan.geometry.len(),
        10,
        "all ten declared geometry fields must survive comment stripping, found {:?}",
        scan.geometry
    );
}

/// An unbalanced brace in doc prose must not move the enum boundary.
///
/// Raised in review: the scan balanced braces over raw source and stripped
/// comments afterwards, so documentation braces counted. The prose in this enum
/// carries 26 brace-bearing comment lines and happens to net to zero today, which
/// made the guard correct by luck. A lone `${` or `}` in a doc comment could
/// truncate the scan (caught by the `>= 9` floor) or extend it past the enum's
/// end (not caught by anything).
#[test]
fn doc_comment_braces_do_not_move_the_enum_boundary() {
    // Every brace here is unbalanced and lives only in prose.
    let synthetic = r#"
        pub enum NodeKind {
            /// Interpolation looks like ${ and closes elsewhere }
            /// A stray close brace: }
            Package {
                /// Deref syntax: @{
                name: String,
                name_span: SourceLocation,
            },
            /// Trailing prose with an opener {
            Number {
                value: String,
            },
        }

        pub struct NotPartOfTheEnum {
            decoy_span: SourceLocation,
        }
    "#;

    let scan = scan_declared_fields(synthetic);
    assert!(scan.unknown.is_empty(), "synthetic source must classify cleanly: {:?}", scan.unknown);

    // Truncation would lose Package.name_span; over-extension would pull in
    // `decoy_span` from the struct that follows the enum.
    let found: BTreeSet<(String, String)> =
        scan.geometry.iter().map(|(v, f, _)| (v.clone(), f.clone())).collect();
    assert_eq!(
        found,
        [("Package".to_string(), "name_span".to_string())].into_iter().collect::<BTreeSet<_>>(),
        "doc-comment braces must neither truncate the enum body nor extend it past its end"
    );
}

/// A second span inside an already-registered nested field must be demanded.
///
/// Raised in review, and it was a real hole: the coverage check compared
/// *sets* of (variant, outer field). Extending `Try.catch_blocks` with another
/// `SourceLocation` left the declared set and the registered set identical, so
/// nothing required a second row. Exhaustive destructuring does not catch it
/// either, because widening a tuple adds no new field name to bind.
///
/// This is the fourth escape of the same family found on this PR, so it is
/// pinned against the scanner itself rather than argued about: the synthetic
/// source below declares two spans in one nested field while the registry
/// offers one row.
#[test]
#[should_panic(expected = "declare a different number of geometry members")]
fn a_second_span_inside_one_nested_field_is_demanded() {
    // Two independent SourceLocations inside a single declared field.
    let synthetic = r#"
        pub enum NodeKind {
            Try {
                body: Box<Node>,
                catch_blocks: Vec<(Option<(String, SourceLocation)>, Option<SourceLocation>, Box<Node>)>,
                finally_block: Option<Box<Node>>,
            },
        }
    "#;

    let scan = scan_declared_fields(synthetic);
    assert!(scan.unknown.is_empty(), "synthetic source must classify cleanly: {:?}", scan.unknown);

    // Exactly the registry this PR ships for that field: one dotted row.
    let one_row = [AstGeometryField {
        kind_name: "Try",
        field: "catch_blocks.variable",
        shape: AstGeometryShape::Nested,
        mapping: AstGeometryMapping::MapRange,
        payload_role: None,
        disposition: AstGeometryDisposition::SourceExact,
    }];

    // First, demonstrate the escape rather than only the fix. The previous guard
    // compared these two *sets*, and here they are equal — so it passed while a
    // second span sat unregistered. The count comparison below is what fails.
    let declared_bases: BTreeSet<(String, String)> =
        scan.geometry.iter().map(|(variant, field, _)| (variant.clone(), field.clone())).collect();
    let registered_bases: BTreeSet<(String, String)> = one_row
        .iter()
        .map(|row| {
            let base = row.field.split('.').next().unwrap_or(row.field);
            (row.kind_name.to_string(), base.to_string())
        })
        .collect();
    assert_eq!(
        declared_bases, registered_bases,
        "the outer-field sets must match here; if they did not, this control would be proving \
         something easier than the reported escape"
    );

    compare_declared_against_registry(&scan.geometry, &one_row);
}

/// The same comparison must accept the honest case, or it proves nothing.
///
/// Opposite-direction control for the test above: one declared span and one
/// registered row is not drift.
#[test]
fn one_nested_span_with_one_row_is_accepted() {
    let synthetic = r#"
        pub enum NodeKind {
            Try {
                body: Box<Node>,
                catch_blocks: Vec<(Option<(String, SourceLocation)>, Box<Node>)>,
                finally_block: Option<Box<Node>>,
            },
        }
    "#;

    let scan = scan_declared_fields(synthetic);
    assert!(scan.unknown.is_empty(), "synthetic source must classify cleanly: {:?}", scan.unknown);

    let one_row = [AstGeometryField {
        kind_name: "Try",
        field: "catch_blocks.variable",
        shape: AstGeometryShape::Nested,
        mapping: AstGeometryMapping::MapRange,
        payload_role: None,
        disposition: AstGeometryDisposition::SourceExact,
    }];

    compare_declared_against_registry(&scan.geometry, &one_row);
}

/// Every registered geometry occurrence must actually reach the coordinate mapper.
///
/// This is the seam the whole registry exists to serve, and until `main` gained
/// `Node::clone_with_mapped_locations` there was nothing to connect it to. There
/// is now, and review pointed out that registration alone proves nothing about
/// it: every arm of `map_payload_locations_with_recovery` matches with `..`, so
/// a field can be forced into this registry by its declared type, forced into
/// `observe_geometry_fields` by exhaustive destructuring, validated in
/// production — and still be silently skipped by the mapper, leaving a mapped
/// clone holding stale offsets. That is precisely the failure this registry is
/// supposed to make impossible.
///
/// The proof does not need the mapper to expose anything. The mapping closure is
/// called once per location the mapper actually touches, so counting calls
/// measures what it really did rather than what it claims:
///
/// ```text
/// map calls == one per node location + one per registered geometry occurrence
/// ```
///
/// A field the mapper ignores makes the count fall short, and the failure names
/// the variant. This is a counting argument, not a value comparison, so it stays
/// true for any mapping function and does not need the observer to surface span
/// values.
#[test]
fn every_registered_geometry_occurrence_reaches_the_mapper()
-> Result<(), Box<dyn std::error::Error>> {
    for fixture in node_kind_fixtures() {
        let sample = &fixture.sample;
        let kind_name = sample.kind.kind_name();
        if geometry_fields_for(kind_name).is_empty() {
            continue;
        }

        // Sum geometry over the whole tree, not just the root: a fixture may
        // nest a geometry-bearing child, and counting only the root would make
        // this test fail for the wrong reason if one ever does.
        let mut occurrences = 0usize;
        let mut stack = vec![sample];
        while let Some(node) = stack.pop() {
            occurrences +=
                observe_geometry_fields(&node.kind).iter().map(|e| e.occurrences).sum::<usize>();
            node.for_each_child_with_field(|_, child| stack.push(child));
        }

        let calls = std::cell::Cell::new(0usize);
        let mapped = sample.clone_with_mapped_locations(|location| {
            calls.set(calls.get() + 1);
            location
        });
        let mapped = mapped.ok_or_else(|| {
            format!("{kind_name}: identity mapping must produce a clone, got None")
        })?;
        assert_eq!(mapped.kind.kind_name(), kind_name, "{kind_name}: mapped clone changed kind");

        let expected = sample.count_nodes() + occurrences;
        assert_eq!(
            calls.get(),
            expected,
            "{kind_name}: the mapper touched {} locations but the registry accounts for {} \
             ({} node locations + {occurrences} registered geometry occurrences). A shortfall \
             means a registered geometry field is not being remapped, so a mapped clone keeps \
             stale offsets for it.",
            calls.get(),
            expected,
            sample.count_nodes()
        );
    }
    Ok(())
}

/// Registry coherence is checked by production code, not only by this suite.
#[test]
fn the_canonical_registry_validates() -> Result<(), Box<dyn std::error::Error>> {
    perl_ast::validate_geometry_registry()?;
    Ok(())
}

/// Textual tripwire: the observer must not silence its own compile error with a
/// rest pattern.
///
/// Adding a geometry field to an existing variant fails to compile inside
/// `observe_geometry_fields` — that is the point of listing every field. An
/// author under time pressure can make that error disappear by writing `..`
/// instead of classifying the field, restoring exactly the drift this registry
/// exists to prevent.
///
/// **What this is not.** This is a scan over source text, not a compile-time
/// check and not a lint. It cannot see the parse tree, so it recognises a rest
/// pattern only by shape: a `..` whose next non-whitespace character is `}`,
/// searched across the whole body so a pattern split over several lines is
/// still caught. That deliberately excludes ordinary `..` uses — `0..n`,
/// `&chunks[0..]`, `a..=b`, `Foo { ..base }` — which an earlier version of this
/// test would have failed on for the wrong reason. It remains a shape match, so
/// a sufficiently unusual spelling could still slip past it.
///
/// So do not read a green result here as proof that the observer is exhaustive.
/// The load-bearing guard is
/// `the_registry_covers_every_geometry_bearing_field_declared_in_the_enum`,
/// which reads declared field types and does not depend on how the observer is
/// written at all. This test is a cheap early warning that fires closer to the
/// mistake; a rustc-side lint would be the precise version of it.
#[test]
fn the_observer_never_uses_a_rest_pattern() -> Result<(), Box<dyn std::error::Error>> {
    const SOURCE: &str = include_str!("../src/geometry_policy.rs");

    let (_, after_signature) = SOURCE
        .split_once("pub fn observe_geometry_fields")
        .ok_or("observe_geometry_fields must exist; it is the observation authority")?;

    // Bound the scan to the function body so an unrelated later item cannot fail
    // this guard. The body ends at the first line closing a top-level item.
    let body: Vec<&str> =
        after_signature.lines().take_while(|line| !line.starts_with('}')).collect();
    assert!(!body.is_empty(), "the observer body must be scannable");

    // Join the comment-stripped body and scan it whole rather than line by line.
    // A rest pattern may be split across lines by formatting:
    //
    //     NodeKind::Identifier {
    //         ..
    //     } => NONE,
    //
    // which a per-line scan never sees, because the `..` line has no `}` on it.
    let code: String = body
        .iter()
        .map(|line| line.split("//").next().unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");

    // A rest pattern closes its struct pattern: `{ name: _, .. }`. A range or
    // slice never has `}` as its next non-whitespace character, across lines or
    // otherwise.
    if let Some((at, _)) =
        code.match_indices("..").find(|(at, _)| code[at + 2..].trim_start().starts_with('}'))
    {
        let line_number = code[..at].matches('\n').count() + 1;
        // Report the whole line holding the `..`, not just the text before it.
        let line_start = code[..at].rfind('\n').map_or(0, |index| index + 1);
        let line_end = code[at..].find('\n').map_or(code.len(), |index| at + index);
        return Err(format!(
            "observe_geometry_fields must destructure every field explicitly, but line \
             {line_number} uses a rest pattern: {}\nA `..` here would let a new geometry-bearing \
             field compile without being classified.",
            code[line_start..line_end].trim()
        )
        .into());
    }
    Ok(())
}

/// A node that carries no geometry reconciles to an empty set, not a default.
#[test]
fn geometry_free_variants_are_explicitly_empty() {
    let loc = SourceLocation { start: 0, end: 1 };
    let number = NodeKind::Number { value: "1".to_string() };
    assert!(
        observe_geometry_fields(&number).is_empty(),
        "Number carries no independent payload geometry"
    );
    assert!(geometry_fields_for("Number").is_empty(), "Number must have no geometry rows");
    assert!(reconcile_node_geometry(&perl_ast::Node::new(number, loc)).is_ok());

    assert!(
        geometry_fields_for("FutureUnregisteredNode").is_empty(),
        "an unknown kind must fail closed with no rows rather than a permissive default"
    );
}

// ---------------------------------------------------------------------------
// Negative controls.
//
// These feed the checker deliberately wrong inputs. If any of them returned
// `Ok`, the positive gate above would be decorative.
// ---------------------------------------------------------------------------

/// The discriminating mutation named by #13234: geometry added to an existing
/// variant without a registry row.
#[test]
fn unregistered_geometry_on_an_existing_variant_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    // Stands in for `Subroutine { .., return_type_span: Option<SourceLocation> }`
    // being added to the enum: the field is observed, nothing registers it.
    let observed = vec![
        ObservedGeometryField {
            field: "name_span",
            shape: AstGeometryShape::Optional,
            occurrences: 1,
        },
        ObservedGeometryField {
            field: "return_type_span",
            shape: AstGeometryShape::Optional,
            occurrences: 1,
        },
    ];

    let Err(drift) = reconcile_geometry_rows("Subroutine", AST_NODE_GEOMETRY_FIELDS, &observed)
    else {
        return Err("an unregistered geometry field must fail the gate".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::UnregisteredField {
            kind_name: "Subroutine".to_string(),
            field: "return_type_span".to_string(),
        }
    );
    assert!(
        drift.to_string().contains("return_type_span"),
        "the failure must name the responsible field: {drift}"
    );
    Ok(())
}

/// Adding a *variant* is not the discriminating mutation, but adding geometry
/// to a previously geometry-free variant is.
#[test]
fn geometry_added_to_a_previously_geometry_free_variant_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let observed = vec![ObservedGeometryField {
        field: "value_span",
        shape: AstGeometryShape::Direct,
        occurrences: 1,
    }];

    let Err(drift) = reconcile_geometry_rows("Number", AST_NODE_GEOMETRY_FIELDS, &observed) else {
        return Err("a geometry-free variant that gains a span must fail the gate".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::UnregisteredField {
            kind_name: "Number".to_string(),
            field: "value_span".to_string(),
        }
    );
    Ok(())
}

/// A row that outlives the field it names must fail rather than silently
/// describing geometry nobody carries.
#[test]
fn a_stale_registry_row_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let Err(drift) = reconcile_geometry_rows("Package", AST_NODE_GEOMETRY_FIELDS, &[]) else {
        return Err("a registered field that is never observed must fail the gate".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::StaleRow {
            kind_name: "Package".to_string(),
            field: "name_span".to_string(),
        }
    );
    Ok(())
}

/// A registry row that misdescribes the shape must fail: a consumer would
/// otherwise map an optional field as if it were always present.
#[test]
fn a_shape_mismatch_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let observed = vec![ObservedGeometryField {
        field: "name_span",
        shape: AstGeometryShape::Direct,
        occurrences: 1,
    }];

    let Err(drift) = reconcile_geometry_rows("Subroutine", AST_NODE_GEOMETRY_FIELDS, &observed)
    else {
        return Err("a registered shape that disagrees with observation must fail the gate".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::ShapeMismatch {
            kind_name: "Subroutine".to_string(),
            field: "name_span".to_string(),
            registered: AstGeometryShape::Optional,
            observed: AstGeometryShape::Direct,
        }
    );
    Ok(())
}

/// Classifying a token as a freely resizable range would let a remap invent
/// bytes that the token's recorded width does not have.
#[test]
fn a_token_registered_as_a_resizable_range_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let mutated = [AstGeometryField {
        kind_name: "Error",
        field: "found",
        shape: AstGeometryShape::Token,
        mapping: AstGeometryMapping::MapRange,
        payload_role: Some(perl_ast::AstPayloadPolicy::RecoverySynthetic),
        disposition: AstGeometryDisposition::Recovery,
    }];

    let observed = vec![ObservedGeometryField {
        field: "found",
        shape: AstGeometryShape::Token,
        occurrences: 1,
    }];

    let Err(drift) = reconcile_geometry_rows("Error", &mutated, &observed) else {
        return Err("a token must not be registered as a freely resizable range".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::TokenIsNotResizable {
            kind_name: "Error".to_string(),
            field: "found".to_string(),
            mapping: AstGeometryMapping::MapRange,
        }
    );
    Ok(())
}

/// The inverse: an ordinary span may not claim the token width rule.
#[test]
fn a_non_token_claiming_width_preservation_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let mutated = [AstGeometryField {
        kind_name: "Package",
        field: "name_span",
        shape: AstGeometryShape::Direct,
        mapping: AstGeometryMapping::MapStartPreserveWidth,
        payload_role: Some(perl_ast::AstPayloadPolicy::DeclarationNameAnchor),
        disposition: AstGeometryDisposition::SourceExact,
    }];

    let observed = vec![ObservedGeometryField {
        field: "name_span",
        shape: AstGeometryShape::Direct,
        occurrences: 1,
    }];

    let Err(drift) = reconcile_geometry_rows("Package", &mutated, &observed) else {
        return Err("only a token may claim the width-preserving mapping rule".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::WidthPreservationRequiresToken {
            kind_name: "Package".to_string(),
            field: "name_span".to_string(),
            shape: AstGeometryShape::Direct,
        }
    );
    Ok(())
}

/// A payload row may not claim the caller-owned boundary rule.
///
/// `CallerOwnedBoundary` is reserved for anchoring decisions the AST does not
/// own. A payload span claiming it would let a mapped-clone consumer legitimately
/// skip a real span while the coherence gate still returned `Ok`.
#[test]
fn a_payload_row_claiming_caller_owned_boundary_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let mutated = [AstGeometryField {
        kind_name: "Package",
        field: "name_span",
        shape: AstGeometryShape::Direct,
        mapping: AstGeometryMapping::CallerOwnedBoundary,
        payload_role: Some(perl_ast::AstPayloadPolicy::DeclarationNameAnchor),
        disposition: AstGeometryDisposition::SourceExact,
    }];

    let observed = vec![ObservedGeometryField {
        field: "name_span",
        shape: AstGeometryShape::Direct,
        occurrences: 1,
    }];

    let Err(drift) = reconcile_geometry_rows("Package", &mutated, &observed) else {
        return Err("a payload row must not claim the caller-owned boundary mapping".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::CallerOwnedMappingOnPayloadRow {
            kind_name: "Package".to_string(),
            field: "name_span".to_string(),
        }
    );
    Ok(())
}

/// A row whose disposition contradicts its owning variant must be rejected by
/// production code, not merely noticed by a test that reads both tables.
#[test]
fn a_wrong_disposition_is_rejected_by_the_registry_validator() {
    // Error is Recovery-classified, so recovery is the only coherent disposition.
    let policy = ast_node_policy("Error");
    assert!(policy.is_some(), "Error must have a policy row");

    let required = geometry_disposition_for_role(None, AstNodeClassification::Recovery);
    assert_eq!(
        required,
        AstGeometryDisposition::Recovery,
        "a recovery variant's geometry must be recovery-dispositioned"
    );
    assert_ne!(
        required,
        AstGeometryDisposition::SourceExact,
        "source-exact must not satisfy a recovery variant, or the derivation proves nothing"
    );

    // Recovery is a floor: no field role can escape it. Without this, adding the
    // role dimension would have opened a way to launder recovery geometry into
    // exact geometry by naming a declaration-name role.
    assert_eq!(
        geometry_disposition_for_role(
            Some(perl_ast::AstPayloadPolicy::DeclarationNameAnchor),
            AstNodeClassification::Recovery
        ),
        AstGeometryDisposition::Recovery,
        "the recovery classification must outrank any field role"
    );

    // A source-boundary variant must not be satisfied by the source-exact default.
    assert_eq!(
        geometry_disposition_for_role(None, AstNodeClassification::SourceBoundary),
        AstGeometryDisposition::SourceBoundary
    );

    // But an opaque region on that same classification stays boundary, while a
    // declaration name on it is exact -- the distinction this dimension exists for.
    assert_eq!(
        geometry_disposition_for_role(
            Some(perl_ast::AstPayloadPolicy::OpaqueSourceRegion),
            AstNodeClassification::SourceBoundary
        ),
        AstGeometryDisposition::SourceBoundary
    );
    assert_eq!(
        geometry_disposition_for_role(
            Some(perl_ast::AstPayloadPolicy::DeclarationNameAnchor),
            AstNodeClassification::SourceBoundary
        ),
        AstGeometryDisposition::SourceExact
    );
}

/// Two rows for the same field would give a consumer two mapping rules.
#[test]
fn a_duplicate_registry_row_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let mutated = [
        AstGeometryField {
            kind_name: "Package",
            field: "name_span",
            shape: AstGeometryShape::Direct,
            mapping: AstGeometryMapping::MapRange,
            payload_role: Some(perl_ast::AstPayloadPolicy::DeclarationNameAnchor),
            disposition: AstGeometryDisposition::SourceExact,
        },
        AstGeometryField {
            kind_name: "Package",
            field: "name_span",
            shape: AstGeometryShape::Optional,
            mapping: AstGeometryMapping::MapRange,
            payload_role: Some(perl_ast::AstPayloadPolicy::DeclarationNameAnchor),
            disposition: AstGeometryDisposition::SourceExact,
        },
    ];

    let Err(drift) = reconcile_geometry_rows("Package", &mutated, &[]) else {
        return Err("a duplicated field row must fail the gate".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::DuplicateRow {
            kind_name: "Package".to_string(),
            field: "name_span".to_string(),
        }
    );
    Ok(())
}

/// Two dotted rows sharing a base but spelling the suffix differently are drift.
///
/// Raised in review: geometry rows use dotted identities (`catch_blocks.variable`)
/// while every other governance bucket uses top-level field names, so the
/// bucket-exclusivity check in `ast_invariant_policy_registry.rs` compares
/// `catch_blocks.variable` against a bare `catch_blocks` and finds no collision.
/// The concern was that duplicate ownership of one base could therefore pass
/// unnoticed.
///
/// It does not, but the protection is *incidental* rather than stated: the
/// observer emits exactly one identity per nested record, so a second row on the
/// same base has nothing observing it and reconciliation rejects it as a stale
/// row. `DuplicateRow` does not fire here — the two strings differ — which is
/// precisely why this deserves its own control instead of being assumed to fall
/// out of the duplicate check.
///
/// Pinning it means a future change that makes reconciliation tolerant of
/// unobserved rows fails here, naming the reason, rather than silently reopening
/// the gap.
#[test]
fn a_second_dotted_row_on_the_same_base_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let mutated = [
        AstGeometryField {
            kind_name: "Try",
            field: "catch_blocks.variable",
            shape: AstGeometryShape::Nested,
            mapping: AstGeometryMapping::MapRange,
            payload_role: None,
            disposition: AstGeometryDisposition::SourceExact,
        },
        AstGeometryField {
            kind_name: "Try",
            field: "catch_blocks.var",
            shape: AstGeometryShape::Nested,
            mapping: AstGeometryMapping::MapRange,
            payload_role: None,
            disposition: AstGeometryDisposition::SourceExact,
        },
    ];

    // What the observer actually reports for a Try bearing one bound variable.
    let observed = [ObservedGeometryField {
        field: "catch_blocks.variable",
        shape: AstGeometryShape::Nested,
        occurrences: 1,
    }];

    let Err(drift) = reconcile_geometry_rows("Try", &mutated, &observed) else {
        return Err("a second dotted row on the same base must fail the gate".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::StaleRow {
            kind_name: "Try".to_string(),
            field: "catch_blocks.var".to_string(),
        },
        "the misspelled suffix must be named, so the author sees which spelling is unbacked"
    );
    Ok(())
}

/// An absent optional span is legitimate and must not be reported as drift.
///
/// This is the opposite-direction control for the unregistered-field test: the
/// gate must distinguish "this field is missing from the registry" from "this
/// instance simply has no value here".
#[test]
fn an_absent_optional_span_is_not_drift() -> Result<(), Box<dyn std::error::Error>> {
    let loc = SourceLocation { start: 0, end: 4 };
    let without_span = perl_ast::Node::new(
        NodeKind::Subroutine {
            name: Some("f".to_string()),
            name_span: None,
            declarator: Some("sub".to_string()),
            prototype: None,
            signature: None,
            attributes: vec![],
            body: Box::new(perl_ast::Node::new(NodeKind::Block { statements: vec![] }, loc)),
        },
        loc,
    );

    let observed = observe_geometry_fields(&without_span.kind);
    assert_eq!(observed.len(), 1, "the field is still registered even when absent");
    assert_eq!(observed[0].occurrences, 0, "an absent optional span observes zero spans");

    reconcile_node_geometry(&without_span)?;
    Ok(())
}

/// The recovery token observes its real width, which is what the width-
/// preserving mapping rule protects.
#[test]
fn a_recovery_token_carries_its_own_validated_width() -> Result<(), Box<dyn std::error::Error>> {
    let loc = SourceLocation { start: 10, end: 13 };
    let token = Token::new_checked(TokenKind::Unknown, "abc", 10, 13)?;
    assert_eq!(token.text.len(), 3, "token text length is its byte width");

    let node = perl_ast::Node::new(
        NodeKind::Error {
            message: "unexpected".to_string(),
            expected: vec![TokenKind::Eof],
            found: Some(token),
            partial: None,
        },
        loc,
    );

    let observed = observe_geometry_fields(&node.kind);
    assert_eq!(
        observed,
        vec![ObservedGeometryField {
            field: "found",
            shape: AstGeometryShape::Token,
            occurrences: 1,
        }]
    );
    reconcile_node_geometry(&node)?;
    Ok(())
}

/// Nested geometry counts real elements, not declared cardinality.
#[test]
fn nested_catch_variable_geometry_counts_actual_elements() -> Result<(), Box<dyn std::error::Error>>
{
    let loc = SourceLocation { start: 0, end: 1 };
    let block = || Box::new(perl_ast::Node::new(NodeKind::Block { statements: vec![] }, loc));

    let node = perl_ast::Node::new(
        NodeKind::Try {
            body: block(),
            catch_blocks: vec![
                (Some(("$e".to_string(), loc)), block()),
                (None, block()),
                (Some(("$f".to_string(), loc)), block()),
            ],
            finally_block: None,
        },
        loc,
    );

    let observed = observe_geometry_fields(&node.kind);
    assert_eq!(
        observed,
        vec![ObservedGeometryField {
            field: "catch_blocks.variable",
            shape: AstGeometryShape::Nested,
            occurrences: 2,
        }],
        "only catch blocks that actually bind a variable carry a span"
    );
    reconcile_node_geometry(&node)?;
    Ok(())
}
