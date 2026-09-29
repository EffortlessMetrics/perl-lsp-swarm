//! Classification of SQL-text expressions at a reviewed DBI sink (#16864).
//!
//! The producer only warns on proven dynamic assembly into the SQL text.
//! A computed statement (variable, call result) is a typed dynamic boundary:
//! the walker cannot distinguish safe from unsafe assembly, so it never
//! guesses and stays silent — mirroring the diagnostics family's
//! dynamic-boundary suppression precedent
//! (`providers/diagnostics/diagnostics_shadow.rs`).

use perl_parser_core::ast::{Node, NodeKind};

/// Classification of the SQL statement argument at a DBI sink.
///
/// Ordered so [`SqlTextEvidence::combine`] can take the strongest evidence
/// without a nested match: proven composition dominates a typed boundary,
/// which dominates a static literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum SqlTextEvidence {
    /// Literal-only SQL (with or without `?` placeholders): proven safe shape.
    Static,
    /// The SQL text is computed and indistinguishable at AST level.
    DynamicBoundary,
    /// A variable is concatenated into the SQL literal.
    Concatenated,
    /// A `$`/`@` sigil variable is interpolated directly into the SQL literal.
    Interpolated,
}

impl SqlTextEvidence {
    /// Fold two operands of a concatenation or every branch of a finite
    /// conditional. Proven interpolation/concatenation dominates; otherwise a
    /// computed operand keeps the whole expression a typed boundary.
    pub(super) fn combine(self, other: Self) -> Self {
        self.max(other)
    }

    pub(super) const fn is_admitted_composition(self) -> bool {
        matches!(self, Self::Interpolated | Self::Concatenated)
    }
}

/// Classify the SQL text expression of a DBI sink call.
pub(super) fn classify_sql_text(node: &Node) -> SqlTextEvidence {
    match &node.kind {
        NodeKind::String { value, interpolated } => {
            if *interpolated && string_contains_interpolation(value) {
                SqlTextEvidence::Interpolated
            } else {
                // Single-quoted literal, `q{}`, or a double-quoted literal
                // with no interpolating sigil: static SQL, placeholders included.
                SqlTextEvidence::Static
            }
        }
        // Heredocs are string literals: an interpolating heredoc (`<<SQL`,
        // `<<"SQL"`) whose body contains a sigil is source-proven assembly,
        // exactly like a double-quoted string; a literal heredoc
        // (`<<'SQL'`) is static (#5035 review).
        NodeKind::Heredoc { content, interpolated, .. } => {
            if *interpolated && string_contains_interpolation(content) {
                SqlTextEvidence::Interpolated
            } else {
                SqlTextEvidence::Static
            }
        }
        NodeKind::Binary { op, .. } if op == "." => classify_concatenation(node),
        // Finite ternary: admit only when an admitted branch is dynamically
        // composed; a computed branch without proven composition stays a
        // typed boundary (#16864).
        NodeKind::Ternary { then_expr, else_expr, .. } => {
            classify_sql_text(then_expr).combine(classify_sql_text(else_expr))
        }
        // A bare variable, call result, or any other expression: the SQL text
        // is computed and indistinguishable at AST level.
        _ => SqlTextEvidence::DynamicBoundary,
    }
}

/// Classify a `.` concatenation chain by combining operand evidence.
fn classify_concatenation(node: &Node) -> SqlTextEvidence {
    let NodeKind::Binary { left, right, .. } = &node.kind else {
        return SqlTextEvidence::DynamicBoundary;
    };

    classify_operand(left).combine(classify_operand(right))
}

fn classify_operand(operand: &Node) -> SqlTextEvidence {
    match &operand.kind {
        NodeKind::Binary { op, .. } if op == "." => classify_concatenation(operand),
        NodeKind::Variable { .. } => SqlTextEvidence::Concatenated,
        NodeKind::String { .. } | NodeKind::Heredoc { .. } | NodeKind::Ternary { .. } => {
            classify_sql_text(operand)
        }
        _ => SqlTextEvidence::DynamicBoundary,
    }
}

/// Whether an interpolating string's text interpolates a variable.
///
/// Escaping follows Perl backslash parity: only an odd-length run of
/// preceding backslashes escapes the sigil. An even-length run escapes the
/// backslash itself, so the sigil still interpolates (`"\\$id"` interpolates
/// `$id` after emitting a literal backslash) (#5035 review).
///
/// Scalar interpolation follows the parser-core name-start rule: almost every
/// non-whitespace character after `$` names a scalar (`$id`, `$1`, `${...}`,
/// `$&`, `` $` ``, `$'`, `$+`, `$@`, `$!`, `$$`, `$?`). Array interpolation
/// stays narrower: identifier starts, `${}`-style braces, package `::`, and
/// the match-offset arrays `@-` / `@+`.
fn string_contains_interpolation(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    chars.iter().enumerate().any(|(index, &ch)| {
        let sigil = match ch {
            '$' | '@' => ch,
            _ => return false,
        };
        if escaped_by_backslash_run(&chars, index) {
            return false;
        }
        chars.get(index + 1).copied().is_some_and(|next| is_interpolation_successor(sigil, next))
    })
}

fn is_interpolation_successor(sigil: char, next: char) -> bool {
    match sigil {
        '$' => !next.is_whitespace(),
        '@' => next.is_alphabetic() || matches!(next, '_' | '{' | ':' | '-' | '+'),
        _ => false,
    }
}

fn escaped_by_backslash_run(chars: &[char], index: usize) -> bool {
    let mut run = 0;
    let mut cursor = index;
    while cursor > 0 && chars.get(cursor - 1) == Some(&'\\') {
        run += 1;
        cursor -= 1;
    }
    run % 2 == 1
}
