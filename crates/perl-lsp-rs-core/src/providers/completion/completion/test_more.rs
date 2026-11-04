//! Test::More function completions for Perl
//!
//! Provides completion for Test::More functions in test contexts.

use super::{context::CompletionContext, items::CompletionItem, items::InsertTextFormat};
use std::borrow::Cow;

/// One Test::More export this server knows how to complete and document.
///
/// Completion insert text, hover signature, and hover description live on the
/// same row so a new export cannot be added to one surface and forgotten on the
/// other.
pub struct TestMoreFunction {
    /// The exported function name, exactly as Test::More exports it.
    pub name: &'static str,
    /// Completion insert text. Authored as a TextMate snippet body when it has
    /// tab stops; a literal Perl `$` must be written `\$`.
    pub snippet: &'static str,
    /// Human-readable call signature, shown on hover.
    pub signature: &'static str,
    /// One-line description, shown on hover and as completion documentation.
    pub description: &'static str,
}

/// Test::More exports this server completes and documents.
///
/// Checked against Test::More 1.302194's `@EXPORT` by
/// `table_covers_every_callable_export`; see that test for the two names in
/// `@EXPORT` that are deliberately absent.
pub const TEST_MORE_FUNCTIONS: &[TestMoreFunction] = &[
    TestMoreFunction {
        name: "ok",
        snippet: "ok(${1:condition}, ${2:name});",
        signature: "ok($expr, $name)",
        description: "Test condition is true",
    },
    TestMoreFunction {
        name: "is",
        snippet: "is(${1:got}, ${2:expected}, ${3:name});",
        signature: "is($got, $expected, $name)",
        description: "Test values are equal (string comparison)",
    },
    TestMoreFunction {
        name: "isnt",
        snippet: "isnt(${1:got}, ${2:not_expected}, ${3:name});",
        signature: "isnt($got, $not_expected, $name)",
        description: "Test values are not equal",
    },
    TestMoreFunction {
        name: "like",
        snippet: "like(${1:got}, ${2:qr/.../}, ${3:name});",
        signature: "like($got, qr/.../, $name)",
        description: "Test string matches regex",
    },
    TestMoreFunction {
        name: "unlike",
        snippet: "unlike(${1:got}, ${2:qr/.../}, ${3:name});",
        signature: "unlike($got, qr/.../, $name)",
        description: "Test string does not match regex",
    },
    TestMoreFunction {
        name: "cmp_ok",
        snippet: "cmp_ok(${1:got}, '${2:op}', ${3:expected}, ${4:name});",
        signature: "cmp_ok($got, $op, $expected, $name)",
        description: "Compare using an operator",
    },
    TestMoreFunction {
        name: "isa_ok",
        snippet: "isa_ok(${1:ref}, '${2:class}', ${3:name});",
        signature: "isa_ok($ref, $class, $name)",
        description: "Test object is of the given class",
    },
    TestMoreFunction {
        name: "can_ok",
        snippet: "can_ok(${1:class_or_obj}, ${2:@methods});",
        signature: "can_ok($class_or_obj, @methods)",
        description: "Test object/class can do methods",
    },
    TestMoreFunction {
        name: "pass",
        snippet: "pass(${1:name});",
        signature: "pass($name)",
        description: "Unconditionally pass a test",
    },
    TestMoreFunction {
        name: "fail",
        snippet: "fail(${1:name});",
        signature: "fail($name)",
        description: "Unconditionally fail a test",
    },
    TestMoreFunction {
        name: "diag",
        snippet: "diag(${1:message});",
        signature: "diag($message)",
        description: "Print a diagnostic message to STDERR",
    },
    TestMoreFunction {
        name: "note",
        snippet: "note(${1:message});",
        signature: "note($message)",
        description: "Print a note message to STDOUT",
    },
    TestMoreFunction {
        name: "explain",
        snippet: "explain(${1:\\$ref});",
        signature: "explain($ref)",
        description: "Dump a data structure as a string",
    },
    TestMoreFunction {
        name: "skip",
        snippet: "skip(${1:why}, ${2:how_many});",
        signature: "skip($why, $how_many)",
        description: "Skip tests (inside a SKIP block)",
    },
    TestMoreFunction {
        name: "todo_skip",
        snippet: "todo_skip(${1:why}, ${2:how_many});",
        signature: "todo_skip($why, $how_many)",
        description: "Mark tests as TODO and skip running them",
    },
    TestMoreFunction {
        name: "BAIL_OUT",
        snippet: "BAIL_OUT(${1:reason});",
        signature: "BAIL_OUT($reason)",
        description: "Stop all testing immediately",
    },
    TestMoreFunction {
        name: "subtest",
        snippet: "subtest '${1:name}' => sub {\n    ${0}\n};",
        signature: "subtest $name => sub { ... }",
        description: "Run a subtest in its own scope",
    },
    TestMoreFunction {
        name: "done_testing",
        snippet: "done_testing(${1:tests});",
        signature: "done_testing($tests?)",
        description: "Finish testing (optional count; omit to auto-count)",
    },
    TestMoreFunction {
        name: "plan",
        snippet: "plan tests => ${1:num};",
        signature: "plan tests => $num",
        description: "Declare the number of tests to run",
    },
    TestMoreFunction {
        name: "use_ok",
        snippet: "use_ok('${1:Module}');",
        signature: "use_ok($module)",
        description: "Test that a module loads successfully",
    },
    TestMoreFunction {
        name: "require_ok",
        snippet: "require_ok('${1:Module}');",
        signature: "require_ok($module)",
        description: "Test that a module requires successfully",
    },
    TestMoreFunction {
        name: "is_deeply",
        snippet: "is_deeply(${1:\\$got}, ${2:\\$expected}, ${3:name});",
        signature: "is_deeply($got, $expected, $name)",
        description: "Deep structure comparison",
    },
    TestMoreFunction {
        name: "new_ok",
        snippet: "new_ok('${1:Class}', [${2:args}], ${3:name});",
        signature: "new_ok($class, \\@args, $name)",
        description: "Test object creation",
    },
    TestMoreFunction {
        name: "eq_array",
        snippet: "eq_array(${1:\\\\@got}, ${2:\\\\@expected});",
        signature: "eq_array(\\@got, \\@expected)",
        description: "Deep array equivalence, returning a boolean rather than \
                      running a test. Discouraged: prefer is_deeply",
    },
    TestMoreFunction {
        name: "eq_hash",
        snippet: "eq_hash(${1:\\\\%got}, ${2:\\\\%expected});",
        signature: "eq_hash(\\%got, \\%expected)",
        description: "Deep hash equivalence, returning a boolean rather than \
                      running a test. Discouraged: prefer is_deeply",
    },
    TestMoreFunction {
        name: "eq_set",
        snippet: "eq_set(${1:\\\\@got}, ${2:\\\\@expected});",
        signature: "eq_set(\\@got, \\@expected)",
        description: "Order-insensitive top-level array equivalence, returning a \
                      boolean. Discouraged: duplicates still matter, and \
                      is_deeply on sorted copies is clearer",
    },
];

/// Return `(signature, description)` for a Test::More function, or `None` if unknown.
pub fn get_test_more_documentation(name: &str) -> Option<(&'static str, &'static str)> {
    TEST_MORE_FUNCTIONS.iter().find(|f| f.name == name).map(|f| (f.signature, f.description))
}

/// Add Test::More completions
pub fn add_test_more_completions(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
) {
    for function in TEST_MORE_FUNCTIONS {
        if context.prefix.is_empty() || function.name.starts_with(&context.prefix) {
            completions.push(CompletionItem {
                label: Cow::Borrowed(function.name),
                kind: super::items::CompletionItemKind::Function,
                detail: Some(Cow::Borrowed("Test::More")),
                documentation: Some(Cow::Borrowed(function.description)),
                insert_text: Some(Cow::Borrowed(function.snippet)),
                sort_text: Some(Cow::Owned(format!("2_{}", function.name))),
                filter_text: Some(Cow::Borrowed(function.name)),
                additional_edits: vec![],
                text_edit_range: Some((context.prefix_start, context.position)),
                commit_characters: None,
                insert_text_format: InsertTextFormat::for_authored_body(function.snippet),
                label_details: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TEST_MORE_FUNCTIONS, get_test_more_documentation};
    use crate::providers::completion_item::{InsertTextFormat, snippet_body_defects};

    /// Every entry's insert text carries `${1:...}` tab stops. Declaring them
    /// `PlainText` — as this table did — sends that grammar to the editor as
    /// literal buffer text, the same defect fixed for the XS API table in
    /// #4956.
    #[test]
    fn entries_with_tab_stops_are_snippet_formatted() {
        for function in TEST_MORE_FUNCTIONS {
            let format = InsertTextFormat::for_authored_body(function.snippet);
            if function.snippet.contains("${") {
                assert!(
                    format.is_snippet(),
                    "`{}` has tab stops but is not snippet-formatted",
                    function.name
                );
                let defects = snippet_body_defects(function.snippet);
                assert!(defects.is_empty(), "`{}`: {defects:?}", function.name);
            } else {
                assert_eq!(
                    format,
                    InsertTextFormat::PlainText,
                    "`{}` has no snippet construct and must stay plaintext",
                    function.name
                );
            }
        }
    }

    /// Clients without `snippetSupport` receive the fallback verbatim, so it
    /// must not leak snippet grammar into the buffer.
    #[test]
    fn plain_fallbacks_are_free_of_snippet_grammar() {
        for function in TEST_MORE_FUNCTIONS {
            let format = InsertTextFormat::for_authored_body(function.snippet);
            let Some(fallback) = format.plain_fallback() else {
                continue;
            };
            assert!(
                !fallback.contains("${") && !fallback.contains("$0"),
                "`{}` fallback still carries snippet grammar: {fallback:?}",
                function.name
            );
            assert!(
                fallback.starts_with(function.name),
                "`{}` fallback does not call the function it completes: {fallback:?}",
                function.name
            );
        }
    }

    #[test]
    fn completion_bodies_are_statement_complete() {
        for function in TEST_MORE_FUNCTIONS {
            assert!(
                function.snippet.ends_with(';'),
                "`{}` completion body must be a complete Perl statement: {:?}",
                function.name,
                function.snippet
            );
        }
    }

    /// `@Test::More::EXPORT` as of Test::More 1.302194.
    const TEST_MORE_EXPORT: &[&str] = &[
        "$TODO",
        "BAIL_OUT",
        "can_ok",
        "cmp_ok",
        "diag",
        "done_testing",
        "eq_array",
        "eq_hash",
        "eq_set",
        "explain",
        "fail",
        "is",
        "is_deeply",
        "isa_ok",
        "isnt",
        "like",
        "new_ok",
        "note",
        "ok",
        "pass",
        "plan",
        "require_ok",
        "skip",
        "subtest",
        "todo",
        "todo_skip",
        "unlike",
        "use_ok",
    ];

    /// Two names in `@EXPORT` are not completable functions and are absent on
    /// purpose:
    ///
    /// - `$TODO` is a package variable (`local $TODO = ...` inside a `TODO`
    ///   block), not a call, so it does not belong in a function table.
    /// - `todo` is a phantom export: Test::More lists it in `@EXPORT` but
    ///   defines no such sub, so importing code that calls it fails.
    const DELIBERATELY_ABSENT: &[&str] = &["$TODO", "todo"];

    #[test]
    fn table_covers_every_callable_export() {
        for name in TEST_MORE_EXPORT {
            if DELIBERATELY_ABSENT.contains(name) {
                continue;
            }
            assert!(
                TEST_MORE_FUNCTIONS.iter().any(|f| f.name == *name),
                "`{name}` is exported by Test::More but has no completion entry"
            );
        }
    }

    #[test]
    fn table_claims_nothing_test_more_does_not_export() {
        for function in TEST_MORE_FUNCTIONS {
            assert!(
                TEST_MORE_EXPORT.contains(&function.name),
                "`{}` is not exported by Test::More",
                function.name
            );
        }
    }

    #[test]
    fn entries_are_unique_and_documented() {
        for (index, function) in TEST_MORE_FUNCTIONS.iter().enumerate() {
            assert!(
                !TEST_MORE_FUNCTIONS[..index].iter().any(|f| f.name == function.name),
                "`{}` appears twice",
                function.name
            );
            assert!(!function.signature.is_empty(), "`{}` has no signature", function.name);
            assert!(!function.description.is_empty(), "`{}` has no description", function.name);
        }
    }

    #[test]
    fn documentation_lookup_covers_the_whole_table() {
        for function in TEST_MORE_FUNCTIONS {
            let docs = get_test_more_documentation(function.name);
            assert_eq!(
                docs,
                Some((function.signature, function.description)),
                "`{}` has no hover documentation",
                function.name
            );
        }
        assert_eq!(get_test_more_documentation("not_a_test_more_function"), None);
    }
}
