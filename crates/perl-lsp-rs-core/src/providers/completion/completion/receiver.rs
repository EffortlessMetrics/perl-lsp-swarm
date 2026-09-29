//! Receiver evidence classification for method completion.
//!
//! Owns the typed provenance used by workspace method completion: source-backed
//! receiver facts, type-engine packages, and text-pattern constructor/bless
//! forms. Defining-class invocants (`$self`, `$this`, `$class`) reach this
//! layer only through canonical value-shape facts, not spelling.

use super::context::CompletionContext;
#[cfg(test)]
use perl_lexer::{PerlLexer, TokenType};
use perl_semantic_analyzer::{
    Node, NodeKind, Parser,
    analysis::value_shape_inferrer::ValueShapeInferrer,
    receiver_facts::{
        ReceiverFact, ReceiverFactContext, ReceiverFactFreshness, ReceiverFallbackState,
        ReceiverKind, receiver_fact_for_method_call,
    },
    symbol::SymbolTable,
    type_facts::TypeEvidence,
    type_inference::{PerlType, TypeInferenceEngine},
};
use perl_semantic_facts::{Confidence, ValueShape};

/// Package for a defining-class invocant at the completion position.
///
/// Consumes existing [`ValueShapeInferrer`] invocant facts at `context.position`.
/// Spelling of `$self` / `$this` / `$class` is not enough; file-level and `main`
/// sites stay unknown. Constructor/bless text patterns are classified first so
/// a reassigned `$self = Other->new` is not overwritten here.
fn defining_class_invocant_receiver(context: &CompletionContext, source: &str) -> Option<String> {
    let var_name = context.receiver_prefix().trim_end_matches("->").strip_prefix('$')?;
    if !ValueShapeInferrer::is_defining_class_invocant_name(var_name) {
        return None;
    }
    let mut parser = Parser::new(source);
    let ast = parser.parse().ok()?;
    let shapes = ValueShapeInferrer::named_invocant_shapes_at(&ast, context.position);
    match shapes.get(var_name) {
        Some(ValueShape::Object { package, .. }) if !package.is_empty() && package != "main" => {
            Some(package.clone())
        }
        _ => None,
    }
}

/// Classification of how a method-completion receiver was inferred at the
/// call site.
///
/// This is *typed receiver-evidence provenance*: source-backed exact facts may
/// drive the narrow semantic method-completion pilot, while weaker evidence
/// keeps the existing fallback path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ReceiverEvidence {
    /// Literal package name on the left of `->`, e.g. `Foo->method` or
    /// `Foo::Bar->method`. High confidence.
    StaticPackage(String),
    /// Defining-class invocant that met the exact source-backed SelfReceiver
    /// cutover bar. High confidence. Spelling of `$self` / `$this` is not enough.
    SelfOrThis(String),
    /// Variable-method call against `$x` assigned earlier in the source as
    /// `my $x = Foo->new(...)`. The constructor convention pins the
    /// receiver to `Foo`. High confidence.
    ConstructorAssignment(String),
    /// Variable-method call against `$x` assigned earlier as
    /// `my $x = bless ..., "Foo"`. Literal `bless` with a literal class.
    /// Medium confidence — strong static evidence, still a Perl runtime
    /// construct.
    LiteralBless(String),
    /// Receiver type was resolved by [`TypeInferenceEngine`] for a `$var`
    /// call site. Medium confidence — confidence ultimately follows the
    /// engine's source, but at this layer we treat all engine results as
    /// medium.
    TypeEngine(String),
    /// Receiver was resolved by the semantic receiver-fact layer and met the
    /// exact/fresh/high-confidence provider cutover bar.
    ObjectFact(String),
    /// Static hash slot, e.g. `$services{db}->`, resolved through a fresh
    /// source-backed receiver fact.
    HashSlotFact(String),
    /// Receiver resolved to two or more distinct candidate packages from a
    /// union-typed variable (e.g. `$obj : Foo | Bar`).  All candidate
    /// packages are exposed so completion can offer methods from every arm.
    ///
    /// The primary `package` in the underlying [`ReceiverFact`] is the first
    /// union arm; `candidate_packages` carries the full ordered set.
    ///
    /// Confidence is high for all arms (union inference is source-backed);
    /// fallback state is `Fallback` because the exact package cannot be
    /// narrowed to a single type at the call site.
    UnionCandidates(Vec<String>),
    /// No receiver evidence found, OR a positively-detected dynamic form
    /// (e.g. `bless {}, $class`, expression-tail class, nested call,
    /// Positively-detected dynamic / fail-closed receiver form — e.g.
    /// `bless {}, $class`, `bless {}, "Foo" . $suffix`,
    /// `wrapper(bless {}, "Foo")`, `bless::class {}, "Foo"`. The user
    /// typed something we *can* see is a `bless` expression but the
    /// resulting class is not a literal we can pin down. Method
    /// completion stays fail-closed for these — no exact receiver
    /// completions and no Unknown-receiver fallback (#7929 outcome A).
    Dynamic,
    /// No receiver evidence found at all (e.g. `$obj->` where `$obj` is
    /// a sub parameter with no assignment in scope). Eligible for the
    /// bounded low-confidence fallback added in #7929 outcome A.
    Unknown,
}

impl ReceiverEvidence {
    /// Returns the inferred receiver package, if any. `Dynamic`, `Unknown`,
    /// and `UnionCandidates` all return `None` — use
    /// [`candidate_packages`](Self::candidate_packages) for union receivers.
    pub(super) fn package(&self) -> Option<&str> {
        match self {
            Self::StaticPackage(p)
            | Self::SelfOrThis(p)
            | Self::ConstructorAssignment(p)
            | Self::LiteralBless(p)
            | Self::TypeEngine(p)
            | Self::ObjectFact(p)
            | Self::HashSlotFact(p) => Some(p.as_str()),
            Self::UnionCandidates(_) | Self::Dynamic | Self::Unknown => None,
        }
    }

    /// Returns the full list of candidate packages for union receivers.
    ///
    /// For `UnionCandidates` returns all packages in declaration order.
    /// For all other variants (including `StaticPackage`, `ObjectFact`,
    /// `Dynamic`, and `Unknown`) returns an empty slice.
    /// Use [`package`](Self::package) to get the single package for
    /// non-union evidence.
    pub(super) fn candidate_packages(&self) -> &[String] {
        match self {
            Self::UnionCandidates(packages) => packages.as_slice(),
            _ => &[],
        }
    }

    /// Returns `true` only when this evidence is eligible for the
    /// bounded Unknown-receiver fallback (#7929). `Dynamic` is
    /// explicitly *not* eligible — dynamic boundaries stay fail-closed.
    pub(super) fn is_unknown_fallback_eligible(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    /// Returns the confidence level for this evidence kind, using the
    /// shared `perl_semantic_facts::Confidence` vocabulary so the rest of
    /// the semantic stack speaks the same language. `Dynamic`, `Unknown`,
    /// and `UnionCandidates` return `None` — there is no single-package
    /// confidence for a multi-candidate receiver.
    ///
    /// Today the production method-completion callsite reads this only
    /// to decide medium-confidence labelling on detail text (#7925
    /// outcome C). It does not yet drive ranking.
    #[allow(dead_code)]
    pub(super) fn confidence(&self) -> Option<Confidence> {
        match self {
            Self::StaticPackage(_) | Self::SelfOrThis(_) | Self::ConstructorAssignment(_) => {
                Some(Confidence::High)
            }
            Self::ObjectFact(_) | Self::HashSlotFact(_) => Some(Confidence::High),
            Self::LiteralBless(_) | Self::TypeEngine(_) => Some(Confidence::Medium),
            Self::UnionCandidates(_) | Self::Dynamic | Self::Unknown => None,
        }
    }

    /// Short, user-facing suffix describing the evidence source, suitable
    /// for appending to a `CompletionItem.detail` string. `Dynamic`,
    /// `Unknown`, and `UnionCandidates` return `None` — when there is no
    /// exact evidence, there is nothing to label. Issue #7918: explanatory
    /// only, no ranking / inclusion change.
    pub(super) fn detail_suffix(&self) -> Option<&'static str> {
        match self {
            Self::StaticPackage(_) => Some("receiver: static package"),
            Self::SelfOrThis(_) => Some("receiver: self/this"),
            Self::ConstructorAssignment(_) => Some("receiver: constructor assignment"),
            Self::LiteralBless(_) => Some("receiver: literal bless"),
            Self::TypeEngine(_) => Some("receiver: type engine"),
            Self::ObjectFact(_) => Some("receiver: source-backed object"),
            Self::HashSlotFact(_) => Some("receiver: hash slot"),
            Self::UnionCandidates(_) => Some("receiver: union candidates"),
            Self::Dynamic | Self::Unknown => None,
        }
    }
}

/// Apply the receiver-evidence detail suffix to an existing base detail
/// string. Returns the unchanged base when the evidence carries no suffix
/// (e.g. `Unknown`). Issue #7918.
pub(super) fn detail_with_evidence(base: String, evidence: &ReceiverEvidence) -> String {
    let Some(suffix) = evidence.detail_suffix() else {
        return base;
    };
    // Outcome C from #7925: append `, medium confidence` only to medium-
    // confidence evidence. High-confidence evidence (the common case)
    // stays clean. `Unknown` already returns `None` from `detail_suffix`
    // and was handled by the early return above.
    match evidence.confidence() {
        Some(Confidence::Medium) => format!("{base} — {suffix}, medium confidence"),
        _ => format!("{base} — {suffix}"),
    }
}

/// Classify the receiver of a `->` method-completion call site.
///
/// Exact source-backed facts win, then constructor/bless/static text patterns,
/// then position-local defining-class invocant shapes, then type-engine packages.
/// `$self` / `$this` / `$class` spelling is not itself receiver evidence.
#[cfg(test)]
pub(super) fn classify_receiver(
    context: &CompletionContext,
    source: &str,
    type_engine: Option<&TypeInferenceEngine>,
) -> ReceiverEvidence {
    classify_receiver_with_symbol_table(context, source, type_engine, None)
}

pub(super) fn classify_receiver_with_symbol_table(
    context: &CompletionContext,
    source: &str,
    type_engine: Option<&TypeInferenceEngine>,
    symbol_table: Option<&SymbolTable>,
) -> ReceiverEvidence {
    if let Some(evidence) = source_backed_receiver_fact_evidence(context, source, type_engine) {
        return evidence;
    }
    let text = classify_text_pattern_receiver_with_symbol_table(context, source, symbol_table);
    if !matches!(text, ReceiverEvidence::Unknown) {
        return text;
    }
    if let Some(pkg) = defining_class_invocant_receiver(context, source) {
        return ReceiverEvidence::TypeEngine(pkg);
    }
    if let Some(pkg) = type_engine_receiver(context, type_engine) {
        return ReceiverEvidence::TypeEngine(pkg);
    }
    text
}

fn source_backed_receiver_fact_evidence(
    context: &CompletionContext,
    source: &str,
    type_engine: Option<&TypeInferenceEngine>,
) -> Option<ReceiverEvidence> {
    let receiver_prefix = context.receiver_prefix();
    let receiver_start = if context.prefix.ends_with("->") {
        context.prefix_start
    } else {
        context.prefix_start.checked_sub(receiver_prefix.len())?
    };
    let current_receiver_prefix =
        source.get(receiver_start..receiver_start + receiver_prefix.len())?;
    if current_receiver_prefix != receiver_prefix {
        return None;
    }

    let receiver_source = receiver_prefix.strip_suffix("->")?.trim();
    if receiver_source.is_empty() {
        return None;
    }

    let fact = receiver_fact_for_arrow_receiver(receiver_source, type_engine?)?;
    exact_receiver_fact_evidence(&fact)
}

fn receiver_fact_for_arrow_receiver(
    receiver_source: &str,
    type_engine: &TypeInferenceEngine,
) -> Option<ReceiverFact> {
    const PROBE_METHOD: &str = "__plsp_receiver_probe";
    let probe_source = format!("{receiver_source}->{PROBE_METHOD}();");
    let mut parser = Parser::new(&probe_source);
    let ast = parser.parse().ok()?;
    let call = method_call_named(&ast, PROBE_METHOD)?;
    Some(receiver_fact_for_method_call(
        call,
        ReceiverFactContext::new(Some(type_engine.environment())).with_source(&probe_source),
    ))
}

fn method_call_named<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    if let NodeKind::MethodCall { method, .. } = &node.kind
        && method == name
    {
        return Some(node);
    }

    match &node.kind {
        NodeKind::Program { statements } => {
            statements.iter().find_map(|child| method_call_named(child, name))
        }
        NodeKind::ExpressionStatement { expression } => method_call_named(expression, name),
        NodeKind::VariableDeclaration { initializer, .. } => {
            initializer.as_deref().and_then(|child| method_call_named(child, name))
        }
        NodeKind::Assignment { lhs, rhs, .. } => {
            method_call_named(lhs, name).or_else(|| method_call_named(rhs, name))
        }
        NodeKind::MethodCall { object, args, .. } => method_call_named(object, name)
            .or_else(|| args.iter().find_map(|child| method_call_named(child, name))),
        NodeKind::Binary { left, right, .. } => {
            method_call_named(left, name).or_else(|| method_call_named(right, name))
        }
        NodeKind::ArrayLiteral { elements } => {
            elements.iter().find_map(|child| method_call_named(child, name))
        }
        NodeKind::HashLiteral { pairs } => pairs.iter().find_map(|(key, value)| {
            method_call_named(key, name).or_else(|| method_call_named(value, name))
        }),
        _ => None,
    }
}

fn exact_receiver_fact_evidence(fact: &ReceiverFact) -> Option<ReceiverEvidence> {
    if fact.confidence == Confidence::Medium
        && fact.freshness == ReceiverFactFreshness::Fresh
        && fact.dynamic_boundary.is_none()
        && fact.source_range.is_some()
        && let Some(package) = literal_bless_package_from_fact(fact)
    {
        return Some(ReceiverEvidence::LiteralBless(package));
    }

    // Union receivers: two or more candidate packages from a union-typed variable.
    // These are source-backed and fresh but cannot claim `Exact` fallback state
    // because the call-site type is ambiguous. Route them to `UnionCandidates`
    // so the completion dispatch can offer methods from every arm (#9500).
    if fact.is_union_receiver()
        && fact.freshness == ReceiverFactFreshness::Fresh
        && fact.dynamic_boundary.is_none()
        && fact.source_range.is_some()
        && fact.confidence == Confidence::High
    {
        return Some(ReceiverEvidence::UnionCandidates(fact.candidate_packages.clone()));
    }

    if fact.confidence != Confidence::High
        || fact.freshness != ReceiverFactFreshness::Fresh
        || fact.fallback_state != ReceiverFallbackState::Exact
        || fact.dynamic_boundary.is_some()
        || fact.source_range.is_none()
    {
        return None;
    }

    let package = fact.package.clone()?;
    match fact.kind {
        ReceiverKind::StaticPackage => Some(ReceiverEvidence::StaticPackage(package)),
        ReceiverKind::SelfReceiver => Some(ReceiverEvidence::SelfOrThis(package)),
        ReceiverKind::ObjectVariable => Some(ReceiverEvidence::ObjectFact(package)),
        ReceiverKind::HashSlot => Some(ReceiverEvidence::HashSlotFact(package)),
        ReceiverKind::HashRefSlot
        | ReceiverKind::ArrayIndex
        | ReceiverKind::DynamicKey
        | ReceiverKind::Unknown => None,
        _ => None,
    }
}

fn literal_bless_package_from_fact(fact: &ReceiverFact) -> Option<String> {
    let package = fact.package.as_ref()?;
    fact.evidence.iter().find_map(|evidence| match evidence {
        TypeEvidence::BlessLiteral { package: evidence_package } if evidence_package == package => {
            Some(package.clone())
        }
        _ => None,
    })
}

/// Type-engine arm of [`classify_receiver`]. Extracted from the legacy
/// `infer_receiver_package_from_type_engine` body, behavior preserved.
fn type_engine_receiver(
    context: &CompletionContext,
    type_engine: Option<&TypeInferenceEngine>,
) -> Option<String> {
    let arrow_prefix = context.receiver_prefix().trim_end_matches("->");
    let var_name = arrow_prefix.strip_prefix('$')?;
    let ty = type_engine?.get_type_at(var_name)?;
    match ty {
        PerlType::Object(class) => Some(class),
        PerlType::Reference(inner) => match inner.as_ref() {
            PerlType::Object(class) => Some(class.clone()),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
pub(super) fn receiver_package_from_context_or_source(
    context: &CompletionContext,
    source: &str,
) -> Option<String> {
    if !context.current_package.is_empty() && context.current_package != "main" {
        return Some(context.current_package.clone());
    }

    let position = context.position.min(source.len());
    let mut parser = Parser::new(source);
    if let Ok(ast) = parser.parse() {
        let analyzer =
            perl_semantic_analyzer::semantic::SemanticAnalyzer::analyze_with_source(&ast, source);
        return receiver_package_from_symbol_table_or_source(
            context,
            source,
            analyzer.symbol_table(),
        );
    }

    source_package_fallback(source, position)
}

#[cfg(test)]
pub(super) fn receiver_package_from_symbol_table_or_source(
    context: &CompletionContext,
    source: &str,
    symbol_table: &SymbolTable,
) -> Option<String> {
    if !context.current_package.is_empty() && context.current_package != "main" {
        return Some(context.current_package.clone());
    }

    let position = context.position.min(source.len());
    let current = CompletionContext::detect_current_package(symbol_table, position);
    if current != "main" {
        return Some(current);
    }

    source_package_fallback(source, position)
}

#[cfg(test)]
pub(super) fn source_package_fallback(source: &str, position: usize) -> Option<String> {
    let prefix = source.get(..position)?;
    let mut lexer = PerlLexer::new(prefix);
    let mut current = "main".to_string();
    let mut brace_depth = 0usize;
    let mut package_blocks: Vec<(usize, String)> = Vec::new();
    let mut package_name: Option<String> = None;
    let mut in_package_declaration = false;

    while let Some(token) = lexer.next_token() {
        match &token.token_type {
            TokenType::Keyword(name) if name.as_ref() == "package" => {
                package_name = None;
                in_package_declaration = true;
            }
            TokenType::Identifier(name) if in_package_declaration && package_name.is_none() => {
                package_name = Some(name.to_string());
            }
            TokenType::LeftBrace if in_package_declaration => {
                let Some(package) = package_name.take() else {
                    in_package_declaration = false;
                    brace_depth = brace_depth.saturating_add(1);
                    continue;
                };
                let previous = current.clone();
                current = package;
                brace_depth = brace_depth.saturating_add(1);
                package_blocks.push((brace_depth, previous));
                in_package_declaration = false;
            }
            TokenType::Semicolon if in_package_declaration => {
                if let Some(package) = package_name.take() {
                    current = package;
                }
                in_package_declaration = false;
            }
            TokenType::LeftBrace => {
                brace_depth = brace_depth.saturating_add(1);
            }
            TokenType::RightBrace => {
                brace_depth = brace_depth.saturating_sub(1);
                while let Some((depth, _)) = package_blocks.last() {
                    if *depth <= brace_depth {
                        break;
                    }
                    let Some((_, previous)) = package_blocks.pop() else {
                        break;
                    };
                    current = previous;
                }
            }
            _ => {}
        }
    }

    (current != "main").then_some(current)
}

/// Text-pattern arm of [`classify_receiver`]. Looks for `Foo->method`
/// (static), `my $x = Foo->new` (constructor assignment), and
/// `my $x = bless ..., "Foo"` (literal bless). Defining-class invocant
/// spellings are not classified here.
#[cfg(test)]
pub(super) fn classify_text_pattern_receiver(
    context: &CompletionContext,
    source: &str,
) -> ReceiverEvidence {
    classify_text_pattern_receiver_with_symbol_table(context, source, None)
}

pub(super) fn classify_text_pattern_receiver_with_symbol_table(
    context: &CompletionContext,
    source: &str,
    _symbol_table: Option<&SymbolTable>,
) -> ReceiverEvidence {
    let arrow_prefix = context.receiver_prefix().trim_end_matches("->");

    // Case 1: Static method call like `My::Package->meth` or `Package->meth`.
    // The prefix already contains the package name (starts with uppercase, no sigil).
    if !arrow_prefix.starts_with('$')
        && !arrow_prefix.starts_with('@')
        && !arrow_prefix.starts_with('%')
        && arrow_prefix.chars().next().is_some_and(|c| c.is_ascii_uppercase())
    {
        return ReceiverEvidence::StaticPackage(arrow_prefix.to_string());
    }

    // Case 2: Variable method call like `$obj->meth` — try to find the
    // receiver type from a recent assignment.
    if arrow_prefix.starts_with('$') {
        let var_name = arrow_prefix;
        let before = &source[..context.position.min(source.len())];

        let lines: Vec<&str> = before.lines().collect();
        for (line_idx, line) in lines.iter().enumerate().rev() {
            let trimmed = line.trim();
            let assign_pos = find_assignment_eq(trimmed);
            if let Some(assign_pos) = assign_pos {
                let lhs = trimmed[..assign_pos].trim();
                if lhs.ends_with(var_name) || lhs.contains(&format!("{var_name} ")) {
                    let rhs = collect_assignment_rhs(&lines, line_idx, assign_pos);
                    let rhs = rhs.trim();
                    // Pattern: `Package::Name->new(...)`
                    if let Some(arrow_pos) = rhs.find("->") {
                        let pkg = rhs[..arrow_pos].trim();
                        if pkg.contains("::")
                            || pkg.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                        {
                            return ReceiverEvidence::ConstructorAssignment(pkg.to_string());
                        }
                    }
                    // Pattern: `bless REF, "Class"` / `bless REF, 'Class'`.
                    // Only literal-string class names produce inference; dynamic
                    // forms like `bless {}, $class` intentionally fall through
                    // (extract_bless_literal_class is fail-closed).
                    if let Some(class) = extract_bless_literal_class(rhs) {
                        return ReceiverEvidence::LiteralBless(class);
                    }
                    // Pattern: dynamic / fail-closed `bless` form (#7929).
                    // We saw a `bless` keyword in the RHS (outside string
                    // literals) but could not extract a literal class —
                    // covers `bless {}, $class`, `bless {}, "Foo" . $suffix`,
                    // `wrapper(bless {}, "Foo")`, `bless::class {}, "Foo"`,
                    // etc. The receiver is dynamic; fail closed (no exact
                    // package and no Unknown-receiver fallback).
                    if rhs_has_bless_keyword_outside_strings(rhs) {
                        return ReceiverEvidence::Dynamic;
                    }
                }
            }
        }
    }

    ReceiverEvidence::Unknown
}

fn collect_assignment_rhs(lines: &[&str], line_idx: usize, assign_pos: usize) -> String {
    let first_line = lines[line_idx].trim();
    let mut rhs = first_line[assign_pos + 1..].trim().to_string();
    if truncate_after_top_level_semicolon(&rhs).len() < rhs.len() {
        return truncate_after_top_level_semicolon(&rhs).to_string();
    }

    for continuation in lines.iter().skip(line_idx + 1) {
        if !rhs.is_empty() {
            rhs.push('\n');
        }
        rhs.push_str(continuation.trim_end());
        let truncated = truncate_after_top_level_semicolon(&rhs);
        if truncated.len() < rhs.len() {
            return truncated.to_string();
        }
    }

    rhs
}

fn truncate_after_top_level_semicolon(s: &str) -> &str {
    let mut depth_paren: i32 = 0;
    let mut depth_brace: i32 = 0;
    let mut depth_bracket: i32 = 0;
    let mut in_string: Option<char> = None;
    let mut prev_was_backslash = false;

    for (idx, ch) in s.char_indices() {
        if let Some(q) = in_string {
            if !prev_was_backslash && ch == q {
                in_string = None;
            }
            prev_was_backslash = !prev_was_backslash && ch == '\\';
            continue;
        }

        prev_was_backslash = false;
        match ch {
            '"' | '\'' => in_string = Some(ch),
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            '[' => depth_bracket += 1,
            ']' => depth_bracket -= 1,
            ';' if depth_paren == 0 && depth_brace == 0 && depth_bracket == 0 => {
                return &s[..idx + ch.len_utf8()];
            }
            _ => {}
        }
    }

    s
}

/// Returns `true` when the RHS contains a *call-like* `bless` keyword
/// outside string literals and comments. Used by
/// `classify_text_pattern_receiver` to detect dynamic / fail-closed bless
/// expressions that could not be resolved to a literal class — these
/// include nested calls (`wrapper(bless {}, "Foo")`), expression-tail
/// forms (`bless {}, "Foo" . $suffix`), dynamic class
/// (`bless {}, $class`), and non-builtin `bless`-prefixed identifiers
/// (`bless::class {}, "Foo"`). Issue #7929.
///
/// "Call-like" means `bless` is followed by ASCII whitespace, `(`, or
/// `::` (the qualified non-builtin form), and is not preceded by a Perl
/// sigil (`$`/`@`/`%`/`&`), an identifier byte, or a hash-key opener
/// (`{`). This rejects harmless mentions like `$bless`, `$obj->{bless}`,
/// and `# bless ...` comments which should remain Unknown / fallback-
/// eligible rather than fail-closed Dynamic.
fn rhs_has_bless_keyword_outside_strings(rhs: &str) -> bool {
    let bytes = rhs.as_bytes();
    let needle = b"bless";
    let mut in_string: Option<u8> = None;
    let mut prev_was_backslash = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_string {
            if !prev_was_backslash && b == q {
                in_string = None;
            }
            prev_was_backslash = !prev_was_backslash && b == b'\\';
            i += 1;
            continue;
        }
        prev_was_backslash = false;
        // Outside strings, `#` starts a comment that runs to end-of-line.
        // The RHS scan is single-line, so terminate here.
        if b == b'#' {
            return false;
        }
        if b == b'"' || b == b'\'' {
            in_string = Some(b);
            i += 1;
            continue;
        }
        if i + needle.len() <= bytes.len()
            && &bytes[i..i + needle.len()] == needle
            && is_call_like_bless(bytes, i)
        {
            return true;
        }
        i += 1;
    }
    false
}

/// Returns `true` when the `bless` token at `bytes[i..i+5]` is *call-like*:
/// not preceded by a sigil/ident-byte/hash-key opener, and followed by
/// whitespace, `(`, or `::`. See [`rhs_has_bless_keyword_outside_strings`].
fn is_call_like_bless(bytes: &[u8], i: usize) -> bool {
    let prev_ok = match i.checked_sub(1).map(|j| bytes[j]) {
        None => true,
        Some(p) => {
            !is_perl_ident_byte_local(p)
                && p != b'$'
                && p != b'@'
                && p != b'%'
                && p != b'&'
                && p != b'{'
        }
    };
    if !prev_ok {
        return false;
    }
    let next_idx = i + b"bless".len();
    match bytes.get(next_idx).copied() {
        None => false,
        Some(n) => {
            n.is_ascii_whitespace()
                || n == b'('
                || (n == b':' && bytes.get(next_idx + 1).copied() == Some(b':'))
        }
    }
}

fn is_perl_ident_byte_local(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Extract the literal class name from a `bless REF, "Class"` expression.
///
/// Anchored to RHS-as-builtin-bless-expression: only succeeds when the
/// RHS, after trimming leading whitespace, *starts* with `bless` followed
/// by end-of-string, ASCII whitespace, or `(`. This means the helper
/// returns `None` for nested forms like `wrapper(bless {}, "Foo")` (where
/// the assignment result is not necessarily the blessed object) and for
/// non-builtin punctuation-suffixed forms like `bless::factory {}, "Foo"`
/// (where the call target is a different sub that merely shares the
/// `bless` prefix).
///
/// Trailing content after the closing quote of the class literal must be
/// only whitespace, the matching closing paren if a leading `(` was
/// consumed, and an optional terminating `;`. Anything else — `. $suffix`,
/// `|| "Bar"`, `, $extra`, etc. — disables inference (fails closed) so
/// non-literal class expressions never produce false-precision evidence.
///
/// Returns `Some(class)` only for the conservative literal form. Returns
/// `None` for dynamic forms (`bless {}, $class`), 1-arg forms
/// (`bless {}` — defaults to caller package, intentionally not inferred
/// here), nested forms, expression-tail forms, and anything that fails
/// to parse cleanly.
fn extract_bless_literal_class(rhs: &str) -> Option<String> {
    // Anchor: RHS must START with the builtin `bless` expression.
    let trimmed = rhs.trim_start();
    if !starts_with_bless_expression(trimmed) {
        return None;
    }
    let after_bless = &trimmed["bless".len()..];

    // Allow optional `(` and whitespace.
    let scan = after_bless.trim_start();
    let (scan, expect_rparen) = match scan.strip_prefix('(') {
        Some(rest) => (rest, true),
        None => (scan, false),
    };

    // Find the comma separating the two args, respecting balanced delimiters
    // and string literals so that hash/array contents like
    // `bless { a => 1, b => 2 }, "Foo"` and `bless [1, 2, 3], "Foo"` parse.
    let comma_pos = find_top_level_comma(scan)?;
    let after_comma = scan[comma_pos + 1..].trim_start();

    // Require a literal quoted string for the class.
    let bytes = after_comma.as_bytes();
    let close_char = match bytes.first()? {
        b'"' => '"',
        b'\'' => '\'',
        _ => return None,
    };
    let body = &after_comma[1..];
    let close_pos = body.find(close_char)?;
    let class = &body[..close_pos];

    if !is_valid_perl_package_name(class) {
        return None;
    }

    // Validate trailing content: only whitespace, the matching `)` if a
    // leading `(` was consumed, and an optional `;` then EOF. Anything else
    // (concatenation, logical-or, extra arg, expression continuation) means
    // the class expression is not a plain literal — fail closed.
    let mut tail = body[close_pos + 1..].trim_start();
    if expect_rparen {
        tail = tail.strip_prefix(')')?.trim_start();
    }
    let tail = tail.strip_prefix(';').unwrap_or(tail).trim();
    if !tail.is_empty() {
        return None;
    }

    Some(class.to_string())
}

/// Returns `true` when `s` begins with the builtin `bless` expression.
///
/// Stricter than a generic word-boundary check: after `bless`, only end
/// of string, ASCII whitespace, or `(` are accepted. This rejects
/// non-builtin forms like `bless::factory(...)`, `bless+REF`, `bless.foo`,
/// and similar punctuation-suffixed identifiers that happen to share the
/// `bless` prefix but are not the Perl builtin.
fn starts_with_bless_expression(s: &str) -> bool {
    if !s.starts_with("bless") {
        return false;
    }
    match s.as_bytes().get("bless".len()).copied() {
        None => true,
        Some(b) => b.is_ascii_whitespace() || b == b'(',
    }
}

/// Find the first top-level `,` outside of `()`, `{}`, `[]`, and string
/// literals. Used to identify the arg separator in `bless REF, CLASS`.
fn find_top_level_comma(s: &str) -> Option<usize> {
    let mut depth_paren: i32 = 0;
    let mut depth_brace: i32 = 0;
    let mut depth_bracket: i32 = 0;
    let mut in_string: Option<char> = None;
    let mut prev_was_backslash = false;

    for (i, c) in s.char_indices() {
        if let Some(q) = in_string {
            if !prev_was_backslash && c == q {
                in_string = None;
            }
            prev_was_backslash = !prev_was_backslash && c == '\\';
            continue;
        }
        prev_was_backslash = false;
        match c {
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            '[' => depth_bracket += 1,
            ']' => depth_bracket -= 1,
            '"' => in_string = Some('"'),
            '\'' => in_string = Some('\''),
            ',' if depth_paren <= 0 && depth_brace <= 0 && depth_bracket <= 0 => {
                return Some(i);
            }
            _ => {}
        }
    }
    None
}

/// Validate a Perl package name: identifier segments separated by `::`.
fn is_valid_perl_package_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut start_of_segment = true;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if start_of_segment {
            if !(c.is_ascii_alphabetic() || c == '_') {
                return false;
            }
            start_of_segment = false;
            continue;
        }
        if c == ':' {
            // Must be `::`, then a fresh segment.
            if chars.next() != Some(':') {
                return false;
            }
            start_of_segment = true;
            continue;
        }
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
    }
    !start_of_segment
}

/// Find the position of a single assignment `=` in a line, skipping compound
/// operators like `==`, `!=`, `<=`, `>=`, `=~`, and `=>`.
///
/// Returns `None` if no assignment operator is found.
fn find_assignment_eq(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'=' {
            continue;
        }
        // Skip if preceded by !, <, >, or = (compound operators)
        if i > 0 && matches!(bytes[i - 1], b'!' | b'<' | b'>' | b'=') {
            continue;
        }
        // Skip if followed by = or ~ or > (==, =~, =>)
        if i + 1 < bytes.len() && matches!(bytes[i + 1], b'=' | b'~' | b'>') {
            continue;
        }
        return Some(i);
    }
    None
}
