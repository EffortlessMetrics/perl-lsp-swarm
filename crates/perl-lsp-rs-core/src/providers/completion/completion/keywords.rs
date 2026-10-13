//! Keyword completion for Perl
//!
//! Provides completion for Perl keywords with snippet expansion.
//!
//! Sort tier: 5_ — keywords come after core builtins (3_) and workspace
//! symbols (4_) because they are always available and match via snippet
//! expansion. Users typing a partial identifier are usually looking for their
//! own symbols or a builtin before a keyword.

use super::{context::CompletionContext, items::CompletionItem, items::InsertTextFormat};
use perl_lexer::LSP_COMPLETION_KEYWORDS;
use std::borrow::Cow;

/// Canonical Perl keywords for completion.
#[must_use]
pub fn keywords() -> &'static [&'static str] {
    LSP_COMPLETION_KEYWORDS
}

/// Keywords that cannot start a term in a value/expression position.
///
/// This includes statement declarations (`package`, `use`, phasers, `class`),
/// compound-statement openers (`if`, `while`, `for`, …), and infix operators
/// (`eq`, `and`, `cmp`, …) that need a left operand. Sorted for binary search
/// and partition checks against [`keywords`].
pub const STATEMENT_ONLY_KEYWORDS: &[&str] = &[
    "ADJUST",
    "AUTOLOAD",
    "BEGIN",
    "CHECK",
    "DESTROY",
    "END",
    "INIT",
    "UNITCHECK",
    "and",
    "catch",
    "class",
    "cmp",
    "default",
    "defer",
    "else",
    "elsif",
    "eq",
    "field",
    "finally",
    "for",
    "foreach",
    "ge",
    "given",
    "gt",
    "if",
    "isa",
    "le",
    "lt",
    "method",
    "ne",
    "or",
    "package",
    "try",
    "unless",
    "until",
    "use",
    "when",
    "while",
    "xor",
];

/// Keywords that can start a term where a value is expected (anonymous `sub`,
/// `do`/`eval` BLOCK, declarators, unary operators, special tokens).
pub const EXPRESSION_OK_KEYWORDS: &[&str] = &[
    "__CLASS__",
    "__FILE__",
    "__LINE__",
    "__PACKAGE__",
    "__SUB__",
    "async",
    "await",
    "blessed",
    "defined",
    "die",
    "do",
    "eval",
    "exit",
    "goto",
    "last",
    "local",
    "my",
    "next",
    "not",
    "our",
    "redo",
    "ref",
    "require",
    "return",
    "scalar",
    "state",
    "sub",
    "undef",
    "wantarray",
    "warn",
];

const _: () = assert!(
    STATEMENT_ONLY_KEYWORDS.len() + EXPRESSION_OK_KEYWORDS.len() == LSP_COMPLETION_KEYWORDS.len()
);

/// Keywords admitted at a statement position (the full inventory) or a value
/// position (`expression_ok` only).
#[must_use]
pub fn keywords_for_position(in_expression_position: bool) -> &'static [&'static str] {
    if in_expression_position { EXPRESSION_OK_KEYWORDS } else { keywords() }
}

/// Curated priority among keywords: the constructs a user is most likely to
/// type next at an empty identifier position, in preference order. Keywords
/// rank within tier 5 by this list first and label second; without it the
/// tier's ASCII ordering systematically surfaces obscure uppercase keywords
/// (ADJUST, AUTOLOAD, …) ahead of control flow (#11858).
const PREFERRED_KEYWORD_ORDER: &[&str] = &[
    "if", "else", "elsif", "unless", "while", "until", "for", "foreach", "my", "sub", "return",
    "package", "use", "our", "local", "next", "last", "redo", "do",
];

/// The control-flow constructs an empty-identifier completion page must
/// always contain (#11858): page-level reserve targets, label-identified so
/// the guarantee cannot be satisfied by unrelated items of the same kind.
pub const FUNDAMENTAL_CONSTRUCT_LABELS: &[&str] =
    &["if", "else", "elsif", "unless", "while", "until", "for", "foreach", "print", "my", "sub"];

fn keyword_preference(keyword: &str) -> usize {
    PREFERRED_KEYWORD_ORDER
        .iter()
        .position(|preferred| *preferred == keyword)
        .unwrap_or(PREFERRED_KEYWORD_ORDER.len())
}

/// Return a brief documentation string for a Perl keyword.
fn keyword_doc(keyword: &str) -> Option<&'static str> {
    match keyword {
        "sub" => Some("Declare a named or anonymous subroutine. Usage: sub name { BLOCK }"),
        "if" => Some("Conditional execution. Usage: if (CONDITION) { BLOCK } elsif { } else { }"),
        "elsif" => Some("Additional condition branch in an if/elsif chain."),
        "else" => Some("Default branch executed when no if/elsif condition is true."),
        "unless" => Some("Execute BLOCK when CONDITION is false. Opposite of 'if'."),
        "while" => Some("Loop while CONDITION is true. Usage: while (CONDITION) { BLOCK }"),
        "until" => Some("Loop until CONDITION becomes true. Opposite of 'while'."),
        "for" => Some("C-style for loop. Usage: for (INIT; CONDITION; STEP) { BLOCK }"),
        "foreach" => Some("Iterate over a list. Usage: foreach my $item (@list) { BLOCK }"),
        "do" => Some("Execute a BLOCK or file. Usage: do { BLOCK } or do FILE"),
        "package" => {
            Some("Declare a package (namespace). Usage: package Name; or package Name { BLOCK }")
        }
        "use" => {
            Some("Load a module at compile time and import symbols. Usage: use Module qw(sym)")
        }
        "no" => Some("Unimport a module's symbols. Usage: no warnings 'experimental'"),
        "require" => Some("Load a module at runtime. Usage: require Module or require 'file.pl'"),
        "return" => Some("Return from a subroutine with an optional value."),
        "my" => Some("Declare a lexically-scoped variable. Usage: my $var = VALUE"),
        "our" => Some("Declare a package-scoped variable. Usage: our $VAR"),
        "local" => Some("Temporarily change a global variable's value. Usage: local $var = VALUE"),
        "next" => Some("Skip to the next iteration of the enclosing loop."),
        "last" => Some("Exit the innermost enclosing loop immediately."),
        "redo" => Some("Restart the current loop iteration without re-evaluating the condition."),
        "given" => Some("Experimental switch statement. Requires 'use feature :5.10'."),
        "when" => Some("Experimental case arm inside 'given'. Requires 'use feature :5.10'."),
        "default" => {
            Some("Experimental default case inside 'given'. Requires 'use feature :5.10'.")
        }
        "and" => Some("Low-precedence logical AND. Same as '&&' but lower precedence."),
        "or" => Some("Low-precedence logical OR. Same as '||' but lower precedence."),
        "not" => Some("Low-precedence logical NOT. Same as '!' but lower precedence."),
        "xor" => Some("Low-precedence logical exclusive OR."),
        "async" => Some(
            "Marks a subroutine as asynchronous (Perl 5.36+ experimental, requires `use feature 'async_await'`).",
        ),
        "await" => Some("Suspends execution until a Future completes (Perl 5.36+ experimental)."),
        "eq" => Some("String equality comparison. Returns true if strings are equal."),
        "ne" => Some("String inequality comparison. Returns true if strings differ."),
        "lt" => Some("String less-than comparison."),
        "gt" => Some("String greater-than comparison."),
        "le" => Some("String less-than-or-equal comparison."),
        "ge" => Some("String greater-than-or-equal comparison."),
        "cmp" => Some("String comparison returning -1, 0, or 1."),
        "x" => Some("Repetition operator. List: (LIST) x N. String: EXPR x N"),
        "print" => Some("Print a list to a filehandle. Usage: print FILEHANDLE LIST"),
        "say" => Some("Like print but appends a newline. Requires 'use feature :5.10'."),
        "chomp" => Some("Remove trailing newline from a string or list."),
        _ => None,
    }
}

/// Add keyword completions.
///
/// `in_expression_position` selects the anonymous `sub { }` snippet, because a
/// named `sub NAME { }` is not a term after `=>` or another value operator.
pub fn add_keyword_completions(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    keywords: &[&'static str],
    in_expression_position: bool,
) {
    for &keyword in keywords {
        if keyword.starts_with(&context.prefix) {
            let (insert_text, snippet) = match keyword {
                "sub" if in_expression_position => ("sub {\n    $0\n}", true),
                "sub" => ("sub ${1:name} {\n    $0\n}", true),
                "if" => ("if ($1) {\n    $0\n}", true),
                "elsif" => ("elsif ($1) {\n    $0\n}", true),
                "else" => ("else {\n    $0\n}", true),
                "unless" => ("unless ($1) {\n    $0\n}", true),
                "while" => ("while ($1) {\n    $0\n}", true),
                "for" => ("for (my \\$i = 0; \\$i < $1; \\$i++) {\n    $0\n}", true),
                "foreach" => ("foreach my \\$${1:item} (@${2:array}) {\n    $0\n}", true),
                "package" => ("package ${1:Name};\n\n$0", true),
                "use" => ("use ${1:Module};\n$0", true),
                _ => (keyword, false),
            };

            completions.push(CompletionItem {
                label: Cow::Borrowed(keyword),
                kind: if snippet {
                    super::items::CompletionItemKind::Snippet
                } else {
                    super::items::CompletionItemKind::Keyword
                },
                detail: Some(Cow::Borrowed("keyword")),
                documentation: keyword_doc(keyword).map(Cow::Borrowed),
                insert_text: Some(Cow::Borrowed(insert_text)),
                // Tier 5: keywords sort after special vars (0_), user vars (1_),
                // user funcs (2_), core builtins (3_), and workspace symbols (4_).
                // Within the tier, the curated preference order ranks control-flow
                // constructs ahead of obscure keywords (#11858): with hundreds of
                // same-tier items beyond a page cap, plain label ordering would
                // fill any keyword representation with ADJUST-style entries.
                sort_text: Some(Cow::Owned(format!(
                    "5_{:02}_{}",
                    keyword_preference(keyword),
                    keyword
                ))),
                filter_text: Some(Cow::Borrowed(keyword)),
                additional_edits: vec![],
                text_edit_range: Some((context.prefix_start, context.position)),
                commit_characters: None,
                insert_text_format: InsertTextFormat::for_authored_body(insert_text),
                label_details: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::context::CompletionContext;
    use super::*;
    use crate::providers::completion_item::snippet_body_defects;
    use perl_tdd_support::must_some;

    fn context_for(prefix: &str) -> CompletionContext {
        CompletionContext {
            position: prefix.len(),
            trigger_character: None,
            in_string: false,
            in_regex: false,
            in_comment: false,
            in_use_statement: false,
            current_package: "main".to_string(),
            prefix: prefix.to_string(),
            prefix_start: 0,
            cursor_scope_id: 0,
        }
    }

    fn completion_for(keyword: &str) -> CompletionItem {
        let mut items = Vec::new();
        add_keyword_completions(&mut items, &context_for(keyword), keywords(), false);
        must_some(items.into_iter().find(|item| item.label == keyword))
    }

    /// #4956 class: keyword snippet bodies must not spell literal Perl
    /// variables as snippet variables. `for`'s `$i` did exactly that.
    #[test]
    fn every_keyword_snippet_body_is_well_formed() {
        for &keyword in keywords() {
            let item = completion_for(keyword);
            let Some(body) = item.insert_text.as_deref() else { continue };
            if item.insert_text_format.is_snippet() {
                let defects = snippet_body_defects(body);
                assert!(defects.is_empty(), "`{keyword}`: {defects:?}");
            }
        }
    }

    /// The C-style loop counter is literal Perl on both client kinds.
    #[test]
    fn for_loop_counter_survives_as_literal_perl() {
        let item = completion_for("for");
        assert_eq!(
            item.insert_text_format.plain_fallback(),
            Some("for (my $i = 0; $i < ; $i++) {\n    \n}")
        );
    }

    /// `foreach` inserts a real `$item` scalar, not an empty placeholder.
    #[test]
    fn foreach_inserts_a_literal_scalar() {
        let item = completion_for("foreach");
        assert_eq!(
            item.insert_text_format.plain_fallback(),
            Some("foreach my $item (@array) {\n    \n}")
        );
    }

    /// A keyword with no expansion is inserted verbatim.
    #[test]
    fn plain_keyword_is_plaintext() {
        let item = completion_for("return");
        assert_eq!(item.insert_text.as_deref(), Some("return"));
        assert_eq!(item.insert_text_format, InsertTextFormat::PlainText);
    }

    #[test]
    fn expression_position_sub_snippet_is_anonymous_and_well_formed() {
        let mut items = Vec::new();
        add_keyword_completions(&mut items, &context_for("sub"), &["sub"], true);
        let item = must_some(items.into_iter().find(|item| item.label == "sub"));
        assert_eq!(item.insert_text.as_deref(), Some("sub {\n    $0\n}"));
        let defects = snippet_body_defects(must_some(item.insert_text.as_deref()));
        assert!(defects.is_empty(), "anonymous sub snippet: {defects:?}");
    }

    #[test]
    fn statement_position_sub_snippet_stays_named() {
        let item = completion_for("sub");
        assert_eq!(item.insert_text.as_deref(), Some("sub ${1:name} {\n    $0\n}"));
    }

    /// The two role lists must partition [`keywords`] so a newly added
    /// `LSP_COMPLETION_KEYWORDS` entry cannot land in both sets or neither.
    #[test]
    fn syntactic_role_lists_partition_the_keyword_inventory() {
        let all = keywords();
        let statement_only = STATEMENT_ONLY_KEYWORDS;
        let expression_ok = EXPRESSION_OK_KEYWORDS;
        assert!(
            is_strictly_sorted(statement_only),
            "STATEMENT_ONLY_KEYWORDS must be strictly sorted"
        );
        assert!(
            is_strictly_sorted(expression_ok),
            "EXPRESSION_OK_KEYWORDS must be strictly sorted"
        );
        assert_eq!(
            statement_only.len() + expression_ok.len(),
            all.len(),
            "role lists must cover the inventory without overlap: statement_only={} expression_ok={} all={}",
            statement_only.len(),
            expression_ok.len(),
            all.len()
        );
        for &keyword in all {
            let in_statement = statement_only.binary_search(&keyword).is_ok();
            let in_expression = expression_ok.binary_search(&keyword).is_ok();
            assert_ne!(
                in_statement, in_expression,
                "{keyword} must belong to exactly one syntactic role"
            );
        }
        assert!(statement_only.binary_search(&"package").is_ok(), "`package` is statement_only");
        for expression_ok_keyword in ["sub", "do", "eval", "my"] {
            assert!(
                expression_ok.binary_search(&expression_ok_keyword).is_ok(),
                "`{expression_ok_keyword}` is expression_ok"
            );
        }
    }

    fn is_strictly_sorted(items: &[&str]) -> bool {
        items.windows(2).all(|pair| pair[0] < pair[1])
    }
}
