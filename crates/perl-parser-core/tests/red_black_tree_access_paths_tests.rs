//! Red/black-tree regression corpus for parser and canonical body HIR.
//!
//! Perl implementations commonly model nodes as blessed hash references and
//! express rotations as nested parent/left/right element reads and writes. This
//! suite pins those realistic access paths, including terminal write-place and
//! read-modify-write modes, without claiming the still-open PIR-A element-place
//! work tracked by #4847 is complete.

mod cpan_test_helpers;

use cpan_test_helpers::assert_clean_parse;
use perl_parser_core::Parser;
use perl_parser_core::hir::{
    AccessMode, HirBody, HirExpr, HirExprId, HirFile, SubscriptKind, lower_ast,
};

const ROTATION_SOURCE: &str = r#"
sub rotate_left {
    my ($tree, $pivot) = @_;
    my $child = $pivot->{right};

    $pivot->{right} = $child->{left};
    $child->{left}->{parent} = $pivot if $child->{left};
    $child->{parent} = $pivot->{parent};

    if (!$pivot->{parent}) {
        $tree->{root} = $child;
    } elsif ($pivot == $pivot->{parent}->{left}) {
        $pivot->{parent}->{left} = $child;
    } else {
        $pivot->{parent}->{right} = $child;
    }

    $child->{left} = $pivot;
    $pivot->{parent} = $child;
}

sub rotate_right {
    my ($tree, $pivot) = @_;
    my $child = $pivot->{left};

    $pivot->{left} = $child->{right};
    $child->{right}->{parent} = $pivot if $child->{right};
    $child->{parent} = $pivot->{parent};

    if (!$pivot->{parent}) {
        $tree->{root} = $child;
    } elsif ($pivot == $pivot->{parent}->{right}) {
        $pivot->{parent}->{right} = $child;
    } else {
        $pivot->{parent}->{left} = $child;
    }

    $child->{right} = $pivot;
    $pivot->{parent} = $child;
}
"#;

const FIXUP_SOURCE: &str = r#"
sub recolor_and_record_rotation {
    my ($tree, $node, $uncle, $direction) = @_;

    $node->{parent}->{color} = 'black';
    $uncle->{color} = 'black';
    $node->{parent}->{parent}->{color} = 'red';
    $tree->{root}->{color} = 'black';

    $tree->{stats}->{rotations}++;
    $tree->{stats}->{by_direction}->{$direction} += 1;
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

fn matches<'a>(
    paths: &'a [SubscriptPath],
    text: &str,
    access: AccessMode,
) -> Vec<&'a SubscriptPath> {
    paths.iter().filter(|path| path.text == text && path.access == access).collect()
}

fn assert_path_count(paths: &[SubscriptPath], text: &str, access: AccessMode, expected: usize) {
    let found = matches(paths, text, access);
    assert_eq!(
        found.len(),
        expected,
        "unexpected {access:?} count for {text}; all paths: {paths:#?}"
    );
}

fn assert_path_present(paths: &[SubscriptPath], text: &str, access: AccessMode) {
    let found = matches(paths, text, access);
    assert!(!found.is_empty(), "expected a {access:?} path for {text}; all paths: {paths:#?}");
}

#[test]
fn red_black_rotations_parse_cleanly() {
    assert_clean_parse(ROTATION_SOURCE);
}

#[test]
fn rotation_nested_writes_only_mark_the_terminal_edge_writable() {
    let file = lower(ROTATION_SOURCE);
    let paths = subscript_paths(ROTATION_SOURCE, &file);

    assert_path_count(&paths, "$child->{left}->{parent}", AccessMode::Write, 1);
    assert_path_count(&paths, "$child->{right}->{parent}", AccessMode::Write, 1);

    let left_parent_write = matches(&paths, "$child->{left}->{parent}", AccessMode::Write);
    assert_eq!(left_parent_write[0].kind, SubscriptKind::Hash);
    assert_eq!(left_parent_write[0].container, "$child->{left}");
    assert_eq!(left_parent_write[0].selector, "parent");

    let right_parent_write = matches(&paths, "$child->{right}->{parent}", AccessMode::Write);
    assert_eq!(right_parent_write[0].kind, SubscriptKind::Hash);
    assert_eq!(right_parent_write[0].container, "$child->{right}");
    assert_eq!(right_parent_write[0].selector, "parent");

    // Navigation to the nested target is a read; only the final `parent`
    // element is the write place.
    assert_path_present(&paths, "$child->{left}", AccessMode::Read);
    assert_path_present(&paths, "$child->{right}", AccessMode::Read);
}

#[test]
fn rotation_paths_keep_condition_reads_distinct_from_branch_writes() {
    let file = lower(ROTATION_SOURCE);
    let paths = subscript_paths(ROTATION_SOURCE, &file);

    // Each path is read in one direction's orientation test and written in
    // that selected branch; the mirror direction's fallback branch writes the
    // same path a second time, so file-wide write counts are two.
    assert_path_count(&paths, "$pivot->{parent}->{left}", AccessMode::Read, 1);
    assert_path_count(&paths, "$pivot->{parent}->{left}", AccessMode::Write, 2);
    assert_path_count(&paths, "$pivot->{parent}->{right}", AccessMode::Read, 1);
    assert_path_count(&paths, "$pivot->{parent}->{right}", AccessMode::Write, 2);

    assert_path_count(&paths, "$tree->{root}", AccessMode::Write, 2);
    assert_path_count(&paths, "$pivot->{right}", AccessMode::Read, 1);
    assert_path_count(&paths, "$pivot->{right}", AccessMode::Write, 1);
    assert_path_count(&paths, "$pivot->{left}", AccessMode::Read, 1);
    assert_path_count(&paths, "$pivot->{left}", AccessMode::Write, 1);
}

#[test]
fn red_black_fixup_preserves_terminal_color_writes() {
    assert_clean_parse(FIXUP_SOURCE);

    let file = lower(FIXUP_SOURCE);
    let paths = subscript_paths(FIXUP_SOURCE, &file);

    assert_path_count(&paths, "$node->{parent}->{color}", AccessMode::Write, 1);
    assert_path_count(&paths, "$node->{parent}->{parent}->{color}", AccessMode::Write, 1);
    assert_path_count(&paths, "$tree->{root}->{color}", AccessMode::Write, 1);
    assert_path_count(&paths, "$uncle->{color}", AccessMode::Write, 1);

    // Parent/grandparent/root navigation remains read-only while the terminal
    // colour slot carries the write.
    assert_path_present(&paths, "$node->{parent}", AccessMode::Read);
    assert_path_present(&paths, "$node->{parent}->{parent}", AccessMode::Read);
    assert_path_present(&paths, "$tree->{root}", AccessMode::Read);
}

#[test]
fn rotation_metrics_keep_one_rmw_place_and_one_computed_selector() {
    let file = lower(FIXUP_SOURCE);
    let paths = subscript_paths(FIXUP_SOURCE, &file);

    assert_path_count(&paths, "$tree->{stats}->{rotations}", AccessMode::ReadModifyWrite, 1);
    assert_path_count(
        &paths,
        "$tree->{stats}->{by_direction}->{$direction}",
        AccessMode::ReadModifyWrite,
        1,
    );

    let directional = matches(
        &paths,
        "$tree->{stats}->{by_direction}->{$direction}",
        AccessMode::ReadModifyWrite,
    );
    assert_eq!(directional[0].container, "$tree->{stats}->{by_direction}");
    assert_eq!(directional[0].selector, "$direction");

    // RMW is a single terminal-place mode; it must not be duplicated as a
    // separate read and write of the computed selector.
    assert_path_count(&paths, "$tree->{stats}->{by_direction}->{$direction}", AccessMode::Read, 0);
    assert_path_count(&paths, "$tree->{stats}->{by_direction}->{$direction}", AccessMode::Write, 0);
    assert_path_present(&paths, "$tree->{stats}", AccessMode::Read);
    assert_path_present(&paths, "$tree->{stats}->{by_direction}", AccessMode::Read);
}
