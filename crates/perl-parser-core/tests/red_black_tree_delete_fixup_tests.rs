//! Red/black-tree deletion regression corpus for parser and canonical body HIR.
//!
//! Deletion, transplant, successor search, and mirrored double-black fix-up
//! compose nested parent/left/right reads differently from insertion rotations.
//! These tests pin terminal write and read-modify-write access without claiming
//! algorithm correctness or the still-open PIR-A place work under #4847.

mod cpan_test_helpers;

use cpan_test_helpers::assert_clean_parse;
use perl_parser_core::Parser;
use perl_parser_core::hir::{
    AccessMode, HirBody, HirExpr, HirExprId, HirFile, SubscriptKind, lower_ast,
};

const DELETE_SOURCE: &str = r#"
sub tree_minimum {
    my ($node) = @_;
    my $cursor = $node;

    while ($cursor->{left}) {
        $cursor = $cursor->{left};
    }

    return $cursor;
}

sub transplant {
    my ($tree, $old, $new) = @_;

    if (!$old->{parent}) {
        $tree->{root} = $new;
    } elsif ($old == $old->{parent}->{left}) {
        $old->{parent}->{left} = $new;
    } else {
        $old->{parent}->{right} = $new;
    }

    $new->{parent} = $old->{parent} if $new;
}

sub delete_node {
    my ($tree, $node) = @_;
    my $successor;
    my $child;

    if (!$node->{left}) {
        $child = $node->{right};
        transplant($tree, $node, $node->{right});
    } elsif (!$node->{right}) {
        $child = $node->{left};
        transplant($tree, $node, $node->{left});
    } else {
        $successor = tree_minimum($node->{right});
        $child = $successor->{right};

        if ($successor->{parent} != $node) {
            transplant($tree, $successor, $successor->{right});
            $successor->{right} = $node->{right};
            $successor->{right}->{parent} = $successor if $successor->{right};
        }

        transplant($tree, $node, $successor);
        $successor->{left} = $node->{left};
        $successor->{left}->{parent} = $successor if $successor->{left};
        $successor->{color} = $node->{color};
    }

    return $child;
}
"#;

const FIXUP_SOURCE: &str = r#"
sub delete_fixup {
    my ($tree, $node, $case) = @_;

    while ($node != $tree->{root} && $node->{color} eq 'black') {
        if ($node == $node->{parent}->{left}) {
            my $sibling = $node->{parent}->{right};

            if ($sibling->{color} eq 'red') {
                $sibling->{color} = 'black';
                $node->{parent}->{color} = 'red';
                rotate_left($tree, $node->{parent});
                $sibling = $node->{parent}->{right};
            }

            if (
                $sibling->{left}->{color} eq 'black'
                && $sibling->{right}->{color} eq 'black'
            ) {
                $sibling->{color} = 'red';
                $node = $node->{parent};
            } else {
                if ($sibling->{right}->{color} eq 'black') {
                    $sibling->{left}->{color} = 'black';
                    $sibling->{color} = 'red';
                    rotate_right($tree, $sibling);
                    $sibling = $node->{parent}->{right};
                }

                $sibling->{color} = $node->{parent}->{color};
                $node->{parent}->{color} = 'black';
                $sibling->{right}->{color} = 'black';
                rotate_left($tree, $node->{parent});
                $node = $tree->{root};
            }
        } else {
            my $sibling = $node->{parent}->{left};

            if ($sibling->{color} eq 'red') {
                $sibling->{color} = 'black';
                $node->{parent}->{color} = 'red';
                rotate_right($tree, $node->{parent});
                $sibling = $node->{parent}->{left};
            }

            if (
                $sibling->{right}->{color} eq 'black'
                && $sibling->{left}->{color} eq 'black'
            ) {
                $sibling->{color} = 'red';
                $node = $node->{parent};
            } else {
                if ($sibling->{left}->{color} eq 'black') {
                    $sibling->{right}->{color} = 'black';
                    $sibling->{color} = 'red';
                    rotate_left($tree, $sibling);
                    $sibling = $node->{parent}->{left};
                }

                $sibling->{color} = $node->{parent}->{color};
                $node->{parent}->{color} = 'black';
                $sibling->{left}->{color} = 'black';
                rotate_right($tree, $node->{parent});
                $node = $tree->{root};
            }
        }
    }

    $node->{color} = 'black';
    $tree->{stats}->{delete_fixups}++;
    $tree->{stats}->{by_case}->{$case} += 1;
}
"#;

#[derive(Debug, PartialEq, Eq)]
struct SubscriptPath {
    text: String,
    container: String,
    selector: String,
    kind: SubscriptKind,
    access: AccessMode,
}

fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

fn expression_text(source: &str, body: &HirBody, id: HirExprId) -> String {
    let Some(range) = body.source_map.expr_range(id) else {
        return String::from("<missing-range>");
    };
    source
        .get(range.start..range.end)
        .map_or_else(|| String::from("<invalid-range>"), str::to_owned)
}

fn subscript_paths(source: &str, file: &HirFile) -> Vec<SubscriptPath> {
    let mut paths = Vec::new();
    for body in &file.bodies {
        for index in 0..body.source_map.expr_ranges.len() {
            let id = HirExprId(index as u32);
            let Some(HirExpr::Subscript(subscript)) = body.expr(id) else {
                continue;
            };
            paths.push(SubscriptPath {
                text: expression_text(source, body, id),
                container: expression_text(source, body, subscript.container),
                selector: expression_text(source, body, subscript.subscript),
                kind: subscript.kind,
                access: subscript.access,
            });
        }
    }
    paths
}

fn matching_paths<'a>(
    paths: &'a [SubscriptPath],
    text: &str,
    access: AccessMode,
) -> Vec<&'a SubscriptPath> {
    paths.iter().filter(|path| path.text == text && path.access == access).collect()
}

fn assert_path_count(paths: &[SubscriptPath], text: &str, access: AccessMode, expected: usize) {
    let found = matching_paths(paths, text, access);
    assert_eq!(
        found.len(),
        expected,
        "unexpected {access:?} count for {text}; all paths: {paths:#?}"
    );
}

fn assert_path_present(paths: &[SubscriptPath], text: &str, access: AccessMode) {
    let found = matching_paths(paths, text, access);
    assert!(!found.is_empty(), "expected a {access:?} path for {text}; all paths: {paths:#?}");
}

fn one_path<'a>(paths: &'a [SubscriptPath], text: &str, access: AccessMode) -> &'a SubscriptPath {
    let found = matching_paths(paths, text, access);
    assert_eq!(found.len(), 1, "expected one {access:?} path for {text}; all paths: {paths:#?}");
    found[0]
}

#[test]
fn red_black_deletion_sources_parse_cleanly() {
    assert_clean_parse(DELETE_SOURCE);
    assert_clean_parse(FIXUP_SOURCE);
}

#[test]
fn successor_search_keeps_loop_navigation_as_reads() {
    let file = lower(DELETE_SOURCE);
    let paths = subscript_paths(DELETE_SOURCE, &file);

    assert_path_count(&paths, "$cursor->{left}", AccessMode::Read, 2);
    assert_path_count(&paths, "$cursor->{left}", AccessMode::Write, 0);
    assert_path_count(&paths, "$cursor->{left}", AccessMode::ReadModifyWrite, 0);
}

#[test]
fn transplant_keeps_terminal_links_separate_from_parent_navigation() {
    let file = lower(DELETE_SOURCE);
    let paths = subscript_paths(DELETE_SOURCE, &file);

    assert_path_count(&paths, "$tree->{root}", AccessMode::Write, 1);
    assert_path_count(&paths, "$old->{parent}->{left}", AccessMode::Read, 1);
    assert_path_count(&paths, "$old->{parent}->{left}", AccessMode::Write, 1);
    assert_path_count(&paths, "$old->{parent}->{right}", AccessMode::Write, 1);
    assert_path_count(&paths, "$new->{parent}", AccessMode::Write, 1);
    assert_path_present(&paths, "$old->{parent}", AccessMode::Read);

    let left_write = one_path(&paths, "$old->{parent}->{left}", AccessMode::Write);
    assert_eq!(left_write.kind, SubscriptKind::Hash);
    assert_eq!(left_write.container, "$old->{parent}");
    assert_eq!(left_write.selector, "left");
}

#[test]
fn successor_relinking_writes_only_terminal_slots() {
    let file = lower(DELETE_SOURCE);
    let paths = subscript_paths(DELETE_SOURCE, &file);

    assert_path_count(&paths, "$successor->{right}->{parent}", AccessMode::Write, 1);
    assert_path_count(&paths, "$successor->{left}->{parent}", AccessMode::Write, 1);
    assert_path_count(&paths, "$successor->{color}", AccessMode::Write, 1);
    assert_path_count(&paths, "$successor->{right}", AccessMode::Write, 1);
    assert_path_count(&paths, "$successor->{left}", AccessMode::Write, 1);
    assert_path_present(&paths, "$successor->{right}", AccessMode::Read);
    assert_path_present(&paths, "$successor->{left}", AccessMode::Read);

    let parent_write = one_path(&paths, "$successor->{right}->{parent}", AccessMode::Write);
    assert_eq!(parent_write.container, "$successor->{right}");
    assert_eq!(parent_write.selector, "parent");
}

#[test]
fn mirrored_double_black_cases_keep_reads_and_writes_distinct() {
    let file = lower(FIXUP_SOURCE);
    let paths = subscript_paths(FIXUP_SOURCE, &file);

    // Exact counts pin both mirrored branches: dropping either side of the
    // if/else halves the nephew write and parent-color counts below.
    assert_path_count(&paths, "$node->{parent}->{left}", AccessMode::Read, 4);
    assert_path_count(&paths, "$node->{parent}->{left}", AccessMode::Write, 0);
    assert_path_count(&paths, "$node->{parent}->{right}", AccessMode::Read, 3);
    assert_path_count(&paths, "$node->{parent}->{right}", AccessMode::Write, 0);
    assert_path_count(&paths, "$sibling->{left}->{color}", AccessMode::Read, 3);
    assert_path_count(&paths, "$sibling->{left}->{color}", AccessMode::Write, 2);
    assert_path_count(&paths, "$sibling->{right}->{color}", AccessMode::Read, 3);
    assert_path_count(&paths, "$sibling->{right}->{color}", AccessMode::Write, 2);
    assert_path_count(&paths, "$node->{parent}->{color}", AccessMode::Read, 2);
    assert_path_count(&paths, "$node->{parent}->{color}", AccessMode::Write, 4);
    assert_path_count(&paths, "$node->{color}", AccessMode::Read, 1);
    assert_path_count(&paths, "$node->{color}", AccessMode::Write, 1);

    let left_nephew_write = matching_paths(&paths, "$sibling->{left}->{color}", AccessMode::Write);
    for path in left_nephew_write {
        assert_eq!(path.container, "$sibling->{left}");
        assert_eq!(path.selector, "color");
    }

    let right_nephew_write =
        matching_paths(&paths, "$sibling->{right}->{color}", AccessMode::Write);
    for path in right_nephew_write {
        assert_eq!(path.container, "$sibling->{right}");
        assert_eq!(path.selector, "color");
    }
}

#[test]
fn deletion_metrics_keep_single_terminal_rmw_places() {
    let file = lower(FIXUP_SOURCE);
    let paths = subscript_paths(FIXUP_SOURCE, &file);

    assert_path_count(&paths, "$tree->{stats}->{delete_fixups}", AccessMode::ReadModifyWrite, 1);
    assert_path_count(&paths, "$tree->{stats}->{by_case}->{$case}", AccessMode::ReadModifyWrite, 1);
    assert_path_count(&paths, "$tree->{stats}->{by_case}->{$case}", AccessMode::Read, 0);
    assert_path_count(&paths, "$tree->{stats}->{by_case}->{$case}", AccessMode::Write, 0);

    let by_case =
        one_path(&paths, "$tree->{stats}->{by_case}->{$case}", AccessMode::ReadModifyWrite);
    assert_eq!(by_case.kind, SubscriptKind::Hash);
    assert_eq!(by_case.container, "$tree->{stats}->{by_case}");
    assert_eq!(by_case.selector, "$case");
    assert_path_present(&paths, "$tree->{stats}", AccessMode::Read);
    assert_path_present(&paths, "$tree->{stats}->{by_case}", AccessMode::Read);
}
