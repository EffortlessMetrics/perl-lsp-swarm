//! BDD tests for C-style for → foreach conversion (issue #18442 item 1).

use perl_lsp_rs_core::providers::code_actions::{CodeAction, CodeActionsProvider};
use perl_parser::Parser;
use perl_tdd_support::must;

fn foreach_convert_action(source: &str) -> Option<CodeAction> {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let provider = CodeActionsProvider::new(source.to_string());
    let actions = provider.get_code_actions(&ast, (0, source.len()), &[]);
    actions.into_iter().find(|action| action.title.contains("Convert to foreach loop"))
}

fn apply_action(source: &str, action: &CodeAction) -> String {
    let mut edits = action.edit.changes.clone();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.location.start));
    let mut out = source.to_string();
    for edit in edits {
        out.replace_range(edit.location.start..edit.location.end, &edit.new_text);
    }
    out
}

fn reparses_clean(source: &str) -> bool {
    Parser::new(source).parse_with_recovery().diagnostics.is_empty()
}

#[test]
fn c_style_for_with_array_subscript_converts_to_foreach_with_item() {
    let source = "for (my $i=0;$i<@items;$i++){ print $items[$i]; }\n";
    let action = foreach_convert_action(source).expect("expected foreach conversion action");
    let edited = apply_action(source, &action);
    assert_eq!(
        edited, "foreach my $item (@items) { print $item; }\n",
        "unexpected conversion output: {edited:?}"
    );
    assert!(reparses_clean(&edited), "converted source should reparse cleanly");
}

#[test]
fn c_style_for_skipped_when_iter_used_outside_subscript() {
    let source = "for (my $i=0;$i<@items;$i++){ print $items[$i], $i; }\n";
    assert!(
        foreach_convert_action(source).is_none(),
        "must not offer conversion when $i appears outside $items[$i]"
    );
}
