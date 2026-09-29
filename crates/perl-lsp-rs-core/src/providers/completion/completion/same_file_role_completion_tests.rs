//! Public completion proof for same-file composed-role methods before the
//! workspace index has published symbols (#16809).
//!
//! These tests go through [`CompletionProvider::get_completions`], not the
//! helper-only collector. Helper-only coverage lives next to
//! `collect_all_package_members_with_source`.

use super::*;
use perl_parser_core::Parser;
use perl_tdd_support::{must, must_some_with};
use perl_workspace::workspace_index::WorkspaceIndex;
use std::sync::Arc;
use url::Url;

const SAME_FILE_ROLE_SOURCE: &str = r#"package Printable;
use Moo::Role;
sub stringify { "ok" }

package User;
use Moo;
with 'Printable';
sub own_method { 1 }
sub consume {
    my $self = shift;
    $self->
}

package main;
my $user = bless {}, 'User';
$user->"#;

const TRANSITIVE_ROLE_SOURCE: &str = r#"package RoleB;
use Moo::Role;
sub deep_method { 1 }

package RoleA;
use Moo::Role;
with 'RoleB';
sub mid_method { 1 }

package User;
use Moo;
with 'RoleA';

package main;
my $user = bless {}, 'User';
$user->"#;

const CYCLE_ROLE_SOURCE: &str = r#"package RoleA;
use Moo::Role;
with 'RoleB';
sub a_method { 1 }

package RoleB;
use Moo::Role;
with 'RoleA';
sub b_method { 1 }

package User;
use Moo;
with 'RoleA';

package main;
my $user = bless {}, 'User';
$user->"#;

const SHADOWING_SOURCE: &str = r#"package Printable;
use Moo::Role;
sub stringify { "role" }

package User;
use Moo;
with 'Printable';
sub stringify { "own" }

package main;
my $user = bless {}, 'User';
$user->"#;

const DYNAMIC_ROLE_SOURCE: &str = r#"package Printable;
use Moo::Role;
sub stringify { "ok" }

package User;
use Moo;
with $role_name;

package main;
my $user = bless {}, 'User';
$user->"#;

const UNRESOLVED_EXTERNAL_SOURCE: &str = r#"package User;
use Moo;
with 'Missing::Role';

package main;
my $user = bless {}, 'User';
$user->"#;

const LEXICAL_ROLE_SOURCE: &str = r#"package HelperRole;
use Moo::Role;
my sub helper { 1 }

package User;
use Moo;
with 'HelperRole';

package main;
my $user = bless {}, 'User';
$user->"#;

/// The consuming package is reopened later in the same file with no ancestry
/// declaration of its own (#16853). Perl has one `User` package, so the silent
/// reopen must not erase the role the earlier segment composed.
const REOPENED_ROLE_SOURCE: &str = r#"package Printable;
use Moo::Role;
sub stringify { "ok" }

package User;
use Moo;
with 'Printable';
sub own_method { 1 }

package User;
sub extra_method { 2 }

package main;
my $user = bless {}, 'User';
$user->"#;

const UNRELATED_SOURCE: &str = r#"package Other;
sub other_method { 1 }

package main;
my $other = bless {}, 'Other';
$other->"#;

fn completions_for(source: &str, index: Arc<WorkspaceIndex>) -> Vec<CompletionItem> {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let provider = CompletionProvider::new_with_index_and_source(&ast, source, Some(index));
    provider.get_completions(source, source.len())
}

fn empty_index() -> Arc<WorkspaceIndex> {
    Arc::new(WorkspaceIndex::new())
}

fn indexed_source(source: &str) -> Arc<WorkspaceIndex> {
    let index = Arc::new(WorkspaceIndex::new());
    must(
        index.index_initial_file(must(Url::parse("file:///workspace/User.pm")), source.to_string()),
    );
    index
}

fn labels(completions: &[CompletionItem]) -> Vec<&str> {
    completions.iter().map(|item| item.label.as_ref()).collect()
}

fn composed_role_item<'a>(
    completions: &'a [CompletionItem],
    name: &str,
    role: &str,
) -> Option<&'a CompletionItem> {
    let from_role = format!("from {role}");
    completions.iter().find(|item| {
        item.label == name
            && item.detail.as_deref().is_some_and(|detail| detail.contains(&from_role))
    })
}

fn method_order<'a>(completions: &'a [CompletionItem], names: &[&str]) -> Vec<&'a str> {
    completions
        .iter()
        .filter(|item| names.contains(&item.label.as_ref()))
        .map(|item| item.label.as_ref())
        .collect()
}

#[test]
fn empty_index_offers_same_file_composed_role_method() {
    let index = empty_index();
    assert!(
        !index.has_symbols(),
        "fixture must start from a workspace index with no published symbols"
    );

    let completions = completions_for(SAME_FILE_ROLE_SOURCE, index);
    let item = composed_role_item(&completions, "stringify", "Printable");
    assert!(
        item.is_some(),
        "exact same-file role method must be offered before workspace indexing; got {:?}",
        labels(&completions)
    );
}

#[test]
fn indexed_same_source_converges_with_empty_index_identity() {
    let empty_completions = completions_for(SAME_FILE_ROLE_SOURCE, empty_index());
    let indexed_completions =
        completions_for(SAME_FILE_ROLE_SOURCE, indexed_source(SAME_FILE_ROLE_SOURCE));

    let names = ["stringify", "own_method"];
    assert_eq!(
        method_order(&empty_completions, &names),
        method_order(&indexed_completions, &names),
        "pre-index and post-index candidate identity/order must converge for the same source generation"
    );
    assert!(
        composed_role_item(&indexed_completions, "stringify", "Printable").is_some(),
        "indexed control must keep the composed-role identity; got {:?}",
        labels(&indexed_completions)
    );
}

#[test]
fn consumer_method_shadows_same_named_role_method() {
    let completions = completions_for(SHADOWING_SOURCE, empty_index());
    let stringify = must_some_with(
        completions.iter().find(|item| item.label == "stringify"),
        "shadowed stringify must still be offered",
    );
    assert!(
        !stringify.detail.as_deref().is_some_and(|detail| detail.contains("from Printable")),
        "consumer method must retain precedence over the composed-role method; got {:?}",
        stringify.detail
    );
    let stringify_count = completions.iter().filter(|item| item.label == "stringify").count();
    assert_eq!(stringify_count, 1, "shadowed role method must not duplicate the consumer method");
}

#[test]
fn transitive_same_file_role_method_is_offered() {
    let completions = completions_for(TRANSITIVE_ROLE_SOURCE, empty_index());
    assert!(
        composed_role_item(&completions, "deep_method", "RoleB").is_some(),
        "transitively composed same-file role method must be offered; got {:?}",
        labels(&completions)
    );
    assert!(
        composed_role_item(&completions, "mid_method", "RoleA").is_some(),
        "direct composed same-file role method must be offered; got {:?}",
        labels(&completions)
    );
}

#[test]
fn role_cycle_terminates_without_duplicate_candidates() {
    let completions = completions_for(CYCLE_ROLE_SOURCE, empty_index());
    let a_count = completions.iter().filter(|item| item.label == "a_method").count();
    let b_count = completions.iter().filter(|item| item.label == "b_method").count();
    assert_eq!(
        a_count,
        1,
        "role cycle must not duplicate a_method; got {:?}",
        labels(&completions)
    );
    assert_eq!(
        b_count,
        1,
        "role cycle must not duplicate b_method; got {:?}",
        labels(&completions)
    );
    assert!(composed_role_item(&completions, "a_method", "RoleA").is_some());
    assert!(composed_role_item(&completions, "b_method", "RoleB").is_some());
}

#[test]
fn dynamic_role_expression_does_not_fabricate_exact_method() {
    let completions = completions_for(DYNAMIC_ROLE_SOURCE, empty_index());
    assert!(
        composed_role_item(&completions, "stringify", "Printable").is_none(),
        "dynamic role composition must stay fail-closed; got {:?}",
        labels(&completions)
    );
}

#[test]
fn lexical_role_sub_is_not_offered_as_composed_method() {
    let completions = completions_for(LEXICAL_ROLE_SOURCE, empty_index());
    assert!(
        composed_role_item(&completions, "helper", "HelperRole").is_none(),
        "lexical role sub must not be rebound as an exact composed method; got {:?}",
        labels(&completions)
    );
}

#[test]
fn unresolved_external_role_does_not_guess_workspace_methods() {
    let completions = completions_for(UNRESOLVED_EXTERNAL_SOURCE, empty_index());
    assert!(
        !completions.iter().any(|item| {
            item.detail.as_deref().is_some_and(|detail| detail.contains("Missing::Role"))
        }),
        "unresolved external role must not be guessed from its name; got {:?}",
        labels(&completions)
    );
}

#[test]
fn edited_role_method_is_visible_on_current_generation() {
    let original = SAME_FILE_ROLE_SOURCE;
    let edited = original.replace("sub stringify { \"ok\" }", "sub render { \"ok\" }");
    let completions = completions_for(&edited, empty_index());
    assert!(
        composed_role_item(&completions, "render", "Printable").is_some(),
        "current source generation must offer the edited role method; got {:?}",
        labels(&completions)
    );
    assert!(
        composed_role_item(&completions, "stringify", "Printable").is_none(),
        "stale role method name must not satisfy the new generation; got {:?}",
        labels(&completions)
    );
}

#[test]
fn stale_prior_document_cannot_satisfy_new_request() {
    let previous = SAME_FILE_ROLE_SOURCE;
    let _ = completions_for(previous, empty_index());
    let completions = completions_for(UNRELATED_SOURCE, empty_index());
    assert!(
        composed_role_item(&completions, "stringify", "Printable").is_none(),
        "a later unrelated request must not reuse prior document role methods; got {:?}",
        labels(&completions)
    );
}

#[test]
fn unrelated_empty_index_file_does_not_receive_other_package_role_methods() {
    let completions = completions_for(UNRELATED_SOURCE, empty_index());
    assert!(
        !completions.iter().any(|item| item.label == "stringify"),
        "unrelated empty-index file must not receive another package's role methods; got {:?}",
        labels(&completions)
    );
}

#[test]
fn empty_index_offers_role_method_after_silent_consumer_reopen() {
    let completions = completions_for(REOPENED_ROLE_SOURCE, empty_index());
    assert!(
        composed_role_item(&completions, "stringify", "Printable").is_some(),
        "a silent reopen of the consuming package must not erase the composed role; got {:?}",
        labels(&completions)
    );
    assert!(
        completions.iter().any(|item| item.label == "extra_method"),
        "the reopened segment's own method must still be offered; got {:?}",
        labels(&completions)
    );
    assert!(
        completions.iter().any(|item| item.label == "own_method"),
        "the first segment's own method must survive the reopen; got {:?}",
        labels(&completions)
    );
}

#[test]
fn reopened_consumer_indexed_result_converges_with_empty_index() {
    let empty_completions = completions_for(REOPENED_ROLE_SOURCE, empty_index());
    let indexed_completions =
        completions_for(REOPENED_ROLE_SOURCE, indexed_source(REOPENED_ROLE_SOURCE));

    let names = ["stringify", "own_method", "extra_method"];
    assert_eq!(
        method_order(&empty_completions, &names),
        method_order(&indexed_completions, &names),
        "pre-index and post-index candidate identity/order must converge for the reopened consumer"
    );
    assert!(
        composed_role_item(&indexed_completions, "stringify", "Printable").is_some(),
        "indexed control must keep the composed-role identity after the reopen; got {:?}",
        labels(&indexed_completions)
    );
}
